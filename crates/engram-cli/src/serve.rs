use anyhow::{Context, Result};
use engram_core::ai::ChatOpts;
use engram_core::dump;
use engram_core::index::Index;
use engram_core::vault::validate_relative_path;
use engram_core::{build_context_prompt, Config, HashingEmbedder, LlmClient, OpenAiCompat, Vault};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;

pub fn run(vault: &Path, host: &str, port: u16) -> Result<()> {
    let listener = TcpListener::bind((host, port))
        .with_context(|| format!("binding local workspace at {host}:{port}"))?;
    run_listener(vault, listener, false)
}

pub fn open(vault: &Path, host: &str, port: u16) -> Result<()> {
    let listener = TcpListener::bind((host, port))
        .with_context(|| format!("binding local workspace at {host}:{port}"))?;
    run_listener(vault, listener, true)
}

fn run_listener(vault: &Path, listener: TcpListener, launch_browser: bool) -> Result<()> {
    let address = listener.local_addr()?;
    let url = format!("http://{address}");
    println!("Engram workspace: {url}");
    println!("Local-only server; press Ctrl-C to stop.");
    if launch_browser {
        let result = if cfg!(target_os = "macos") {
            std::process::Command::new("open").arg(&url).spawn()
        } else if cfg!(target_os = "windows") {
            std::process::Command::new("cmd").args(["/C", "start", &url]).spawn()
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
                if let Err(error) = handle(vault, stream) {
                    eprintln!("request failed: {error:#}");
                }
            }
            Err(error) => eprintln!("connection failed: {error}"),
        }
    }
    Ok(())
}

