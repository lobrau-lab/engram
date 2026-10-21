use crate::takeover::{Failure as TakeoverFailure, Takeover};
use anyhow::{Context, Result};
use engram_core::ai::ChatOpts;
use engram_core::capability;
use engram_core::dump;
use engram_core::embedded::{self, ChatMessage};
use engram_core::index::Index;
use engram_core::vault::validate_relative_path;
use engram_core::{
    atomic_write, build_context_prompt, Config, HashingEmbedder, LlmClient, OpenAiCompat, Vault,
};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_BODY_BYTES: usize = 512 * 1024;
const MAX_ACTIVE_CONNECTIONS: usize = 64;

struct ConnectionLimiter {
    active: AtomicUsize,
}

struct ConnectionPermit {
    limiter: Arc<ConnectionLimiter>,
}

impl ConnectionLimiter {
    fn new() -> Self {
        Self {
            active: AtomicUsize::new(0),
        }
    }

    fn try_acquire(self: &Arc<Self>) -> Option<ConnectionPermit> {
        let mut active = self.active.load(Ordering::Relaxed);
        loop {
            if active >= MAX_ACTIVE_CONNECTIONS {
                return None;
            }
            match self.active.compare_exchange_weak(
                active,
                active + 1,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    return Some(ConnectionPermit {
                        limiter: Arc::clone(self),
                    })
                }
                Err(current) => active = current,
            }
        }
    }
}

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        self.limiter.active.fetch_sub(1, Ordering::Release);
    }
}

pub fn run(vault: &Path, project_root: Option<&Path>, host: &str, port: u16) -> Result<()> {
    validate_bind_host(host)?;
    let listener = TcpListener::bind((host, port))
        .with_context(|| format!("binding local workspace at {host}:{port}"))?;
    run_listener(vault, project_root, listener, false)
}

pub fn open(vault: &Path, project_root: Option<&Path>, host: &str, port: u16) -> Result<()> {
    validate_bind_host(host)?;
    let listener = TcpListener::bind((host, port))
        .with_context(|| format!("binding local workspace at {host}:{port}"))?;
    run_listener(vault, project_root, listener, true)
}

fn validate_bind_host(host: &str) -> Result<()> {
    if matches!(host, "127.0.0.1" | "localhost" | "::1" | "[::1]") {
        return Ok(());
    }
    anyhow::bail!("Engram is local-only and only binds to localhost, 127.0.0.1, or ::1")
}

fn run_listener(
    vault: &Path,
    project_root: Option<&Path>,
    listener: TcpListener,
    launch_browser: bool,
) -> Result<()> {
    let project_root = project_root
        .map(|path| {
            let root = path
                .canonicalize()
                .with_context(|| format!("resolving project root {}", path.display()))?;
            anyhow::ensure!(root.is_dir(), "project root must be a directory");
            Ok::<_, anyhow::Error>(root)
        })
        .transpose()?;
    let address = listener.local_addr()?;
    let url = format!("http://{address}");
    let takeover = Arc::new(Takeover::new(vault.to_path_buf()));
    let connections = Arc::new(ConnectionLimiter::new());
    println!("Engram workspace: {url}");
    println!("Local-only server; press Ctrl-C to stop.");
    if launch_browser {
        let result = if cfg!(target_os = "macos") {
            std::process::Command::new("open").arg(&url).spawn()
        } else if cfg!(target_os = "windows") {
            std::process::Command::new("cmd")
                .args(["/C", "start", &url])
                .spawn()
        } else {
            std::process::Command::new("xdg-open").arg(&url).spawn()
        };
        if let Err(error) = result {
            eprintln!("could not open browser automatically: {error}");
        }
    }
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let Some(permit) = connections.try_acquire() else {
                    let mut stream = stream;
                    let _ = stream.write_all(&response(
                        503,
                        "text/plain; charset=utf-8",
                        "local workspace is busy",
                    ));
                    continue;
                };
                let vault = vault.to_path_buf();
                let project_root = project_root.clone();
                let takeover = Arc::clone(&takeover);
                if let Err(error) = std::thread::Builder::new().spawn(move || {
                    let _permit = permit;
                    if let Err(error) =
                        handle(&vault, project_root.as_deref(), &takeover, stream, address)
                    {
                        eprintln!("request failed: {error:#}");
                    }
                }) {
                    eprintln!("could not start request worker: {error}");
                }
            }
            Err(error) => eprintln!("connection failed: {error}"),
        }
    }
    Ok(())
}

struct Request {
    method: String,
    target: String,
    body: String,
    local_control: bool,
    host: String,
    origin: Option<String>,
    sec_fetch_site: Option<String>,
}

fn handle(
    vault: &Path,
    project_root: Option<&Path>,
    takeover: &Takeover,
    mut stream: TcpStream,
    address: SocketAddr,
) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let response = match read_request(&mut stream) {
        Ok(request) => match validate_request(&request, address) {
            Ok(()) => match dispatch(vault, project_root, takeover, &request) {
                Ok(response) => response,
                Err(error) => {
                    eprintln!("request failed: {error:#}");
                    response(
                        400,
                        "text/plain; charset=utf-8",
                        &format!("request failed: {error:#}"),
                    )
                }
            },
            Err(error) => response(
                403,
                "text/plain; charset=utf-8",
                &format!("rejected: {error:#}"),
            ),
        },
        Err(error) => response(
            400,
            "text/plain; charset=utf-8",
            &format!("bad request: {error:#}"),
        ),
    };
    stream.write_all(&response)?;
    Ok(())
}

fn validate_request(request: &Request, address: SocketAddr) -> Result<()> {
    anyhow::ensure!(
        host_is_local(&request.host, address),
        "unexpected Host header"
    );
    if request.method == "GET" || request.method == "HEAD" {
        return Ok(());
    }
    anyhow::ensure!(
        request.origin.is_some() || request.sec_fetch_site.is_some(),
        "missing request origin metadata"
    );
    if let Some(origin) = &request.origin {
        anyhow::ensure!(
            origin_is_local(origin, &request.host),
            "cross-origin requests are not allowed"
        );
    }
    if let Some(site) = &request.sec_fetch_site {
        anyhow::ensure!(
            site == "same-origin" || site == "none",
            "cross-site requests are not allowed"
        );
    }
    Ok(())
}

fn host_is_local(host: &str, address: SocketAddr) -> bool {
    let host = host.trim().to_ascii_lowercase();
    let port = address.port();
    let mut allowed = vec![
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
        format!("[::1]:{port}"),
    ];
    if port == 80 {
        allowed.extend([
            "127.0.0.1".to_string(),
            "localhost".to_string(),
            "[::1]".to_string(),
        ]);
    }
    allowed.iter().any(|candidate| candidate == &host)
}

fn origin_is_local(origin: &str, host: &str) -> bool {
    origin
        .trim()
        .to_ascii_lowercase()
        .eq_ignore_ascii_case(&format!("http://{}", host.trim().to_ascii_lowercase()))
}

fn dispatch(
    vault: &Path,
    project_root: Option<&Path>,
    takeover: &Takeover,
    request: &Request,
) -> Result<Vec<u8>> {
    let (path, query) = request
        .target
        .as_str()
        .split_once('?')
        .unwrap_or((request.target.as_str(), ""));
    let body = &request.body;
    let local_control = request.local_control;
    let response = match (request.method.as_str(), path) {
        ("GET", "/") => redirect("/threadline/"),
        ("GET", "/vault") => render_index(vault, query)?,
        ("GET", "/threadline") => redirect("/threadline/"),
        ("GET", "/dump") => render_dump(vault)?,
        ("GET", "/dump/review") => render_dump_review(vault, query)?,
        ("POST", "/api/v1/dump") => submit_dump(vault, body)?,
        ("POST", path) if path.starts_with("/api/v1/dump/") && path.ends_with("/analyze") => {
            analyze_dump(vault, path)?
        }
        ("POST", path) if path.starts_with("/api/v1/dump/") && path.ends_with("/review") => {
            review_dump(vault, path, body)?
        }
        ("POST", path) if path.starts_with("/api/v1/dump/") && path.ends_with("/apply") => {
            apply_dump(vault, path, body)?
        }
        ("POST", path) if path.starts_with("/api/v1/dump/") && path.ends_with("/undo") => {
            undo_dump(vault, path, body)?
        }
        ("GET", "/threadline/") | ("GET", "/threadline/index.html") => response(
            200,
            "text/html; charset=utf-8",
            include_str!("../web/index.html"),
        ),
        ("GET", asset_path) if asset_path.starts_with("/threadline/assets/") => {
            match threadline_image(asset_path) {
                Some(contents) => response_bytes(200, "image/png", contents),
                None => response(404, "text/plain; charset=utf-8", "Not found"),
            }
        }
        ("GET", asset_path) if asset_path.starts_with("/threadline/") => {
            match threadline_asset(asset_path) {
                Some((content_type, contents)) => response(200, content_type, contents),
                None => response(404, "text/plain; charset=utf-8", "Not found"),
            }
        }
        ("GET", "/api/v1/project/status") => project_status(project_root),
        ("GET", "/api/v1/project/files") => project_files(project_root, query),
        ("GET", "/api/v1/project/read") => project_read(project_root, query),
        ("POST", "/api/v1/project/write") => project_write(project_root, body),
        ("POST", "/api/v1/project/directory") => project_create_directory(project_root, body),
        ("POST", "/api/v1/project/check") => project_run_check(project_root, body),
        ("GET", "/api/v1/inference/embedded") => embedded_status(),
        ("GET", "/api/v1/apps") => list_apps(vault),
        ("POST", "/api/v1/apps/inspect") => inspect_app(vault, body),
        ("POST", "/api/v1/apps/run") => run_app(vault, body),
        ("GET", "/api/v1/workshop") => list_workshop(vault),
        ("POST", "/api/v1/workshop/propose") => workshop_propose(project_root, vault, body),
        ("GET", "/api/v1/takeover/status") => takeover_status(takeover),
        ("POST", "/api/v1/takeover/arm") if local_control => takeover_arm(takeover, body),
        ("POST", "/api/v1/takeover/disarm") if local_control => takeover_disarm(takeover),
        ("POST", "/api/v1/takeover/action") if local_control => takeover_action(takeover, body),
        ("POST", "/api/v1/takeover/permission") if local_control => takeover_permission(takeover),
        ("GET", "/api/v1/takeover/audit/export") if local_control => {
            takeover_audit_export(takeover)
        }
        ("GET", "/api/v1/takeover/audit/export") => {
            project_error(403, "local control header required")
        }
        ("POST", "/api/v1/takeover/purge") if local_control => takeover_purge(takeover),
        ("GET", path) if path.starts_with("/api/v1/takeover/screenshot/") => {
            takeover_screenshot(takeover, path)
        }
        ("POST", path) if path.starts_with("/api/v1/takeover/") => {
            project_error(403, "local control header required")
        }
        ("GET", "/v1/models") => embedded_models(),
        ("POST", "/v1/chat/completions") => chat_completions(body),
        ("GET", "/api/v1/inference/omlx") => omlx_status(),
        ("POST", "/api/v1/inference/omlx/start") if local_control => omlx_action("start"),
        ("POST", "/api/v1/inference/omlx/stop") if local_control => omlx_action("stop"),
        ("POST", "/api/v1/inference/omlx/restart") if local_control => omlx_action("restart"),
        ("POST", path) if path.starts_with("/api/v1/inference/omlx/") => {
            project_error(403, "local control header required")
        }
        ("GET", "/api/v1/vault/search") => {
            let search = parameter(query, "q").unwrap_or_default();
            if search.trim().is_empty() || search.len() > 512 {
                response(
                    400,
                    "application/json; charset=utf-8",
                    "{\"error\":\"query must contain 1–512 bytes\"}",
                )
            } else {
                match search_vault_api(vault, &search) {
                    Ok(json) => response(200, "application/json; charset=utf-8", &json),
                    Err(error) => response(
                        500,
                        "application/json; charset=utf-8",
                        &serde_json::json!({"error": error.to_string()}).to_string(),
                    ),
                }
            }
        }
        ("GET", "/note") => render_note(vault, query)?,
        ("GET", "/new") => render_new()?,
        ("POST", "/new") => create_note(vault, body)?,
        ("GET", "/graph") => render_graph(vault)?,
        ("POST", "/save") => save_note(vault, body)?,
        ("GET", "/trash") => render_trash(vault)?,
        ("POST", "/index") => rebuild_index(vault)?,
        ("POST", "/rename") => rename_note(vault, body)?,
        ("POST", "/delete") => delete_note(vault, body)?,
        ("POST", "/restore") => restore_note(vault, body)?,
        ("GET", "/ask") => render_ask()?,
        ("POST", "/ask") => ask(vault, body)?,
        _ => response(404, "text/plain; charset=utf-8", "Not found"),
    };
    Ok(response)
}

#[derive(serde::Deserialize)]
struct ChatCompletionRequest {
    #[serde(default)]
    messages: Vec<ChatMessage>,
    #[serde(default)]
    tools: Vec<serde_json::Value>,
    #[serde(default)]
    stream: bool,
    #[serde(default)]
    max_tokens: Option<u32>,
}

fn embedded_status() -> Vec<u8> {
    let status = embedded::availability();
    response(
        200,
        "application/json; charset=utf-8",
        &serde_json::json!({
            "available": status.available,
            "provider": status.provider,
            "model": status.model,
            "reason": status.reason,
        })
        .to_string(),
    )
}

fn embedded_models() -> Vec<u8> {
    let status = embedded::availability();
    let data = if status.available {
        vec![serde_json::json!({
            "id": status.model,
            "object": "model",
            "owned_by": "local",
        })]
    } else {
        Vec::new()
    };
    response(
        200,
        "application/json; charset=utf-8",
        &serde_json::json!({ "object": "list", "data": data }).to_string(),
    )
}

fn inference_error(status: u16, message: &str) -> Vec<u8> {
    response(
        status,
        "application/json; charset=utf-8",
        &serde_json::json!({
            "error": {
                "message": message,
                "type": "embedded_inference_error",
            }
        })
        .to_string(),
    )
}

fn chat_completions(body: &str) -> Vec<u8> {
    let request: ChatCompletionRequest = match serde_json::from_str(body) {
        Ok(request) => request,
        Err(error) => return inference_error(400, &format!("invalid chat request: {error}")),
    };
    if !request.tools.is_empty() {
        return inference_error(400, "the built-in provider does not support function tools");
    }
    if request.messages.is_empty() {
        return inference_error(400, "messages must contain at least one entry");
    }
    let max_tokens = request.max_tokens.unwrap_or(1024).clamp(1, 4096);
    let created = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let content = match embedded::complete(&request.messages, Some(max_tokens)) {
        Ok(content) => content,
        Err(error) => return inference_error(503, &error.to_string()),
    };
    let model = embedded::availability().model;

    if request.stream {
        let chunk = serde_json::json!({
            "id": format!("chatcmpl-embedded-{created}"),
            "object": "chat.completion.chunk",
            "created": created,
            "model": model,
            "choices": [{
                "index": 0,
                "delta": { "role": "assistant", "content": content },
                "finish_reason": serde_json::Value::Null,
            }],
        });
        let stop = serde_json::json!({
            "id": format!("chatcmpl-embedded-{created}"),
            "object": "chat.completion.chunk",
            "created": created,
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "stop",
            }],
        });
        let body = format!("data: {chunk}\n\ndata: {stop}\n\ndata: [DONE]\n\n");
        return response_bytes(200, "text/event-stream; charset=utf-8", body.as_bytes());
    }

    response(
        200,
        "application/json; charset=utf-8",
        &serde_json::json!({
            "id": format!("chatcmpl-embedded-{created}"),
            "object": "chat.completion",
            "created": created,
            "model": model,
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": content },
                "finish_reason": "stop",
            }],
            "usage": serde_json::Value::Null,
        })
        .to_string(),
    )
}

#[derive(serde::Deserialize)]
struct InspectAppRequest {
    path: String,
}

#[derive(serde::Deserialize)]
struct RunAppRequest {
    id: String,
    action: String,
}

#[derive(serde::Deserialize)]
struct TakeoverArmRequest {
    #[serde(default = "default_takeover_minutes")]
    minutes: u64,
}

fn default_takeover_minutes() -> u64 {
    15
}

fn takeover_status(takeover: &Takeover) -> Vec<u8> {
    response(
        200,
        "application/json; charset=utf-8",
        &takeover.status().to_string(),
    )
}

fn takeover_arm(takeover: &Takeover, body: &str) -> Vec<u8> {
    let request: TakeoverArmRequest = match serde_json::from_str(body) {
        Ok(request) => request,
        Err(error) => {
            return takeover_error(400, &format!("invalid takeover arm request: {error}"))
        }
    };
    match takeover.arm(request.minutes) {
        Ok(value) => response(200, "application/json; charset=utf-8", &value.to_string()),
        Err(error) => takeover_failure(error),
    }
}

fn takeover_disarm(takeover: &Takeover) -> Vec<u8> {
    response(
        200,
        "application/json; charset=utf-8",
        &takeover.disarm().to_string(),
    )
}

fn takeover_action(takeover: &Takeover, body: &str) -> Vec<u8> {
    match takeover.action(body) {
        Ok(value) => response(200, "application/json; charset=utf-8", &value.to_string()),
        Err(error) => takeover_failure(error),
    }
}

fn takeover_permission(takeover: &Takeover) -> Vec<u8> {
    match takeover.request_permission() {
        Ok(value) => response(200, "application/json; charset=utf-8", &value.to_string()),
        Err(error) => takeover_failure(error),
    }
}

fn takeover_audit_export(takeover: &Takeover) -> Vec<u8> {
    match takeover.audit_export() {
        Ok(bytes) => response_bytes(200, "application/x-ndjson; charset=utf-8", &bytes),
        Err(error) => takeover_failure(error),
    }
}

fn takeover_purge(takeover: &Takeover) -> Vec<u8> {
    match takeover.purge() {
        Ok(value) => response(200, "application/json; charset=utf-8", &value.to_string()),
        Err(error) => takeover_failure(error),
    }
}

fn takeover_screenshot(takeover: &Takeover, path: &str) -> Vec<u8> {
    let name = path
        .strip_prefix("/api/v1/takeover/screenshot/")
        .unwrap_or_default();
    match takeover.screenshot_bytes(name) {
        Ok(bytes) => response_bytes(200, "image/png", &bytes),
        Err(error) => takeover_failure(error),
    }
}

fn takeover_failure(error: TakeoverFailure) -> Vec<u8> {
    takeover_error(error.status, &error.message)
}

fn takeover_error(status: u16, message: &str) -> Vec<u8> {
    response(
        status,
        "application/json; charset=utf-8",
        &serde_json::json!({ "ok": false, "error": message }).to_string(),
    )
}

fn list_apps(vault: &Path) -> Vec<u8> {
    match capability::list(vault) {
        Ok(apps) => response(
            200,
            "application/json; charset=utf-8",
            &serde_json::json!({ "apps": apps }).to_string(),
        ),
        Err(error) => inference_error(500, &error.to_string()),
    }
}

fn inspect_app(vault: &Path, body: &str) -> Vec<u8> {
    let request: InspectAppRequest = match serde_json::from_str(body) {
        Ok(request) => request,
        Err(error) => return inference_error(400, &format!("invalid inspect request: {error}")),
    };
    if request.path.trim().is_empty() || request.path.len() > 2000 {
        return inference_error(400, "provide a path between 1 and 2000 bytes");
    }
    match capability::inspect(vault, &request.path) {
        Ok(digest) => response(
            200,
            "application/json; charset=utf-8",
            &serde_json::to_string(&digest).unwrap_or_else(|_| "{}".into()),
        ),
        Err(error) => inference_error(400, &error.to_string()),
    }
}

fn run_app(vault: &Path, body: &str) -> Vec<u8> {
    let request: RunAppRequest = match serde_json::from_str(body) {
        Ok(request) => request,
        Err(error) => return inference_error(400, &format!("invalid run request: {error}")),
    };
    match capability::run(vault, &request.id, &request.action) {
        Ok(result) => response(
            200,
            "application/json; charset=utf-8",
            &serde_json::to_string(&result).unwrap_or_else(|_| "{}".into()),
        ),
        Err(error) => inference_error(400, &error.to_string()),
    }
}