fn handle(vault: &Path, mut stream: TcpStream) -> Result<()> {
    let mut buffer = [0u8; 256 * 1024];
    let size = stream.read(&mut buffer)?;
    let request = String::from_utf8_lossy(&buffer[..size]);
    let mut lines = request.lines();
    let request_line = lines.next().context("missing request line")?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("/");
    let body = request
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap_or("");

    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let response = match (method, path) {
        ("GET", "/") | ("GET", "/vault") => render_index(vault, query)?,
        ("GET", "/threadline") => redirect("/threadline/"),
        ("GET", "/dump") => render_dump(vault)?,
        ("POST", "/api/v1/dump") => submit_dump(vault, body)?,
        ("POST", path) if path.starts_with("/api/v1/dump/") && path.ends_with("/analyze") => analyze_dump(vault, path)?,
        ("GET", "/threadline/") | ("GET", "/threadline/index.html") => {
            response(200, "text/html; charset=utf-8", include_str!("../web/index.html"))
        }
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
        ("GET", "/api/v1/vault/search") => {
            let search = parameter(query, "q").unwrap_or_default();
            if search.trim().is_empty() || search.len() > 512 {
                response(400, "application/json; charset=utf-8", "{\"error\":\"query must contain 1–512 bytes\"}")
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
        ("GET", "/ask") => render_ask()?,
        ("POST", "/ask") => ask(vault, body)?,
        _ => response(404, "text/plain; charset=utf-8", "Not found"),
    };
    stream.write_all(&response)?;
    Ok(())
}

fn render_index(vault: &Path, query: &str) -> Result<Vec<u8>> {
    let search = parameter(query, "q").unwrap_or_default();
    let notes = Vault::open(vault.to_path_buf())?.scan()?;
    let mut body = String::from("<header style=\"display:flex;align-items:center;gap:16px\"><img src=\"/threadline/assets/engram-logo.png\" alt=\"Engram\" width=\"112\" height=\"112\" style=\"object-fit:contain\"><h1>Engram</h1></header><form><input name=\"q\" placeholder=\"Search\" value=\"");
    body.push_str(&escape(&search));
    body.push_str("\"><button>Search</button></form><p><a href=\"/new\">New note</a> · <a href=\"/ask\">Ask the local assistant</a> · <a href=\"/graph\">Open graph</a> · <a href=\"/threadline/\">Open Threadline chat</a></p><ul>");
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
    let rows = items.iter().map(|item| format!("<li><b>{}</b> <small>{} · {} bytes · {}</small> <form method=\"post\" action=\"/api/v1/dump/{}/analyze\" style=\"display:inline\"><button>Analyze locally</button></form></li>", escape(&item.filename), escape(&item.id), item.content.len(), escape(&item.status), encode(&item.id))).collect::<String>();
    let html = format!("<a href=\"/\">← Vault</a><h1>Dump / Inbox</h1><p>Supported now: pasted text, Markdown, and plain text. Items remain pending and are not indexed until approval.</p><textarea id=\"paste\" placeholder=\"Paste text here…\"></textarea><button id=\"submit-paste\">Add pasted text</button><p><input id=\"files\" type=\"file\" multiple accept=\".md,.markdown,.txt,text/plain,text/markdown\"></p><ul>{rows}</ul><script>const send=(filename,source,content)=>fetch('/api/v1/dump',{{method:'POST',headers:{{'content-type':'application/x-www-form-urlencoded'}},body:new URLSearchParams({{filename,source,content}})}}).then(r=>r.ok?r.text():r.text().then(t=>Promise.reject(t))).then(()=>location.reload());document.querySelector('#submit-paste').onclick=()=>send('pasted-text.txt','paste',document.querySelector('#paste').value);document.querySelector('#files').onchange=e=>Promise.all([...e.target.files].map(f=>new Promise((ok,bad)=>{{const r=new FileReader();r.onload=()=>send(f.name,'file',r.result).then(ok,bad);r.onerror=bad;r.readAsText(f)}})));</script>");
    Ok(page("Dump / Inbox", &html))
}

fn submit_dump(vault: &Path, body: &str) -> Result<Vec<u8>> {
    let filename = parameter(body, "filename").unwrap_or_else(|| "pasted-text.txt".into());
    let source = parameter(body, "source").unwrap_or_else(|| "paste".into());
    let content = parameter(body, "content").context("missing dump content")?;
    let item = dump::submit(vault, &filename, &source, &content)?;
    Ok(response(201, "application/json; charset=utf-8", &serde_json::json!({"id": item.id, "status": item.status}).to_string()))
}

fn analyze_dump(vault: &Path, path: &str) -> Result<Vec<u8>> {
    let id = path.strip_prefix("/api/v1/dump/").and_then(|path| path.strip_suffix("/analyze")).unwrap_or("");
    let config_path = vault.join(".engram").join("config.toml");
    let config = Config::load(&config_path).context("local inference configuration is missing")?;
    let client = OpenAiCompat::new(config.llm_base, config.llm_model, config.llm_key, config.provider);
    let analysis = dump::analyze(vault, id, &client)?;
    Ok(response(200, "application/json; charset=utf-8", &serde_json::to_string(&analysis)?))
}

fn render_graph(vault: &Path) -> Result<Vec<u8>> {
    let notes = Vault::open(vault.to_path_buf())?.scan()?;
    let mut body = String::from("<a href=\"/\">← Vault</a><h1>Knowledge graph</h1><svg viewBox=\"0 0 900 520\" style=\"width:100%;border:1px solid #ddd;background:#fafafa\">");
    for (index, note) in notes.iter().enumerate() {
        let x = 100 + (index % 5) * 170;
        let y = 100 + (index / 5) * 150;
        for link in &note.links {
            if let Some(target) = notes.iter().position(|candidate| candidate.id == *link || candidate.path.file_stem().and_then(|s| s.to_str()) == Some(link)) {
                let tx = 100 + (target % 5) * 170;
                let ty = 100 + (target / 5) * 150;
                body.push_str(&format!("<line x1=\"{x}\" y1=\"{y}\" x2=\"{tx}\" y2=\"{ty}\" stroke=\"#999\"/>",));
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
    let path = safe_path(vault, &relative, false)?;
    let note = std::fs::read_to_string(&path)
        .with_context(|| format!("reading note {}", relative.display()))?;
    let notes = Vault::open(vault.to_path_buf())?.scan()?;
    let target = relative.to_string_lossy();
    let stem = relative.file_stem().and_then(|x| x.to_str()).unwrap_or_default();
    let mut backlinks = String::from("<h2>Backlinks</h2><ul>");
    for candidate in notes {
        if candidate.links.iter().any(|link| link == target.as_ref() || link == stem) {
            backlinks.push_str(&format!("<li><a href=\"/note?path={}\">{}</a></li>", encode(&candidate.id), escape(&candidate.title)));
        }
    }
    backlinks.push_str("</ul>");
    let body = format!(
        "<a href=\"/\">← Vault</a><h1>{}</h1><form method=\"post\" action=\"/save\"><input type=\"hidden\" name=\"path\" value=\"{}\"><textarea name=\"body\" autofocus>{}</textarea><button>Save</button></form><h2>Preview</h2><article class=\"preview\">{}</article>{}",
        escape(relative.to_string_lossy().as_ref()),
        encode(relative.to_string_lossy().as_ref()),
        escape(&note),
        markdown_html(&note),
        backlinks
    );
    Ok(page("Edit note", &body))
}

fn save_note(vault: &Path, body: &str) -> Result<Vec<u8>> {
    let relative = parameter(body, "path").context("missing note path")?;
    let relative = validate_relative_path(Path::new(&relative))?;
    let content = parameter(body, "body").context("missing note body")?;
    if let Some(parent) = vault.join(&relative).parent() { std::fs::create_dir_all(parent)?; }
    let path = safe_path(vault, &relative, true)?;
    std::fs::write(&path, content)
        .with_context(|| format!("writing note {}", relative.display()))?;
    let config_path = vault.join(".engram").join("config.toml");
    let config = if config_path.exists() { Config::load(&config_path)? } else { Config::new(vault) };
    let index = Index::open(&config.db)?;
    let note = Vault::open(vault.to_path_buf())?.read_note(&relative.to_string_lossy())?;
    index.upsert(&note, &HashingEmbedder::new(config.embed_dim))?;
    Ok(redirect("/"))
}

fn render_new() -> Result<Vec<u8>> {
    Ok(page("New note", "<a href=\"/\">← Vault</a><h1>New note</h1><form method=\"post\" action=\"/new\"><input name=\"path\" placeholder=\"notes/my-note.md\" required><textarea name=\"body\" placeholder=\"# Title\n\nWrite here…\" autofocus></textarea><button>Create note</button></form>"))
}

fn create_note(vault: &Path, body: &str) -> Result<Vec<u8>> {
    let relative = parameter(body, "path").context("missing note path")?;
    let relative = validate_relative_path(Path::new(&relative))?;
    if relative.extension().and_then(|x| x.to_str()) != Some("md") {
        anyhow::bail!("new notes must use the .md extension")
    }
    let path = safe_path(vault, &relative, true)?;
    if path.exists() { anyhow::bail!("note already exists: {}", relative.display()) }
    std::fs::write(&path, parameter(body, "body").unwrap_or_default())?;
    Ok(redirect(&format!("/note?path={}", encode(relative.to_string_lossy().as_ref()))))
}

fn render_ask() -> Result<Vec<u8>> {
    Ok(page("Ask Engram", "<a href=\"/\">← Vault</a><h1>Ask Engram</h1><form method=\"post\" action=\"/ask\"><textarea name=\"question\" placeholder=\"Ask about your local notes\" autofocus></textarea><button>Ask</button></form><p>Answers use the configured on-device OpenAI-compatible endpoint and cite note ids.</p>"))
}

fn ask(vault: &Path, body: &str) -> Result<Vec<u8>> {
    let question = parameter(body, "question").context("missing question")?;
    let config = Config::load(&vault.join(".engram").join("config.toml"))?;
    let index = Index::open(&config.db)?;
    let hits = index.search(&question, false, 6)?;
    let mut notes = Vec::new();
    for hit in hits {
        notes.push((hit.id.clone(), index.get_note_body(&hit.id)?.unwrap_or_default()));
    }
    let prompt = build_context_prompt(&question, &notes, config.context_window.saturating_mul(4));
    let client = OpenAiCompat::new(config.llm_base, config.llm_model, config.llm_key, config.provider);
    let answer = client.complete(&prompt, &ChatOpts::default())?;
    let history = vault.join(".engram").join("conversations");
    std::fs::create_dir_all(&history)?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
    std::fs::write(history.join(format!("{stamp}.md")), format!("# {question}\n\n## Question\n\n{question}\n\n## Answer\n\n{answer}\n"))?;
    Ok(page("Answer", &format!("<a href=\"/ask\">← Ask again</a><h1>Answer</h1><pre>{}</pre>", escape(&answer))))
}

fn safe_path(vault: &Path, relative: &Path, for_write: bool) -> Result<std::path::PathBuf> {
    let root = vault.canonicalize().context("canonicalizing vault root")?;
    let candidate = root.join(relative);
    let checked = if candidate.exists() {
        candidate.canonicalize().context("canonicalizing vault path")?
    } else if for_write {
        let parent = candidate.parent().context("note has no parent")?.canonicalize()?;
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
        404 => "Not Found",
        _ => "Internal Server Error",
    };
    let header = format!("HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
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
        "/threadline/src/app.js" => ("text/javascript; charset=utf-8", include_str!("../web/src/app.js")),
        "/threadline/src/core/store.js" => ("text/javascript; charset=utf-8", include_str!("../web/src/core/store.js")),
        "/threadline/src/core/memory-manager.js" => ("text/javascript; charset=utf-8", include_str!("../web/src/core/memory-manager.js")),
        "/threadline/src/core/context-packer.js" => ("text/javascript; charset=utf-8", include_str!("../web/src/core/context-packer.js")),
        "/threadline/src/core/app-actions.js" => ("text/javascript; charset=utf-8", include_str!("../web/src/core/app-actions.js")),
        "/threadline/src/core/tool-registry.js" => ("text/javascript; charset=utf-8", include_str!("../web/src/core/tool-registry.js")),
        "/threadline/src/core/permission-gate.js" => ("text/javascript; charset=utf-8", include_str!("../web/src/core/permission-gate.js")),
        "/threadline/src/providers/openai-compatible.js" => ("text/javascript; charset=utf-8", include_str!("../web/src/providers/openai-compatible.js")),
        "/threadline/src/providers/engram-vault.js" => ("text/javascript; charset=utf-8", include_str!("../web/src/providers/engram-vault.js")),
        _ => return None,
    };
    Some(asset)
}

fn threadline_image(path: &str) -> Option<&'static [u8]> {
    match path {
        "/threadline/assets/engram-logo.png" => Some(include_bytes!("../web/assets/engram-logo.png")),
        "/threadline/assets/engram-mark.png" => Some(include_bytes!("../web/assets/engram-mark.png")),
        "/threadline/assets/engram-mark-green.png" => Some(include_bytes!("../web/assets/engram-mark-green.png")),
        "/threadline/assets/engram-icon-purple.png" => Some(include_bytes!("../web/assets/engram-icon-purple.png")),
        "/threadline/assets/engram-icon-pink.png" => Some(include_bytes!("../web/assets/engram-icon-pink.png")),
        "/threadline/assets/engram-icon-blue.png" => Some(include_bytes!("../web/assets/engram-icon-blue.png")),
        "/threadline/assets/engram-icon-fire.png" => Some(include_bytes!("../web/assets/engram-icon-fire.png")),
        "/threadline/assets/engram-icon-forest.png" => Some(include_bytes!("../web/assets/engram-icon-forest.png")),
        "/threadline/assets/engram-icon-ice.png" => Some(include_bytes!("../web/assets/engram-icon-ice.png")),
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
    input.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn markdown_html(markdown: &str) -> String {
    let mut html = String::new();
    let mut in_code = false;
    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            if in_code { html.push_str("</code></pre>"); } else { html.push_str("<pre><code>"); }
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
    if in_code { html.push_str("</code></pre>"); }
    html
}