#[derive(serde::Deserialize)]
struct WorkshopRequest {
    goal: String,
    #[serde(default)]
    files: Vec<String>,
}

fn list_workshop(vault: &Path) -> Vec<u8> {
    let dir = vault.join(".engram").join("workshop");
    let mut proposals = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|part| part.to_str()) != Some("md") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let created = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs())
                .unwrap_or_default();
            let title = content
                .lines()
                .find_map(|line| line.strip_prefix("# Workshop proposal:"))
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .unwrap_or("Workshop proposal");
            proposals.push((
                created,
                serde_json::json!({
                    "id": path.file_stem().and_then(|part| part.to_str()).unwrap_or("proposal"),
                    "title": title,
                    "created": created,
                    "content": content,
                }),
            ));
        }
    }
    proposals.sort_by_key(|left| std::cmp::Reverse(left.0));
    let proposals = proposals
        .into_iter()
        .take(12)
        .map(|(_, proposal)| proposal)
        .collect::<Vec<_>>();
    response(
        200,
        "application/json; charset=utf-8",
        &serde_json::json!({ "proposals": proposals }).to_string(),
    )
}

fn workshop_propose(project_root: Option<&Path>, vault: &Path, body: &str) -> Vec<u8> {
    let Some(root) = project_root else {
        return project_error(
            403,
            "the workshop is disabled; restart with --project-root <directory> to grant source access",
        );
    };
    let request: WorkshopRequest = match serde_json::from_str(body) {
        Ok(request) => request,
        Err(error) => return inference_error(400, &format!("invalid workshop request: {error}")),
    };
    let goal = request.goal.trim();
    if goal.is_empty() || goal.len() > 4000 {
        return inference_error(400, "provide a goal between 1 and 4000 bytes");
    }
    if request.files.is_empty() || request.files.len() > 3 {
        return inference_error(400, "choose between 1 and 3 source files");
    }

    let root = match root.canonicalize() {
        Ok(root) => root,
        Err(error) => return inference_error(500, &error.to_string()),
    };
    let mut excerpts = String::new();
    let mut included = Vec::new();
    for relative in &request.files {
        if relative.len() > 500 {
            return inference_error(400, "a source file path is too long");
        }
        if is_sensitive_project_path(Path::new(relative)) {
            return inference_error(
                403,
                "sensitive or hidden project paths are not available to the workshop",
            );
        }
        let path = match resolve_project_path(&root, relative, false) {
            Ok(path) => path,
            Err(error) => return inference_error(400, &error.to_string()),
        };
        if !path.is_file() {
            return inference_error(400, "workshop paths must be regular files");
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(_) => return inference_error(415, "workshop files must be UTF-8 text"),
        };
        let excerpt = truncate_text(&content, 8000);
        excerpts.push_str(&format!(
            "\n--- FILE: {} ---\n{}\n",
            relative.trim(),
            excerpt
        ));
        included.push(relative.trim().to_string());
        if excerpts.len() > 24000 {
            return inference_error(
                400,
                "the selected source files exceed the workshop context budget",
            );
        }
    }

    let system = "You are the Threadline Workshop, a careful senior engineer. Propose the smallest reviewable change to Engram's own source code. Never claim a change was applied, tested, or merged. Never propose disabling tests, consent, Host/Origin checks, hidden-path checks, hash verification, or any other security control. Do not use tools or run commands. Output exactly these sections: 1) Intent, 2) Files, 3) Unified diff, 4) Tests, 5) Risks.";
    let prompt = format!(
        "GOAL:\n{goal}\n\nSOURCE EXCERPTS:{excerpts}\n\nReturn a proposal only. The human will review, apply, and test it."
    );
    let proposal = match engram_core::embedded::complete(
        &[
            ChatMessage {
                role: "system".into(),
                content: system.into(),
            },
            ChatMessage {
                role: "user".into(),
                content: prompt,
            },
        ],
        Some(1600),
    ) {
        Ok(proposal) => proposal,
        Err(error) => return inference_error(503, &error.to_string()),
    };

    let created = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let id = format!("{created}-{}", workshop_slug(goal));
    let path = vault
        .join(".engram")
        .join("workshop")
        .join(format!("{id}.md"));
    let document = format!(
        "# Workshop proposal: {goal}\n\n- Created: {created}\n- Files: {}\n- Status: proposed, not applied\n\n{proposal}\n",
        included.join(", ")
    );
    if let Err(error) = atomic_write::replace(&path, document.as_bytes()) {
        return inference_error(500, &error.to_string());
    }
    response(
        200,
        "application/json; charset=utf-8",
        &serde_json::json!({
            "id": id,
            "title": goal,
            "files": included,
            "proposal": document,
            "path": path.to_string_lossy(),
            "status": "proposed",
        })
        .to_string(),
    )
}

fn truncate_text(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_string();
    }
    let mut end = limit;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}

fn workshop_slug(value: &str) -> String {
    let mut slug = String::new();
    let mut previous_dash = false;
    for character in value.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            slug.push(character);
            previous_dash = false;
        } else if !previous_dash && !slug.is_empty() {
            slug.push('-');
            previous_dash = true;
        }
        if slug.len() >= 40 {
            break;
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "proposal".into()
    } else {
        slug.to_string()
    }
}

fn omlx_cli() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        candidates.push(PathBuf::from(home).join(".omlx/bin/omlx"));
    }
    candidates.extend([
        PathBuf::from("/opt/homebrew/bin/omlx"),
        PathBuf::from("/usr/local/bin/omlx"),
    ]);
    candidates.into_iter().find(|path| {
        if !path.is_file() {
            return false;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::metadata(path)
                .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
        }
        #[cfg(not(unix))]
        {
            true
        }
    })
}

fn omlx_status() -> Vec<u8> {
    let supported = cfg!(target_os = "macos");
    let cli_available = supported && omlx_cli().is_some();
    let address: SocketAddr = "127.0.0.1:8000".parse().expect("fixed loopback address");
    let endpoint_listening =
        supported && TcpStream::connect_timeout(&address, Duration::from_millis(250)).is_ok();
    let message = if !supported {
        "Threadline's oMLX lifecycle controls are available on macOS."
    } else if !cli_available {
        "oMLX CLI not found. Launch oMLX or install its official CLI shim to enable lifecycle controls."
    } else if endpoint_listening {
        "A service is listening on oMLX's default loopback port. This does not prove that a model is loaded."
    } else {
        "No service is listening on oMLX's default loopback port."
    };
    response(
        200,
        "application/json; charset=utf-8",
        &serde_json::json!({
            "provider": "oMLX", "supported": supported, "cliAvailable": cli_available,
            "endpointListening": endpoint_listening, "message": message
        })
        .to_string(),
    )
}

fn omlx_action(action: &str) -> Vec<u8> {
    if !cfg!(target_os = "macos") {
        return response(
            501,
            "application/json; charset=utf-8",
            r#"{"ok":false,"error":"oMLX lifecycle controls are available on macOS only."}"#,
        );
    }
    if !matches!(action, "start" | "stop" | "restart") {
        return response(
            400,
            "application/json; charset=utf-8",
            r#"{"ok":false,"error":"Unsupported oMLX action."}"#,
        );
    }
    let Some(cli) = omlx_cli() else {
        return response(
            503,
            "application/json; charset=utf-8",
            r#"{"ok":false,"error":"oMLX CLI not found. Launch oMLX or install its official CLI shim."}"#,
        );
    };
    // Fixed executable and fixed subcommands only: never invoke a shell or
    // accept a command path/argument from the browser.
    let result = std::process::Command::new(cli).arg(action).output();
    match result {
        Ok(output) if output.status.success() => response(200, "application/json; charset=utf-8", &serde_json::json!({
            "ok": true, "action": action, "message": format!("oMLX {action} command completed.")
        }).to_string()),
        Ok(output) => {
            let detail = String::from_utf8_lossy(&output.stderr);
            let detail = detail.trim();
            let detail = if detail.is_empty() { format!("oMLX {action} exited with {}.", output.status) } else { detail.chars().take(1200).collect() };
            response(500, "application/json; charset=utf-8", &serde_json::json!({"ok": false, "error": detail}).to_string())
        }
        Err(error) => response(500, "application/json; charset=utf-8", &serde_json::json!({"ok": false, "error": format!("Could not run the oMLX CLI: {error}")}).to_string()),
    }
}

fn project_status(root: Option<&Path>) -> Vec<u8> {
    match root {
        Some(root) => response(200, "application/json; charset=utf-8", &serde_json::json!({"enabled": true, "root": root.display().to_string(), "access": ["list", "read", "write"]}).to_string()),
        None => response(200, "application/json; charset=utf-8", r#"{"enabled":false,"access":[]}"#),
    }
}

fn project_files(root: Option<&Path>, query: &str) -> Vec<u8> {
    let Some(root) = root else {
        return project_disabled();
    };
    let relative = parameter(query, "path").unwrap_or_else(|| ".".to_string());
    let directory = if relative == "." {
        root.to_path_buf()
    } else {
        match resolve_project_path(root, &relative, false) {
            Ok(path) if path.is_dir() => path,
            Ok(_) => {
                return response(
                    400,
                    "application/json; charset=utf-8",
                    r#"{"error":"path is not a directory"}"#,
                )
            }
            Err(error) => return project_error(400, &error.to_string()),
        }
    };
    let mut files = Vec::new();
    for entry in WalkDir::new(&directory)
        .min_depth(1)
        .max_depth(5)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            if entry.depth() == 0 {
                return true;
            }
            entry
                .path()
                .strip_prefix(root)
                .ok()
                .is_some_and(|relative| !is_sensitive_project_path(relative))
        })
        .filter_map(Result::ok)
    {
        if files.len() >= 500 {
            break;
        }
        let path = entry.path();
        let rel = match path.strip_prefix(root) {
            Ok(path) => path,
            Err(_) => continue,
        };
        if entry.file_type().is_file() && !is_sensitive_project_path(rel) {
            files.push(serde_json::json!({"path": rel.to_string_lossy(), "bytes": entry.metadata().map(|meta| meta.len()).unwrap_or(0)}));
        }
    }
    response(
        200,
        "application/json; charset=utf-8",
        &serde_json::json!({"files": files, "truncated": files.len() >= 500}).to_string(),
    )
}

fn project_read(root: Option<&Path>, query: &str) -> Vec<u8> {
    let Some(root) = root else {
        return project_disabled();
    };
    let Some(relative) = parameter(query, "path") else {
        return project_error(400, "path is required");
    };
    match resolve_project_path(root, &relative, false) {
        Ok(path) if path.is_file() && !is_sensitive_project_path(Path::new(&relative)) => {
            match std::fs::metadata(&path) {
                Ok(meta) if meta.len() <= 1024 * 1024 => match std::fs::read_to_string(&path) {
                    Ok(content) => response(
                        200,
                        "application/json; charset=utf-8",
                        &serde_json::json!({"path": relative, "content": content}).to_string(),
                    ),
                    Err(_) => project_error(415, "file is not UTF-8 text"),
                },
                Ok(_) => project_error(413, "file exceeds the 1 MiB read limit"),
                Err(error) => project_error(400, &error.to_string()),
            }
        }
        Ok(_) => project_error(400, "path is not a readable project file"),
        Err(error) => project_error(400, &error.to_string()),
    }
}

fn project_write(root: Option<&Path>, body: &str) -> Vec<u8> {
    let Some(root) = root else {
        return project_disabled();
    };
    let payload: serde_json::Value = match serde_json::from_str(body) {
        Ok(value) => value,
        Err(_) => return project_error(400, "expected a JSON object"),
    };
    let Some(relative) = payload.get("path").and_then(serde_json::Value::as_str) else {
        return project_error(400, "path is required");
    };
    let Some(content) = payload.get("content").and_then(serde_json::Value::as_str) else {
        return project_error(400, "content is required");
    };
    if content.len() > 512 * 1024 {
        return project_error(413, "file exceeds the 512 KiB write limit");
    }
    if is_sensitive_project_path(Path::new(relative)) {
        return project_error(403, "writes to sensitive or hidden paths are blocked");
    }
    let path = match resolve_project_path(root, relative, true) {
        Ok(path) => path,
        Err(error) => return project_error(400, &error.to_string()),
    };
    if path.exists() && !path.is_file() {
        return project_error(400, "target is not a regular file");
    }
    if let Some(parent) = path.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            return project_error(500, &error.to_string());
        }
    }
    if let Err(error) = atomic_write::replace(&path, content.as_bytes()) {
        return project_error(500, &error.to_string());
    }
    response(
        200,
        "application/json; charset=utf-8",
        &serde_json::json!({"written": true, "path": relative, "bytes": content.len()}).to_string(),
    )
}

fn resolve_project_path(root: &Path, raw: &str, allow_missing: bool) -> Result<PathBuf> {
    let relative = Path::new(raw);
    anyhow::ensure!(!relative.is_absolute(), "absolute paths are not allowed");
    anyhow::ensure!(
        relative
            .components()
            .all(|part| matches!(part, Component::Normal(_))),
        "path must contain only normal relative components"
    );
    anyhow::ensure!(!raw.is_empty(), "path cannot be empty");
    ensure_no_project_symlinks(root, relative)?;
    let candidate = root.join(relative);
    let checked = if candidate.exists() {
        candidate.canonicalize().context("resolving project path")?
    } else if allow_missing {
        let parent = candidate
            .parent()
            .context("target has no parent")?
            .canonicalize()
            .context("parent directory must already exist")?;
        parent.join(candidate.file_name().context("target has no filename")?)
    } else {
        anyhow::bail!("project path does not exist")
    };
    anyhow::ensure!(
        checked.starts_with(root),
        "path escapes the granted project root"
    );
    Ok(checked)
}

fn ensure_no_project_symlinks(root: &Path, relative: &Path) -> Result<()> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) => anyhow::ensure!(
                !metadata.file_type().is_symlink(),
                "symbolic links are not allowed in project paths"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn project_create_directory(root: Option<&Path>, body: &str) -> Vec<u8> {
    let Some(root) = root else {
        return project_disabled();
    };
    let payload: serde_json::Value = match serde_json::from_str(body) {
        Ok(value) => value,
        Err(_) => return project_error(400, "expected a JSON object"),
    };
    let Some(relative) = payload.get("path").and_then(serde_json::Value::as_str) else {
        return project_error(400, "path is required");
    };
    let rel = Path::new(relative);
    if relative.trim().is_empty() {
        return project_error(400, "path is required");
    }
    if is_sensitive_project_path(rel) {
        return project_error(403, "hidden or sensitive directory paths are blocked");
    }
    if rel.is_absolute()
        || rel
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return project_error(400, "path must contain only normal relative components");
    }
    let mut current = root.to_path_buf();
    for component in rel.components() {
        current.push(component.as_os_str());
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return project_error(400, "directory path crosses a symlink or non-directory");
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if let Err(error) = std::fs::create_dir(&current) {
                    return project_error(500, &error.to_string());
                }
            }
            Err(error) => return project_error(500, &error.to_string()),
        }
    }
    response(
        200,
        "application/json; charset=utf-8",
        &serde_json::json!({"created": true, "path": relative}).to_string(),
    )
}

fn is_sensitive_project_path(path: &Path) -> bool {
    path.components().any(|part| match part {
        Component::Normal(name) => {
            let name = name.to_string_lossy().to_ascii_lowercase();
            matches!(
                name.as_str(),
                ".git"
                    | ".engram"
                    | ".ssh"
                    | ".aws"
                    | ".codex"
                    | ".secrets"
                    | "node_modules"
                    | "target"
                    | "vendor"
            ) || name == ".env"
                || name.starts_with(".env.")
        }
        _ => true,
    })
}

fn project_run_check(root: Option<&Path>, body: &str) -> Vec<u8> {
    let Some(root) = root else {
        return project_disabled();
    };
    let payload: serde_json::Value = match serde_json::from_str(body) {
        Ok(value) => value,
        Err(_) => return project_error(400, "expected a JSON object"),
    };
    let Some(task) = payload.get("task").and_then(serde_json::Value::as_str) else {
        return project_error(400, "task is required");
    };
    let (program, args, label): (&str, &[&str], &str) = match task {
        "git_status" => ("git", &["status", "--short"], "git status --short"),
        "git_diff_check" => ("git", &["diff", "--check"], "git diff --check"),
        "cargo_check_cli_offline" => (
            "cargo",
            &["check", "--offline", "-p", "engram-cli"],
            "cargo check --offline -p engram-cli",
        ),
        "cargo_test_cli_offline" => (
            "cargo",
            &["test", "--offline", "-p", "engram-cli"],
            "cargo test --offline -p engram-cli",
        ),
        _ => {
            return project_error(
                400,
                "unsupported task; only fixed local/offline checks are allowed",
            )
        }
    };
    let output = match std::process::Command::new(program)
        .args(args)
        .current_dir(root)
        .output()
    {
        Ok(output) => output,
        Err(error) => return project_error(500, &format!("could not run {label}: {error}")),
    };
    let success = output.status.success();
    let exit_code = output.status.code();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!(
        "{}{}",
        stdout,
        if stderr.is_empty() {
            String::new()
        } else {
            format!("\n{stderr}")
        }
    );
    let truncated = combined.len() > 64 * 1024;
    let output = if truncated {
        combined.chars().take(64 * 1024).collect::<String>()
    } else {
        combined
    };
    response(200, "application/json; charset=utf-8", &serde_json::json!({"task": label, "success": success, "exitCode": exit_code, "output": output, "truncated": truncated}).to_string())
}

fn project_disabled() -> Vec<u8> {
    project_error(
        403,
        "project file tools are disabled; restart with --project-root <directory>",
    )
}
fn project_error(status: u16, message: &str) -> Vec<u8> {
    response(
        status,
        "application/json; charset=utf-8",
        &serde_json::json!({"error": message}).to_string(),
    )
}

fn read_request(stream: &mut TcpStream) -> Result<Request> {
    let mut header = Vec::new();
    let mut byte = [0u8; 1];
    while !header.windows(4).any(|w| w == b"\r\n\r\n") {
        if header.len() >= MAX_HEADER_BYTES {
            anyhow::bail!("request headers exceed limit")
        }
        stream
            .read_exact(&mut byte)
            .context("reading request headers")?;
        header.push(byte[0]);
    }
    let text = std::str::from_utf8(&header).context("request headers are not UTF-8")?;
    let mut lines = text.split("\r\n");
    let request_line = lines.next().context("missing request line")?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().context("missing HTTP method")?.to_string();
    let target = parts.next().context("missing HTTP target")?.to_string();
    let version = parts.next().context("missing HTTP version")?;
    anyhow::ensure!(
        version == "HTTP/1.1" || version == "HTTP/1.0",
        "unsupported HTTP version"
    );
    let mut content_length = None;
    let mut local_control = false;
    let mut host = None;
    let mut origin = None;
    let mut sec_fetch_site = None;
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').context("malformed HTTP header")?;
        if name.eq_ignore_ascii_case("transfer-encoding") {
            anyhow::bail!("transfer encoding is unsupported")
        }
        if name.eq_ignore_ascii_case("x-threadline-local-control") && value.trim() == "1" {
            local_control = true;
        }
        if name.eq_ignore_ascii_case("content-length") {
            if content_length.is_some() {
                anyhow::bail!("duplicate content-length")
            }
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .context("invalid content-length")?,
            );
        }
        if name.eq_ignore_ascii_case("host") {
            if host.is_some() {
                anyhow::bail!("duplicate host")
            }
            host = Some(value.trim().to_string());
        }
        if name.eq_ignore_ascii_case("origin") {
            if origin.is_some() {
                anyhow::bail!("duplicate origin")
            }
            origin = Some(value.trim().to_string());
        }
        if name.eq_ignore_ascii_case("sec-fetch-site") {
            if sec_fetch_site.is_some() {
                anyhow::bail!("duplicate sec-fetch-site")
            }
            sec_fetch_site = Some(value.trim().to_ascii_lowercase());
        }
    }
    let length = content_length.unwrap_or(0);
    if length > MAX_BODY_BYTES {
        anyhow::bail!("request body exceeds limit")
    }
    let mut body = vec![0u8; length];
    stream
        .read_exact(&mut body)
        .context("reading complete request body")?;
    Ok(Request {
        method,
        target,
        body: String::from_utf8(body).context("request body is not UTF-8")?,
        local_control,
        host: host.context("missing Host header")?,
        origin,
        sec_fetch_site,
    })
}

fn render_index(vault: &Path, query: &str) -> Result<Vec<u8>> {
    let search = parameter(query, "q").unwrap_or_default();
    let notes = Vault::open(vault.to_path_buf())?.scan()?;
    let mut body = String::from("<header style=\"display:flex;align-items:center;gap:16px\"><img src=\"/threadline/assets/engram-logo.png\" alt=\"Engram\" width=\"112\" height=\"112\" style=\"object-fit:contain\"><h1>Engram</h1></header><form><input name=\"q\" placeholder=\"Search\" value=\"");
    body.push_str(&escape(&search));
    body.push_str("\"><button>Search</button></form><p><a href=\"/new\">New note</a> · <a href=\"/ask\">Ask the local assistant</a> · <a href=\"/graph\">Open graph</a> · <a href=\"/trash\">Trash</a> · <a href=\"/threadline/\">Open Threadline chat</a></p><form method=\"post\" action=\"/index\"><button>Rebuild index</button></form><ul>");
    for note in notes {
        let haystack = format!("{} {}", note.title, note.body).to_lowercase();
        if !search.is_empty() && !haystack.contains(&search.to_lowercase()) {
            continue;
        }
        body.push_str(&format!(
            "<li><a href=\"/note?path={}\">{}</a> <small>{}</small></li>",
            encode(&note.id),
            escape(&note.title),
            escape(&note.id)
        ));
    }
    body.push_str("</ul><p>On-device workspace. Markdown files remain the source of truth.</p>");
    Ok(page("Vault", &body))
}

fn search_vault_api(vault: &Path, query: &str) -> Result<String> {
    let config_path = vault.join(".engram").join("config.toml");
    let config = if config_path.exists() {
        Config::load(&config_path)?
    } else {
        Config::new(vault.to_path_buf())
    };
    let index = Index::open(&config.db)?;
    let indexed = index.get_last_indexed()?.is_some();
    let hits = index.search(query, false, 6)?;
    let hits: Vec<_> = hits
        .into_iter()
        .map(|hit| {
            let id = hit.id;
            serde_json::json!({
                "id": id,
                "title": hit.title,
                "snippet": hit.snippet,
                "score": hit.score,
                "url": format!("/note?path={}", encode(&id)),
            })
        })
        .collect();
    Ok(serde_json::json!({ "query": query, "indexed": indexed, "hits": hits }).to_string())
}

fn render_dump(vault: &Path) -> Result<Vec<u8>> {
    let items = dump::list(vault)?;
    let rows = items.iter().map(|item| format!("<li><b>{}</b> <small>{} · {} bytes · {}</small> <a href=\"/dump/review?id={}\">Review</a> <form method=\"post\" action=\"/api/v1/dump/{}/analyze\" style=\"display:inline\"><button>Analyze locally</button></form></li>", escape(&item.filename), escape(&item.id), item.content.len(), escape(&item.status), encode(&item.id), encode(&item.id))).collect::<String>();
    let html = format!("<a href=\"/vault\">← Vault</a><h1>Dump / Inbox</h1><p>Supported now: pasted text, Markdown, and plain text. Items remain pending and are not indexed until approval.</p><textarea id=\"paste\" placeholder=\"Paste text here…\"></textarea><button id=\"submit-paste\">Add pasted text</button><p><input id=\"files\" type=\"file\" multiple accept=\".md,.markdown,.txt,text/plain,text/markdown\"></p><ul>{rows}</ul><script>const send=(filename,source,content)=>fetch('/api/v1/dump',{{method:'POST',headers:{{'content-type':'application/x-www-form-urlencoded'}},body:new URLSearchParams({{filename,source,content}})}}).then(r=>r.ok?r.text():r.text().then(t=>Promise.reject(t))).then(()=>location.reload());document.querySelector('#submit-paste').onclick=()=>send('pasted-text.txt','paste',document.querySelector('#paste').value);document.querySelector('#files').onchange=e=>Promise.all([...e.target.files].map(f=>new Promise((ok,bad)=>{{const r=new FileReader();r.onload=()=>send(f.name,'file',r.result).then(ok,bad);r.onerror=bad;r.readAsText(f)}})));</script>");
    Ok(page("Dump / Inbox", &html))
}

fn render_dump_review(vault: &Path, query: &str) -> Result<Vec<u8>> {
    let id = parameter(query, "id").context("missing dump id")?;
    let item = dump::get(vault, &id)?;
    let analysis = dump::get_analysis(vault, &id).ok();
    let suggestions = analysis.map(|a| a.suggestions).unwrap_or_default();
    let cards = suggestions.iter().map(|s| format!("<article><h3>{} · {}</h3><p><b>Source evidence:</b> {}</p><p>{}</p><form method=\"post\" action=\"/api/v1/dump/{}/review\"><input type=\"hidden\" name=\"suggestion_id\" value=\"{}\"><label>Path <input name=\"path\" value=\"{}\"></label><label>Title <input name=\"title\" value=\"{}\"></label><label>Body <textarea name=\"body\">{}</textarea></label><button name=\"status\" value=\"accepted\">Accept proposal</button><button name=\"status\" value=\"rejected\">Reject</button></form><h4>Preview</h4><pre>{}</pre>{}<form method=\"post\" action=\"/api/v1/dump/{}/apply\"><input type=\"hidden\" name=\"suggestion_id\" value=\"{}\"><button>Apply accepted proposal</button></form>{}</article>", escape(&s.kind), escape(&s.review_status), escape(&s.evidence), escape(&s.rationale), encode(&id), encode(&s.id), escape(s.edited_path.as_deref().or(s.path.as_deref()).unwrap_or("")), escape(s.edited_title.as_deref().or(s.title.as_deref()).unwrap_or("")), escape(s.edited_body.as_deref().or(s.body.as_deref()).unwrap_or("")), escape(&format!("# {}\n\n{}", s.edited_title.as_deref().or(s.title.as_deref()).unwrap_or("Untitled"), s.edited_body.as_deref().or(s.body.as_deref()).unwrap_or(""))), if s.review_status == "accepted" { "" } else { "<small>Accept before applying.</small>" }, encode(&id), encode(&s.id), if s.review_status == "applied" { format!("<form method=\"post\" action=\"/api/v1/dump/{}/undo\"><input type=\"hidden\" name=\"suggestion_id\" value=\"{}\"><button>Undo</button></form>", encode(&id), encode(&s.id)) } else { String::new() })).collect::<String>();
    let html = format!("<a href=\"/dump\">← Inbox</a><h1>Review dump item</h1><p><b>Original source:</b> {}</p><pre>{}</pre><section>{}</section>", escape(&item.filename), escape(&item.content), cards);
    Ok(page("Dump review", &html))
}

fn submit_dump(vault: &Path, body: &str) -> Result<Vec<u8>> {
    let filename = parameter(body, "filename").unwrap_or_else(|| "pasted-text.txt".into());
    let source = parameter(body, "source").unwrap_or_else(|| "paste".into());
    let content = parameter(body, "content").context("missing dump content")?;
    let item = dump::submit(vault, &filename, &source, &content)?;
    Ok(response(
        201,
        "application/json; charset=utf-8",
        &serde_json::json!({"id": item.id, "status": item.status}).to_string(),
    ))
}

fn analyze_dump(vault: &Path, path: &str) -> Result<Vec<u8>> {
    let id = path
        .strip_prefix("/api/v1/dump/")
        .and_then(|path| path.strip_suffix("/analyze"))
        .unwrap_or("");
    let config_path = vault.join(".engram").join("config.toml");
    let config = Config::load(&config_path).context("local inference configuration is missing")?;
    let client = OpenAiCompat::new(
        config.llm_base,
        config.llm_model,
        config.llm_key,
        config.provider,
    );
    let analysis = dump::analyze(vault, id, &client)?;
    Ok(response(
        200,
        "application/json; charset=utf-8",
        &serde_json::to_string(&analysis)?,
    ))
}

fn review_dump(vault: &Path, path: &str, body: &str) -> Result<Vec<u8>> {
    let id = path
        .strip_prefix("/api/v1/dump/")
        .and_then(|x| x.strip_suffix("/review"))
        .unwrap_or("");
    dump::review(
        vault,
        id,
        &parameter(body, "suggestion_id").context("missing suggestion id")?,
        &parameter(body, "status").unwrap_or_default(),
        parameter(body, "path"),
        parameter(body, "title"),
        parameter(body, "body"),
    )?;
    Ok(redirect(&format!("/dump/review?id={}", encode(id))))
}

fn apply_dump(vault: &Path, path: &str, body: &str) -> Result<Vec<u8>> {
    let id = path
        .strip_prefix("/api/v1/dump/")
        .and_then(|x| x.strip_suffix("/apply"))
        .unwrap_or("");
    let suggestion_id = parameter(body, "suggestion_id").context("missing suggestion id")?;
    let report = dump::apply(vault, id, &[suggestion_id])?;
    Ok(response(
        200,
        "application/json; charset=utf-8",
        &serde_json::to_string(&report)?,
    ))
}

fn undo_dump(vault: &Path, path: &str, body: &str) -> Result<Vec<u8>> {
    let id = path
        .strip_prefix("/api/v1/dump/")
        .and_then(|x| x.strip_suffix("/undo"))
        .unwrap_or("");
    dump::undo(
        vault,
        id,
        &parameter(body, "suggestion_id").context("missing suggestion id")?,
    )?;
    Ok(redirect(&format!("/dump/review?id={}", encode(id))))
}

fn render_graph(vault: &Path) -> Result<Vec<u8>> {
    let notes = Vault::open(vault.to_path_buf())?.scan()?;
    let mut body = String::from("<a href=\"/vault\">← Vault</a><h1>Knowledge graph</h1><svg viewBox=\"0 0 900 520\" style=\"width:100%;border:1px solid #ddd;background:#fafafa\">");
    for (index, note) in notes.iter().enumerate() {
        let x = 100 + (index % 5) * 170;
        let y = 100 + (index / 5) * 150;
        for link in &note.links {
            if let Some(target) = notes.iter().position(|candidate| {
                candidate.id == *link
                    || candidate.path.file_stem().and_then(|s| s.to_str()) == Some(link)
            }) {
                let tx = 100 + (target % 5) * 170;
                let ty = 100 + (target / 5) * 150;
                body.push_str(&format!(
                    "<line x1=\"{x}\" y1=\"{y}\" x2=\"{tx}\" y2=\"{ty}\" stroke=\"#999\"/>",
                ));
            }
        }
    }
    for (index, note) in notes.iter().enumerate() {
        let x = 100 + (index % 5) * 170;
        let y = 100 + (index / 5) * 150;
        body.push_str(&format!("<a href=\"/note?path={}\"><circle cx=\"{x}\" cy=\"{y}\" r=\"34\" fill=\"#356\"/><text x=\"{x}\" y=\"{}\" text-anchor=\"middle\" fill=\"white\" font-size=\"12\">{}</text></a>", encode(&note.id), y + 4, escape(&note.title.chars().take(18).collect::<String>())));
    }
    body.push_str("</svg><p>Edges are extracted from local <code>[[wikilinks]]</code>; Markdown remains the source of truth.</p>");
    Ok(page("Knowledge graph", &body))
}

fn render_note(vault: &Path, query: &str) -> Result<Vec<u8>> {
    let relative = parameter(query, "path").context("missing note path")?;
    let relative = validate_relative_path(Path::new(&relative))?;
    anyhow::ensure!(is_markdown(&relative), "notes must use the .md extension");
    let path = safe_path(vault, &relative, false)?;
    let note = std::fs::read_to_string(&path)
        .with_context(|| format!("reading note {}", relative.display()))?;
    let vault_handle = Vault::open(vault.to_path_buf())?;
    let notes = vault_handle.scan()?;
    let note_meta = vault_handle.read_note(&relative.to_string_lossy())?;
    let target = relative.to_string_lossy();
    let stem = relative
        .file_stem()
        .and_then(|x| x.to_str())
        .unwrap_or_default();
    let mut backlinks = String::from("<h2>Backlinks</h2><ul>");
    for candidate in &notes {
        if candidate
            .links
            .iter()
            .any(|link| link.as_str() == target.as_ref() || link.as_str() == stem)
        {
            backlinks.push_str(&format!(
                "<li><a href=\"/note?path={}\">{}</a></li>",
                encode(&candidate.id),
                escape(&candidate.title)
            ));
        }
    }
    backlinks.push_str("</ul>");
    let mut broken = String::new();
    for link in &note_meta.links {
        if !notes.iter().any(|candidate| {
            candidate.id == *link
                || candidate.path.file_stem().and_then(|s| s.to_str()) == Some(link.as_str())
        }) {
            broken.push_str(&format!("<li>{}</li>", escape(link)));
        }
    }
    let broken = if broken.is_empty() {
        String::new()
    } else {
        format!("<h2>Broken links</h2><ul>{broken}</ul>")
    };
    let relative_text = relative.to_string_lossy();
    let body = format!(
        "<a href=\"/vault\">← Vault</a> · <a href=\"/trash\">Trash</a><h1>{}</h1><form method=\"post\" action=\"/save\"><input type=\"hidden\" name=\"path\" value=\"{}\"><textarea name=\"body\" autofocus>{}</textarea><button>Save</button></form><h2>Preview</h2><article class=\"preview\">{}</article>{}{}<h2>Rename note</h2><form method=\"post\" action=\"/rename\" onsubmit=\"return confirm('Rename ' + this.elements.path.value + ' to ' + this.elements.new_path.value + '?')\"><input type=\"hidden\" name=\"path\" value=\"{}\"><input name=\"new_path\" value=\"{}\" required><button>Rename note</button></form><h2>Move to trash</h2><form method=\"post\" action=\"/delete\" onsubmit=\"return confirm('Move ' + this.elements.path.value + ' to trash?')\"><input type=\"hidden\" name=\"path\" value=\"{}\"><button>Move to trash</button></form>",
        escape(relative_text.as_ref()),
        encode(relative_text.as_ref()),
        escape(&note),
        markdown_html(&note),
        backlinks,
        broken,
        escape(relative_text.as_ref()),
        escape(relative_text.as_ref()),
        escape(relative_text.as_ref())
    );
    Ok(page("Edit note", &body))
}

fn vault_index(vault: &Path) -> Result<(Index, usize)> {
    let config_path = vault.join(".engram").join("config.toml");
    let config = if config_path.exists() {
        Config::load(&config_path)?
    } else {
        Config::new(vault)
    };
    Ok((Index::open(&config.db)?, config.embed_dim))
}

fn rebuild_index(vault: &Path) -> Result<Vec<u8>> {
    let (index, embed_dim) = vault_index(vault)?;
    let notes = Vault::open(vault.to_path_buf())?.scan()?;
    index.rebuild(&notes, &HashingEmbedder::new(embed_dim), unix_secs())?;
    audit_mutation(vault, "index", "vault", "ok");
    Ok(redirect("/vault"))
}

fn upsert_note_index(vault: &Path, relative: &Path) -> Result<()> {
    let id = relative.to_string_lossy().replace('\\', "/");
    let (index, embed_dim) = vault_index(vault)?;
    let note = Vault::open(vault.to_path_buf())?.read_note(&id)?;
    index.upsert(&note, &HashingEmbedder::new(embed_dim))
}

fn remove_note_index(vault: &Path, relative: &Path) -> Result<()> {
    let id = relative.to_string_lossy().replace('\\', "/");
    let (index, _) = vault_index(vault)?;
    index.remove(&id)
}

fn save_note(vault: &Path, body: &str) -> Result<Vec<u8>> {
    let relative = parameter(body, "path").context("missing note path")?;
    let relative = validate_relative_path(Path::new(&relative))?;
    anyhow::ensure!(is_markdown(&relative), "notes must use the .md extension");
    let content = parameter(body, "body").context("missing note body")?;
    let path = safe_path(vault, &relative, true)?;
    atomic_write::replace(&path, content.as_bytes())
        .with_context(|| format!("writing note {}", relative.display()))?;
    upsert_note_index(vault, &relative)?;
    Ok(redirect("/"))
}

fn render_new() -> Result<Vec<u8>> {
    Ok(page("New note", "<a href=\"/vault\">← Vault</a><h1>New note</h1><form method=\"post\" action=\"/new\"><input name=\"path\" placeholder=\"notes/my-note.md\" required><textarea name=\"body\" placeholder=\"# Title\n\nWrite here…\" autofocus></textarea><button>Create note</button></form>"))
}

fn create_note(vault: &Path, body: &str) -> Result<Vec<u8>> {
    let relative = parameter(body, "path").context("missing note path")?;
    let relative = validate_relative_path(Path::new(&relative))?;
    anyhow::ensure!(
        is_markdown(&relative),
        "new notes must use the .md extension"
    );
    let path = safe_path(vault, &relative, true)?;
    if path.exists() {
        anyhow::bail!("note already exists: {}", relative.display())
    }
    atomic_write::replace(
        &path,
        parameter(body, "body").unwrap_or_default().as_bytes(),
    )?;
    upsert_note_index(vault, &relative)?;
    Ok(redirect(&format!(
        "/note?path={}",
        encode(relative.to_string_lossy().as_ref())
    )))
}

#[derive(serde::Serialize, serde::Deserialize)]
struct TrashManifest {
    original_path: String,
    deleted_at: i64,
}

struct TrashEntry {
    id: String,
    original_path: String,
    deleted_at: i64,
}

fn valid_trash_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
}

fn unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(unix)]
fn restrict(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
}

#[cfg(not(unix))]
fn restrict(_path: &Path, _mode: u32) {}

fn audit_mutation(vault: &Path, action: &str, path: &str, result: &str) {
    let directory = vault.join(".engram").join("audit");
    if std::fs::create_dir_all(&directory).is_err() {
        return;
    }
    restrict(&directory, 0o700);
    let entry = serde_json::json!({
        "at": unix_secs(),
        "action": action,
        "path": path,
        "result": result,
    });
    let audit_path = directory.join("vault.jsonl");
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    if let Ok(mut file) = options.open(&audit_path) {
        let _ = writeln!(file, "{entry}");
    }
    restrict(&audit_path, 0o600);
}

fn trash_note(vault: &Path, relative: &Path) -> Result<String> {
    let source = safe_path(vault, relative, false)?;
    let root = vault.join(".engram").join("trash");
    std::fs::create_dir_all(&root)?;
    restrict(&root, 0o700);
    let stamp = unix_secs();
    let mut trash_dir = None;
    for sequence in 0..1000 {
        let candidate = root.join(format!("{stamp}-{sequence}"));
        match std::fs::create_dir(&candidate) {
            Ok(()) => {
                trash_dir = Some(candidate);
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    let directory = trash_dir.context("could not allocate a trash entry")?;
    restrict(&directory, 0o700);
    let destination = directory.join(relative);
    std::fs::create_dir_all(destination.parent().context("trash path has no parent")?)?;
    let manifest = TrashManifest {
        original_path: relative.to_string_lossy().replace('\\', "/"),
        deleted_at: stamp,
    };
    let manifest_path = directory.join("manifest.json");
    atomic_write::replace(&manifest_path, &serde_json::to_vec_pretty(&manifest)?)?;
    restrict(&manifest_path, 0o600);
    if let Err(error) = std::fs::rename(&source, &destination) {
        let _ = std::fs::remove_dir_all(&directory);
        return Err(error.into());
    }
    directory
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .context("trash entry has no id")
}

fn trash_entries(vault: &Path) -> Result<Vec<TrashEntry>> {
    let root = vault.join(".engram").join("trash");
    let mut entries = Vec::new();
    let Ok(read_dir) = std::fs::read_dir(&root) else {
        return Ok(entries);
    };
    for entry in read_dir {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        if !valid_trash_id(&id) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(entry.path().join("manifest.json")) else {
            continue;
        };
        let Ok(manifest) = serde_json::from_str::<TrashManifest>(&text) else {
            continue;
        };
        entries.push(TrashEntry {
            id,
            original_path: manifest.original_path,
            deleted_at: manifest.deleted_at,
        });
    }
    entries.sort_by(|left, right| {
        right
            .deleted_at
            .cmp(&left.deleted_at)
            .then_with(|| right.id.cmp(&left.id))
    });
    Ok(entries)
}

fn render_trash(vault: &Path) -> Result<Vec<u8>> {
    let mut rows = String::new();
    for entry in trash_entries(vault)? {
        rows.push_str(&format!(
            "<li><strong>{}</strong><br><small>Deleted {}</small><form method=\"post\" action=\"/restore\" onsubmit=\"return confirm('Restore ' + this.elements.original.value + '?')\"><input type=\"hidden\" name=\"id\" value=\"{}\"><input type=\"hidden\" name=\"original\" value=\"{}\"><button>Restore</button></form></li>",
            escape(&entry.original_path),
            entry.deleted_at,
            encode(&entry.id),
            escape(&entry.original_path)
        ));
    }
    if rows.is_empty() {
        rows.push_str("<li>Trash is empty.</li>");
    }
    Ok(page(
        "Trash",
        &format!("<a href=\"/vault\">← Vault</a><h1>Trash</h1><p>Deleted notes stay under <code>.engram/trash</code> until restored.</p><ul>{rows}</ul>"),
    ))
}

fn rename_note(vault: &Path, body: &str) -> Result<Vec<u8>> {
    let from = parameter(body, "path").context("missing note path")?;
    let to = parameter(body, "new_path").context("missing new note path")?;
    let from_relative = validate_relative_path(Path::new(&from))?;
    let to_relative = validate_relative_path(Path::new(&to))?;
    anyhow::ensure!(
        is_markdown(&from_relative) && is_markdown(&to_relative),
        "notes must use the .md extension"
    );
    let from_path = safe_path(vault, &from_relative, false)?;
    let to_path = safe_path(vault, &to_relative, true)?;
    if from_path == to_path {
        return Ok(redirect(&format!(
            "/note?path={}",
            encode(to_relative.to_string_lossy().as_ref())
        )));
    }
    anyhow::ensure!(
        !to_path.exists(),
        "destination already exists: {}",
        to_relative.display()
    );
    std::fs::hard_link(&from_path, &to_path)
        .with_context(|| format!("renaming note {}", from_relative.display()))?;
    if let Err(error) = std::fs::remove_file(&from_path) {
        let _ = std::fs::remove_file(&to_path);
        return Err(error.into());
    }
    remove_note_index(vault, &from_relative)?;
    upsert_note_index(vault, &to_relative)?;
    audit_mutation(
        vault,
        "rename",
        &format!(
            "{} -> {}",
            from_relative.to_string_lossy(),
            to_relative.to_string_lossy()
        ),
        "ok",
    );
    Ok(redirect(&format!(
        "/note?path={}",
        encode(to_relative.to_string_lossy().as_ref())
    )))
}

fn delete_note(vault: &Path, body: &str) -> Result<Vec<u8>> {
    let relative = parameter(body, "path").context("missing note path")?;
    let relative = validate_relative_path(Path::new(&relative))?;
    anyhow::ensure!(is_markdown(&relative), "notes must use the .md extension");
    let id = trash_note(vault, &relative)?;
    remove_note_index(vault, &relative)?;
    audit_mutation(
        vault,
        "delete",
        &format!("{} -> trash/{id}", relative.to_string_lossy()),
        "ok",
    );
    Ok(redirect("/vault"))
}

fn restore_note(vault: &Path, body: &str) -> Result<Vec<u8>> {
    let id = parameter(body, "id").context("missing trash entry")?;
    anyhow::ensure!(valid_trash_id(&id), "invalid trash entry");
    let directory = vault.join(".engram").join("trash").join(&id);
    let manifest: TrashManifest = serde_json::from_str(
        &std::fs::read_to_string(directory.join("manifest.json"))
            .context("reading trash manifest")?,
    )
    .context("parsing trash manifest")?;
    let relative = validate_relative_path(Path::new(&manifest.original_path))?;
    anyhow::ensure!(is_markdown(&relative), "notes must use the .md extension");
    let source = directory.join(&relative);
    let metadata = std::fs::symlink_metadata(&source).context("trashed note is missing")?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "invalid trashed note"
    );
    let target = safe_path(vault, &relative, true)?;
    anyhow::ensure!(
        !target.exists(),
        "restore destination already exists: {}",
        relative.display()
    );
    std::fs::hard_link(&source, &target).context("restoring note")?;
    if let Err(error) = std::fs::remove_file(&source) {
        let _ = std::fs::remove_file(&target);
        return Err(error.into());
    }
    let _ = std::fs::remove_dir_all(&directory);
    upsert_note_index(vault, &relative)?;
    audit_mutation(
        vault,
        "restore",
        &format!("trash/{id} -> {}", relative.to_string_lossy()),
        "ok",
    );
    Ok(redirect(&format!(
        "/note?path={}",
        encode(relative.to_string_lossy().as_ref())
    )))
}

fn render_ask() -> Result<Vec<u8>> {
    Ok(page("Ask Engram", "<a href=\"/vault\">← Vault</a><h1>Ask Engram</h1><form method=\"post\" action=\"/ask\"><textarea name=\"question\" placeholder=\"Ask about your local notes\" autofocus></textarea><button>Ask</button></form><p>Answers use the configured on-device OpenAI-compatible endpoint and cite note ids.</p>"))
}

fn ask(vault: &Path, body: &str) -> Result<Vec<u8>> {
    let question = parameter(body, "question").context("missing question")?;
    let config = Config::load(&vault.join(".engram").join("config.toml"))?;
    let index = Index::open(&config.db)?;
    let hits = index.search(&question, false, 6)?;
    let mut notes = Vec::new();
    for hit in hits {
        notes.push((
            hit.id.clone(),
            index.get_note_body(&hit.id)?.unwrap_or_default(),
        ));
    }
    let prompt = build_context_prompt(&question, &notes, config.context_window.saturating_mul(4));
    let client = OpenAiCompat::new(
        config.llm_base,
        config.llm_model,
        config.llm_key,
        config.provider,
    );
    let answer = client.complete(&prompt, &ChatOpts::default())?;
    let history = vault.join(".engram").join("conversations");
    std::fs::create_dir_all(&history)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    std::fs::write(
        history.join(format!("{stamp}.md")),
        format!("# {question}\n\n## Question\n\n{question}\n\n## Answer\n\n{answer}\n"),
    )?;
    Ok(page(
        "Answer",
        &format!(
            "<a href=\"/ask\">← Ask again</a><h1>Answer</h1><pre>{}</pre>",
            escape(&answer)
        ),
    ))
}

fn has_hidden_component(path: &Path) -> bool {
    path.components().any(|component| match component {
        Component::Normal(name) => name.to_string_lossy().starts_with('.'),
        _ => false,
    })
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}

/// Reject symlinks in every existing component so a note path cannot reach
/// outside the vault through a link placed inside it.
fn ensure_no_symlink_components(root: &Path, relative: &Path) -> Result<()> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) => anyhow::ensure!(
                !metadata.file_type().is_symlink(),
                "symbolic links are not allowed in notes"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn safe_path(vault: &Path, relative: &Path, for_write: bool) -> Result<std::path::PathBuf> {
    anyhow::ensure!(
        !has_hidden_component(relative),
        "hidden paths are not accessible"
    );
    let root = vault.canonicalize().context("canonicalizing vault root")?;
    let candidate = root.join(relative);
    ensure_no_symlink_components(&root, relative)?;
    if for_write {
        if let Some(parent) = candidate.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
    }
    let checked = if candidate.exists() {
        candidate
            .canonicalize()
            .context("canonicalizing vault path")?
    } else if for_write {
        let parent = candidate
            .parent()
            .context("note has no parent")?
            .canonicalize()?;
        parent.join(candidate.file_name().context("note has no filename")?)
    } else {
        anyhow::bail!("note does not exist")
    };
    if !checked.starts_with(&root) {
        anyhow::bail!("note path escapes the vault")
    }
    Ok(checked)
}

fn page(title: &str, body: &str) -> Vec<u8> {
    response(200, "text/html; charset=utf-8", &format!(
        "<!doctype html><meta charset=utf-8><title>{}</title><style>body{{font:16px system-ui;max-width:900px;margin:40px auto;padding:0 20px}}textarea{{width:100%;height:55vh;font:15px monospace}}a{{color:#36c}}.preview{{border:1px solid #ddd;padding:20px;line-height:1.55}}code,pre{{background:#f3f3f3;padding:2px 5px}}</style>{}",
        escape(title), body
    ))
}

fn response(status: u16, content_type: &str, body: &str) -> Vec<u8> {
    response_bytes(status, content_type, body.as_bytes())
}

fn response_bytes(status: u16, content_type: &str, body: &[u8]) -> Vec<u8> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        _ => "Internal Server Error",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nX-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let mut response = Vec::with_capacity(header.len() + body.len());
    response.extend_from_slice(header.as_bytes());
    response.extend_from_slice(body);
    response
}

/// Static Threadline modules are embedded in the binary so `engram open`
/// remains a single-binary local workspace.
fn threadline_asset(path: &str) -> Option<(&'static str, &'static str)> {
    let asset = match path {
        "/threadline/styles.css" => ("text/css; charset=utf-8", include_str!("../web/styles.css")),
        "/threadline/src/app.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../web/src/app.js"),
        ),
        "/threadline/src/core/store.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../web/src/core/store.js"),
        ),
        "/threadline/src/core/memory-manager.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../web/src/core/memory-manager.js"),
        ),
        "/threadline/src/core/context-packer.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../web/src/core/context-packer.js"),
        ),
        "/threadline/src/core/evaluation-bench.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../web/src/core/evaluation-bench.js"),
        ),
        "/threadline/src/core/app-actions.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../web/src/core/app-actions.js"),
        ),
        "/threadline/src/core/agent-execution.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../web/src/core/agent-execution.js"),
        ),
        "/threadline/src/core/tool-registry.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../web/src/core/tool-registry.js"),
        ),
        "/threadline/src/core/permission-gate.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../web/src/core/permission-gate.js"),
        ),
        "/threadline/src/core/privacy-policy.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../web/src/core/privacy-policy.js"),
        ),
        "/threadline/src/providers/openai-compatible.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../web/src/providers/openai-compatible.js"),
        ),
        "/threadline/src/providers/engram-vault.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../web/src/providers/engram-vault.js"),
        ),
        _ => return None,
    };
    Some(asset)
}

fn threadline_image(path: &str) -> Option<&'static [u8]> {
    match path {
        "/threadline/assets/engram-logo.png" => {
            Some(include_bytes!("../web/assets/engram-logo.png"))
        }
        "/threadline/assets/engram-mark.png" => {
            Some(include_bytes!("../web/assets/engram-mark.png"))
        }
        "/threadline/assets/engram-mark-green.png" => {
            Some(include_bytes!("../web/assets/engram-mark-green.png"))
        }
        "/threadline/assets/engram-icon-purple.png" => {
            Some(include_bytes!("../web/assets/engram-icon-purple.png"))
        }
        "/threadline/assets/engram-icon-pink.png" => {
            Some(include_bytes!("../web/assets/engram-icon-pink.png"))
        }
        "/threadline/assets/engram-icon-blue.png" => {
            Some(include_bytes!("../web/assets/engram-icon-blue.png"))
        }
        "/threadline/assets/engram-icon-fire.png" => {
            Some(include_bytes!("../web/assets/engram-icon-fire.png"))
        }
        "/threadline/assets/engram-icon-forest.png" => {
            Some(include_bytes!("../web/assets/engram-icon-forest.png"))
        }
        "/threadline/assets/engram-icon-ice.png" => {
            Some(include_bytes!("../web/assets/engram-icon-ice.png"))
        }
        _ => None,
    }
}

fn redirect(location: &str) -> Vec<u8> {
    format!("HTTP/1.1 303 See Other\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").into_bytes()
}

fn parameter(input: &str, name: &str) -> Option<String> {
    input.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == name).then(|| decode(value))
    })
}

fn decode(input: &str) -> String {
    let input = input.replace('+', " ");
    let bytes = input.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(value) = u8::from_str_radix(&input[i + 1..i + 3], 16) {
                output.push(value);
                i += 3;
                continue;
            }
        }
        output.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&output).into_owned()
}

fn encode(input: &str) -> String {
    input.bytes().fold(String::new(), |mut out, byte| {
        if byte.is_ascii_alphanumeric() || b"-_.~/".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
        out
    })
}

fn escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn markdown_html(markdown: &str) -> String {
    let mut html = String::new();
    let mut in_code = false;
    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            if in_code {
                html.push_str("</code></pre>");
            } else {
                html.push_str("<pre><code>");
            }
            in_code = !in_code;
            continue;
        }
        if in_code {
            html.push_str(&escape(line));
            html.push('\n');
        } else if let Some(title) = line.strip_prefix("# ") {
            html.push_str(&format!("<h1>{}</h1>", escape(title)));
        } else if let Some(title) = line.strip_prefix("## ") {
            html.push_str(&format!("<h2>{}</h2>", escape(title)));
        } else if let Some(item) = line.strip_prefix("- ") {
            html.push_str(&format!("<li>{}</li>", escape(item)));
        } else if line.trim().is_empty() {
            html.push_str("<br>");
        } else {
            html.push_str(&format!("<p>{}</p>", escape(line)));
        }
    }
    if in_code {
        html.push_str("</code></pre>");
    }
    html
}

#[cfg(test)]
mod tests {
    use super::{
        chat_completions, create_note, delete_note, encode, has_hidden_component, host_is_local,
        is_markdown, origin_is_local, rename_note, restore_note, save_note, search_vault_api,
        threadline_asset, trash_entries, validate_bind_host, validate_request, ConnectionLimiter,
        Request,
    };
    use std::net::SocketAddr;
    use std::path::Path;

    #[test]
    fn accepts_loopback_bind_hosts() {
        for host in ["127.0.0.1", "localhost", "::1", "[::1]"] {
            assert!(validate_bind_host(host).is_ok(), "{host}");
        }
    }

    #[test]
    fn rejects_non_loopback_and_ambiguous_hosts() {
        for host in ["0.0.0.0", "192.168.1.10", "10.0.0.2", "example.com", "::"] {
            assert!(validate_bind_host(host).is_err(), "{host}");
        }
    }

    #[test]
    fn accepts_only_the_loopback_hosts_it_was_bound_to() {
        let address: SocketAddr = "127.0.0.1:8765".parse().unwrap();
        for host in ["127.0.0.1:8765", "localhost:8765", "[::1]:8765"] {
            assert!(host_is_local(host, address), "{host}");
        }
        for host in [
            "evil.example",
            "127.0.0.1:9999",
            "localhost",
            "127.0.0.1:8765.evil.example",
        ] {
            assert!(!host_is_local(host, address), "{host}");
        }
    }

    #[test]
    fn rejects_cross_origin_state_changes() {
        assert!(origin_is_local("http://localhost:8765", "localhost:8765"));
        assert!(origin_is_local("http://127.0.0.1:8765", "127.0.0.1:8765"));
        assert!(!origin_is_local("http://evil.example", "localhost:8765"));
        assert!(!origin_is_local("null", "localhost:8765"));
    }

    #[test]
    fn rejects_state_changes_without_origin_metadata() {
        let address: SocketAddr = "127.0.0.1:8765".parse().unwrap();
        let request = Request {
            method: "POST".into(),
            target: "/api/v1/takeover/arm".into(),
            body: "{}".into(),
            local_control: true,
            host: "127.0.0.1:8765".into(),
            origin: None,
            sec_fetch_site: None,
        };

        assert!(validate_request(&request, address).is_err());
    }

    #[test]
    fn embedded_chat_reports_unsupported_tool_calls() {
        let response = chat_completions(
            r#"{"messages":[{"role":"user","content":"hello"}],"tools":[{"type":"function","function":{"name":"computer_use"}}]}"#,
        );
        let response = String::from_utf8_lossy(&response);

        assert!(response.starts_with("HTTP/1.1 400 Bad Request"));
        assert!(response.contains("does not support function tools"));
    }

    #[test]
    fn connection_limiter_caps_active_requests_and_releases_slots() {
        let limiter = std::sync::Arc::new(ConnectionLimiter::new());
        let mut permits = (0..super::MAX_ACTIVE_CONNECTIONS)
            .map(|_| limiter.try_acquire().expect("available slot"))
            .collect::<Vec<_>>();

        assert!(limiter.try_acquire().is_none());
        drop(permits.pop());
        assert!(limiter.try_acquire().is_some());
    }

    #[test]
    fn note_lifecycle_keeps_the_index_and_trash_consistent() {
        let vault = std::env::temp_dir().join(format!(
            "engram-note-lifecycle-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&vault).unwrap();

        create_note(
            &vault,
            &format!(
                "path={}&body={}",
                encode("notes/one.md"),
                encode("# Quasar\n\nunique alpha")
            ),
        )
        .unwrap();
        save_note(
            &vault,
            &format!(
                "path={}&body={}",
                encode("notes/one.md"),
                encode("# Quasar\n\nunique beta")
            ),
        )
        .unwrap();
        rename_note(
            &vault,
            &format!(
                "path={}&new_path={}",
                encode("notes/one.md"),
                encode("notes/two.md")
            ),
        )
        .unwrap();
        assert!(!vault.join("notes/one.md").exists());
        assert!(vault.join("notes/two.md").exists());

        let search: serde_json::Value =
            serde_json::from_str(&search_vault_api(&vault, "unique").unwrap()).unwrap();
        assert_eq!(search["hits"][0]["id"], "notes/two.md");

        delete_note(&vault, &format!("path={}", encode("notes/two.md"))).unwrap();
        assert!(!vault.join("notes/two.md").exists());
        let search: serde_json::Value =
            serde_json::from_str(&search_vault_api(&vault, "unique").unwrap()).unwrap();
        assert!(search["hits"].as_array().unwrap().is_empty());
        let entries = trash_entries(&vault).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].original_path, "notes/two.md");

        restore_note(&vault, &format!("id={}", encode(&entries[0].id))).unwrap();
        assert!(vault.join("notes/two.md").exists());
        let search: serde_json::Value =
            serde_json::from_str(&search_vault_api(&vault, "unique").unwrap()).unwrap();
        assert_eq!(search["hits"][0]["id"], "notes/two.md");

        create_note(
            &vault,
            &format!(
                "path={}&body={}",
                encode("notes/three.md"),
                encode("# Three")
            ),
        )
        .unwrap();
        assert!(rename_note(
            &vault,
            &format!(
                "path={}&new_path={}",
                encode("notes/two.md"),
                encode("notes/three.md")
            ),
        )
        .is_err());
        assert!(vault.join("notes/two.md").exists());
        assert!(rename_note(
            &vault,
            &format!(
                "path={}&new_path={}",
                encode("notes/two.md"),
                encode("../escape.md")
            ),
        )
        .is_err());
        assert!(delete_note(&vault, &format!("path={}", encode(".engram/config.toml")),).is_err());

        let audit = std::fs::read_to_string(vault.join(".engram/audit/vault.jsonl")).unwrap();
        assert!(audit.contains("\"rename\""));
        assert!(audit.contains("\"delete\""));
        assert!(audit.contains("\"restore\""));

        let _ = std::fs::remove_dir_all(vault);
    }

    #[test]
    fn hidden_and_non_markdown_note_paths_are_refused() {
        for path in [".engram/config.toml", "notes/.hidden.md", ".git/config"] {
            assert!(has_hidden_component(Path::new(path)), "{path}");
        }
        for path in ["note.md", "notes/visible.md"] {
            assert!(!has_hidden_component(Path::new(path)), "{path}");
        }
        assert!(is_markdown(Path::new("note.md")));
        assert!(is_markdown(Path::new("note.MD")));
        assert!(!is_markdown(Path::new("note.toml")));
        assert!(!is_markdown(Path::new("note")));
    }

    #[test]
    fn serves_every_threadline_module_imported_by_app() {
        for path in [
            "/threadline/src/core/evaluation-bench.js",
            "/threadline/src/core/store.js",
            "/threadline/src/core/memory-manager.js",
            "/threadline/src/core/context-packer.js",
            "/threadline/src/core/app-actions.js",
            "/threadline/src/core/agent-execution.js",
            "/threadline/src/core/permission-gate.js",
            "/threadline/src/core/tool-registry.js",
            "/threadline/src/core/privacy-policy.js",
            "/threadline/src/providers/openai-compatible.js",
            "/threadline/src/providers/engram-vault.js",
        ] {
            assert!(
                threadline_asset(path).is_some(),
                "missing module route: {path}"
            );
        }
    }
}
