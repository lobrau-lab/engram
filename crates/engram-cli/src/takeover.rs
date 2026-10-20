//! Consent-based macOS GUI takeover.
//!
//! A takeover session is armed explicitly in the Threadline UI for a bounded
//! number of minutes. While it is armed, the local model may inspect the
//! accessibility tree, focus applications, press keys, type, click, scroll,
//! drag, run AppleScript, and capture the screen without a prompt for every
//! step. Stopping the session is always immediate, and every action is written
//! to a local audit log.
//!
//! The implementation is a separate macOS helper compiled by `build.rs`. The
//! helper only acts when invoked with a command; it never listens in the
//! background. On other platforms every operation reports unavailable.

use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const HELPER_BYTES: &[u8] = include_bytes!(env!("ENGRAM_CONTROL_BIN"));
const AUDIT_LIMIT: usize = 80;
const MAX_TEXT: usize = 16_000;
const MAX_APPLESCRIPT: usize = 20_000;
const MAX_APP: usize = 500;
const MAX_LABEL: usize = 300;
const MAX_KEY: usize = 80;
const APPLESCRIPT_TIMEOUT: Duration = Duration::from_secs(20);

pub struct Takeover {
    vault: PathBuf,
    inner: Mutex<State>,
}

struct State {
    session: Option<Session>,
    audit: Vec<AuditEntry>,
    epoch: u64,
}

struct Session {
    token: String,
    minutes: u64,
    started_at: u64,
    expires_at: u64,
}

#[derive(Clone, Serialize, Deserialize)]
struct AuditEntry {
    at: u64,
    action: String,
    summary: String,
    ok: bool,
}

#[derive(Debug, Deserialize)]
struct ActionRequest {
    action: String,
    #[serde(default)]
    token: String,
    #[serde(default)]
    app: Option<String>,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    value: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    button: Option<String>,
    #[serde(default)]
    x: Option<i32>,
    #[serde(default)]
    y: Option<i32>,
    #[serde(default)]
    dx: Option<i32>,
    #[serde(default)]
    dy: Option<i32>,
    #[serde(default)]
    x1: Option<i32>,
    #[serde(default)]
    y1: Option<i32>,
    #[serde(default)]
    x2: Option<i32>,
    #[serde(default)]
    y2: Option<i32>,
    #[serde(default)]
    depth: Option<u32>,
}

#[derive(Debug)]
pub struct Failure {
    pub status: u16,
    pub message: String,
}

impl Failure {
    fn new(status: u16, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    fn bad_request(message: impl Into<String>) -> Self {
        Self::new(400, message)
    }

    fn forbidden(message: impl Into<String>) -> Self {
        Self::new(403, message)
    }

    fn unsupported(message: impl Into<String>) -> Self {
        Self::new(501, message)
    }

    fn internal(message: impl Into<String>) -> Self {
        Self::new(500, message)
    }
}

impl From<std::io::Error> for Failure {
    fn from(error: std::io::Error) -> Self {
        Self::internal(error.to_string())
    }
}

impl Takeover {
    pub fn new(vault: PathBuf) -> Self {
        let audit = load_audit(&vault);
        Self {
            vault,
            inner: Mutex::new(State {
                session: None,
                audit,
                epoch: 0,
            }),
        }
    }

    /// Read-only status: helper availability, accessibility permission,
    /// frontmost application, screens, running apps, session, and audit.
    pub fn status(&self) -> Value {
        let supported = cfg!(target_os = "macos");
        let (session, audit) = self.snapshot();
        let mut helper_available = false;
        let mut helper_error = None;
        let mut info = Value::Null;
        let mut apps = Value::Array(Vec::new());

        if supported {
            match self.run_helper(&["status"]) {
                Ok(value) => {
                    helper_available = true;
                    info = value;
                }
                Err(error) => helper_error = Some(error.message),
            }
            if helper_available {
                match self.run_helper(&["apps"]) {
                    Ok(value) => {
                        apps = value.get("apps").cloned().unwrap_or_else(|| json!([]));
                    }
                    Err(error) => helper_error = Some(error.message),
                }
            }
        } else {
            helper_error = Some("GUI takeover is only available on macOS.".to_string());
        }

        json!({
            "supported": supported,
            "platform": std::env::consts::OS,
            "helper": { "available": helper_available, "error": helper_error },
            "accessibility": info.get("accessibility").and_then(Value::as_bool).unwrap_or(false),
            "frontmost": info.get("frontmost").and_then(Value::as_str).unwrap_or(""),
            "frontmostBundleId": info.get("frontmostBundleId").and_then(Value::as_str).unwrap_or(""),
            "screens": info.get("screens").cloned().unwrap_or_else(|| json!([])),
            "apps": apps,
            "session": session,
            "audit": audit,
        })
    }

    /// Arm a timed, trusted session. The returned token is held only by the
    /// browser tab that armed it; it is never stored in the vault.
    pub fn arm(&self, minutes: u64) -> Result<Value, Failure> {
        if !cfg!(target_os = "macos") {
            return Err(Failure::unsupported(
                "GUI takeover is only available on macOS.",
            ));
        }
        if !(1..=120).contains(&minutes) {
            return Err(Failure::bad_request(
                "arm the takeover session for between 1 and 120 minutes",
            ));
        }
        let now = now();
        let token = random_hex(24);
        let expires_at = now + minutes * 60;
        {
            let mut state = self.lock();
            state.epoch = state.epoch.wrapping_add(1);
            state.session = Some(Session {
                token: token.clone(),
                minutes,
                started_at: now,
                expires_at,
            });
        }
        self.record(
            "session_armed",
            &format!("{minutes} minute trusted takeover session"),
            true,
        );
        Ok(json!({
            "ok": true,
            "token": token,
            "minutes": minutes,
            "expiresAt": expires_at,
            "remainingSeconds": minutes * 60,
        }))
    }

    /// Stop the session immediately. This intentionally does not require the
    /// token: a stop control must always work.
    pub fn disarm(&self) -> Value {
        let had_session = {
            let mut state = self.lock();
            state.epoch = state.epoch.wrapping_add(1);
            state.session.take().is_some()
        };
        if had_session {
            self.record("session_disarmed", "manual stop", true);
        }
        json!({ "ok": true, "armed": false })
    }

    /// Open the macOS Accessibility pane and ask the helper to show the
    /// system trust prompt. The user still decides whether to grant access.
    pub fn request_permission(&self) -> Result<Value, Failure> {
        let result = self.run_helper(&["request-trust"])?;
        let settings_opened = Command::new("/usr/bin/open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        Ok(json!({
            "ok": true,
            "accessibility": result.get("accessibility").and_then(Value::as_bool).unwrap_or(false),
            "settingsOpened": settings_opened,
        }))
    }

    /// Run one action inside an armed session.
    pub fn action(&self, body: &str) -> Result<Value, Failure> {
        let request: ActionRequest = serde_json::from_str(body)
            .map_err(|error| Failure::bad_request(format!("invalid takeover request: {error}")))?;
        if request.action.len() > 40
            || !request
                .action
                .chars()
                .all(|character| character.is_ascii_lowercase() || character == '_')
        {
            return Err(Failure::bad_request("invalid takeover action name"));
        }
        let epoch = self.ensure_session(&request.token)?;
        let summary = summarize(&request);
        let outcome = self.dispatch(&request, epoch);
        self.record(&request.action, &summary, outcome.is_ok());
        outcome.map(|result| {
            json!({
                "ok": true,
                "action": request.action,
                "result": result,
            })
        })
    }

    /// Read a screenshot previously created by this module. Only safe file
    /// names under `.engram/takeover/screenshots` are served.
    pub fn screenshot_bytes(&self, name: &str) -> Result<Vec<u8>, Failure> {
        if name.is_empty()
            || name.len() > 96
            || !name.ends_with(".png")
            || !name
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || ".-_".contains(character))
        {
            return Err(Failure::new(404, "screenshot not found"));
        }
        let path = self
            .vault
            .join(".engram")
            .join("takeover")
            .join("screenshots")
            .join(name);
        fs::read(path).map_err(|_| Failure::new(404, "screenshot not found"))
    }

    fn dispatch(&self, request: &ActionRequest, epoch: u64) -> Result<Value, Failure> {
        match request.action.as_str() {
            "focus" => {
                let app = app_arg(&request.app)?;
                self.run_helper_in_session(&["focus", &app], epoch)
            }
            "windows" => {
                let app = app_arg(&request.app)?;
                self.run_helper_in_session(&["windows", &app], epoch)
            }
            "tree" => {
                let app = app_arg(&request.app)?;
                let depth = request.depth.unwrap_or(6).clamp(1, 12).to_string();
                self.run_helper_in_session(&["tree", &app, &depth], epoch)
            }
            "key" => {
                let key = key_arg(&request.key)?;
                self.run_helper_in_session(&["key", &key], epoch)
            }
            "type" => {
                let text = text_arg(&request.text)?;
                self.run_helper_in_session(&["type", &text], epoch)
            }
            "click" => {
                let x = coordinate(request.x, "x")?.to_string();
                let y = coordinate(request.y, "y")?.to_string();
                let button = button_arg(&request.button)?;
                self.run_helper_in_session(&["click", &x, &y, &button], epoch)
            }
            "move" => {
                let x = coordinate(request.x, "x")?.to_string();
                let y = coordinate(request.y, "y")?.to_string();
                self.run_helper_in_session(&["move", &x, &y], epoch)
            }
            "scroll" => {
                let dy = request
                    .dy
                    .ok_or_else(|| Failure::bad_request("dy is required"))?
                    .clamp(-200, 200)
                    .to_string();
                let dx = request.dx.unwrap_or(0).clamp(-200, 200).to_string();
                self.run_helper_in_session(&["scroll", &dy, &dx], epoch)
            }
            "drag" => {
                let x1 = coordinate(request.x1, "x1")?.to_string();
                let y1 = coordinate(request.y1, "y1")?.to_string();
                let x2 = coordinate(request.x2, "x2")?.to_string();
                let y2 = coordinate(request.y2, "y2")?.to_string();
                self.run_helper_in_session(&["drag", &x1, &y1, &x2, &y2], epoch)
            }
            "click_ui" => {
                let app = app_arg(&request.app)?;
                let role = label_arg(&request.role)?;
                let title = label_arg(&request.title)?;
                if role.is_empty() && title.is_empty() {
                    return Err(Failure::bad_request("provide a UI role, a title, or both"));
                }
                self.run_helper_in_session(&["click-ui", &app, &role, &title], epoch)
            }
            "set_value" => {
                let app = app_arg(&request.app)?;
                let role = label_arg(&request.role)?;
                let title = label_arg(&request.title)?;
                let value = value_arg(&request.value)?;
                if role.is_empty() && title.is_empty() {
                    return Err(Failure::bad_request("provide a UI role, a title, or both"));
                }
                self.run_helper_in_session(&["set-value", &app, &role, &title, &value], epoch)
            }
            "open" => self.open_app(&app_arg(&request.app)?, epoch),
            "applescript" => self.applescript(&script_arg(&request.source)?, epoch),
            "screenshot" => self.screenshot(epoch),
            _ => Err(Failure::bad_request(format!(
                "unsupported takeover action: {}",
                request.action
            ))),
        }
    }

    fn ensure_session(&self, token: &str) -> Result<u64, Failure> {
        let now = now();
        let mut state = self.lock();
        let expired = state
            .session
            .as_ref()
            .is_some_and(|session| session.expires_at <= now);
        if expired {
            state.session = None;
            state.epoch = state.epoch.wrapping_add(1);
            drop(state);
            self.record("session_expired", "session deadline reached", true);
            return Err(Failure::forbidden(
                "The takeover session expired. Arm a new one to continue.",
            ));
        }
        let Some(session) = state.session.as_ref() else {
            return Err(Failure::forbidden(
                "Arm a takeover session in the Takeover view first.",
            ));
        };
        if token.is_empty() || session.token != token {
            return Err(Failure::forbidden(
                "The takeover session token is invalid. Stop and re-arm the session.",
            ));
        }
        Ok(state.epoch)
    }

    fn session_active(&self, epoch: u64) -> bool {
        let now = now();
        let expired = {
            let mut state = self.lock();
            if state.epoch != epoch {
                return false;
            }
            match state.session.as_ref() {
                Some(session) if session.expires_at > now => return true,
                Some(_) => {
                    state.session = None;
                    state.epoch = state.epoch.wrapping_add(1);
                    true
                }
                None => return false,
            }
        };
        if expired {
            self.record("session_expired", "session deadline reached", true);
        }
        false
    }

    fn snapshot(&self) -> (Value, Vec<AuditEntry>) {
        let now = now();
        let expired = {
            let mut state = self.lock();
            let expired = state
                .session
                .as_ref()
                .is_some_and(|session| session.expires_at <= now);
            if expired {
                state.session = None;
                state.epoch = state.epoch.wrapping_add(1);
            }
            expired
        };
        if expired {
            self.record("session_expired", "session deadline reached", true);
        }
        let state = self.lock();
        let session = state
            .session
            .as_ref()
            .map(|session| {
                json!({
                    "armed": true,
                    "startedAt": session.started_at,
                    "expiresAt": session.expires_at,
                    "remainingSeconds": session.expires_at.saturating_sub(now),
                    "minutes": session.minutes,
                })
            })
            .unwrap_or(Value::Null);
        (session, state.audit.clone())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn record(&self, action: &str, summary: &str, ok: bool) {
        let entry = AuditEntry {
            at: now(),
            action: action.to_string(),
            summary: clip(summary, 320),
            ok,
        };
        {
            let mut state = self.lock();
            state.audit.push(entry.clone());
            if state.audit.len() > AUDIT_LIMIT {
                state.audit.remove(0);
            }
        }
        let dir = self.vault.join(".engram").join("takeover");
        if fs::create_dir_all(&dir).is_err() {
            return;
        }
        restrict_dir(&dir);
        let audit_path = dir.join("audit.jsonl");
        let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&audit_path)
        else {
            return;
        };
        restrict_file(&audit_path);
        if let Ok(line) = serde_json::to_string(&entry) {
            let _ = writeln!(file, "{line}");
        }
    }

    fn run_helper(&self, args: &[&str]) -> Result<Value, Failure> {
        let helper = self.ensure_helper()?;
        let mut command = Command::new(&helper);
        command.args(args);
        let output = self.run_command(&mut command, None)?;
        Self::parse_helper_output(output)
    }

    fn run_helper_in_session(&self, args: &[&str], epoch: u64) -> Result<Value, Failure> {
        let helper = self.ensure_helper()?;
        let mut command = Command::new(&helper);
        command.args(args);
        let output = self.run_command(&mut command, Some(epoch))?;
        Self::parse_helper_output(output)
    }

    fn parse_helper_output(output: Output) -> Result<Value, Failure> {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let value: Value = serde_json::from_str(stdout.trim()).map_err(|_| {
            let detail = if stderr.trim().is_empty() {
                stdout.trim()
            } else {
                stderr.trim()
            };
            Failure::internal(format!(
                "the takeover helper returned no JSON: {}",
                clip(detail, 400)
            ))
        })?;
        if let Some(message) = value.get("error").and_then(Value::as_str) {
            let status = if message.contains("Accessibility permission") {
                403
            } else {
                500
            };
            return Err(Failure::new(status, clip(message, 600)));
        }
        if !output.status.success() {
            return Err(Failure::internal(format!(
                "the takeover helper failed: {}",
                clip(stderr.trim(), 400)
            )));
        }
        Ok(value)
    }

    fn run_command(&self, command: &mut Command, epoch: Option<u64>) -> Result<Output, Failure> {
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|error| {
            Failure::internal(format!("could not start takeover action: {error}"))
        })?;
        let mut stdout = child.stdout.take().expect("piped stdout");
        let stdout_reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            stdout.read_to_end(&mut bytes).map(|_| bytes)
        });
        let mut stderr = child.stderr.take().expect("piped stderr");
        let stderr_reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            stderr.read_to_end(&mut bytes).map(|_| bytes)
        });

        let status = loop {
            if epoch.is_some_and(|epoch| !self.session_active(epoch)) {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(Failure::forbidden(
                    "Takeover was stopped while this action was running.",
                ));
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = stdout_reader.join();
                    let _ = stderr_reader.join();
                    return Err(Failure::internal(format!(
                        "could not wait for takeover action: {error}"
                    )));
                }
            }
        };
        let stdout = stdout_reader
            .join()
            .map_err(|_| Failure::internal("could not collect takeover action output"))?
            .map_err(Failure::from)?;
        let stderr = stderr_reader
            .join()
            .map_err(|_| Failure::internal("could not collect takeover action output"))?
            .map_err(Failure::from)?;
        Ok(Output {
            status,
            stdout,
            stderr,
        })
    }

    fn ensure_helper(&self) -> Result<PathBuf, Failure> {
        if !cfg!(target_os = "macos") {
            return Err(Failure::unsupported(
                "GUI takeover is only available on macOS.",
            ));
        }
        let dir = helper_dir()?;
        fs::create_dir_all(&dir)?;
        restrict_dir(&dir);
        let path = dir.join("engram-control");
        let hash = hex(&Sha256::digest(HELPER_BYTES));
        let stamp_path = dir.join("engram-control.sha256");
        let installed = fs::read_to_string(&stamp_path)
            .map(|stamp| stamp.trim() == hash)
            .unwrap_or(false);
        if !installed || !path.is_file() {
            let temporary = dir.join(format!(
                ".engram-control.{}.{}.tmp",
                std::process::id(),
                random_hex(4)
            ));
            fs::write(&temporary, HELPER_BYTES)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&temporary, fs::Permissions::from_mode(0o755))?;
            }
            fs::rename(&temporary, &path)?;
            let _ = fs::write(&stamp_path, format!("{hash}\n"));
        }
        Ok(path)
    }

    fn screenshot(&self, epoch: u64) -> Result<Value, Failure> {
        let dir = self
            .vault
            .join(".engram")
            .join("takeover")
            .join("screenshots");
        fs::create_dir_all(&dir)?;
        restrict_dir(&dir);
        let name = format!("{}-{}.png", now(), random_hex(4));
        let path = dir.join(&name);
        let mut command = Command::new("/usr/sbin/screencapture");
        command.args(["-x", "-t", "png"]).arg(&path);
        let output = self.run_command(&mut command, Some(epoch))?;
        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr);
            return Err(Failure::internal(format!(
                "screen capture failed. Grant Screen Recording permission to Engram, then retry. {}",
                clip(detail.trim(), 300)
            )));
        }
        restrict_file(&path);
        Ok(json!({
            "file": name,
            "url": format!("/api/v1/takeover/screenshot/{name}"),
        }))
    }

    fn open_app(&self, app: &str, epoch: u64) -> Result<Value, Failure> {
        if app.starts_with('-') {
            return Err(Failure::bad_request(
                "application name cannot start with '-'",
            ));
        }
        let mut command = Command::new("/usr/bin/open");
        if app.contains('/') {
            if !app.to_lowercase().ends_with(".app") {
                return Err(Failure::bad_request(
                    "provide an application name or a path to a .app bundle",
                ));
            }
            command.arg(app);
        } else {
            command.arg("-a").arg(app);
        }
        let output = self.run_command(&mut command, Some(epoch))?;
        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr);
            return Err(Failure::internal(format!(
                "could not open {app}: {}",
                clip(detail.trim(), 300)
            )));
        }
        Ok(json!({ "opened": app }))
    }

    fn applescript(&self, source: &str, epoch: u64) -> Result<Value, Failure> {
        let dir = self.vault.join(".engram").join("takeover").join("tmp");
        fs::create_dir_all(&dir)?;
        restrict_dir(&dir);
        let id = random_hex(8);
        let stdout_path = dir.join(format!("{id}.out"));
        let stderr_path = dir.join(format!("{id}.err"));
        let stdout = File::create(&stdout_path)?;
        let stderr = File::create(&stderr_path)?;
        restrict_file(&stdout_path);
        restrict_file(&stderr_path);
        let mut child = Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg(source)
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn()
            .map_err(|error| Failure::internal(format!("could not run osascript: {error}")))?;
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => {
                    if !self.session_active(epoch) {
                        let _ = child.kill();
                        let _ = child.wait();
                        let _ = fs::remove_file(&stdout_path);
                        let _ = fs::remove_file(&stderr_path);
                        return Err(Failure::forbidden(
                            "Takeover was stopped while AppleScript was running.",
                        ));
                    }
                    if started.elapsed() >= APPLESCRIPT_TIMEOUT {
                        let _ = child.kill();
                        let _ = child.wait();
                        break None;
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = fs::remove_file(&stdout_path);
                    let _ = fs::remove_file(&stderr_path);
                    return Err(Failure::internal(format!(
                        "could not wait for osascript: {error}"
                    )));
                }
            }
        };
        let stdout = fs::read_to_string(&stdout_path).unwrap_or_default();
        let stderr = fs::read_to_string(&stderr_path).unwrap_or_default();
        let _ = fs::remove_file(&stdout_path);
        let _ = fs::remove_file(&stderr_path);
        match status {
            Some(status) if status.success() => Ok(json!({
                "stdout": clip(stdout.trim_end(), MAX_TEXT),
                "stderr": clip(stderr.trim_end(), MAX_TEXT),
            })),
            Some(status) => Err(Failure::internal(format!(
                "AppleScript exited with {status}: {}",
                clip(stderr.trim(), 600)
            ))),
            None => Err(Failure::internal(
                "AppleScript timed out after 20 seconds and was stopped.",
            )),
        }
    }
}

fn load_audit(vault: &Path) -> Vec<AuditEntry> {
    let path = vault.join(".engram").join("takeover").join("audit.jsonl");
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut entries: Vec<AuditEntry> = text
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    if entries.len() > AUDIT_LIMIT {
        entries.drain(0..entries.len() - AUDIT_LIMIT);
    }
    entries
}

fn helper_dir() -> Result<PathBuf, Failure> {
    if let Some(home) = std::env::var_os("HOME") {
        return Ok(PathBuf::from(home)
            .join("Library")
            .join("Caches")
            .join("Engram")
            .join("bin"));
    }
    Ok(std::env::temp_dir().join("engram-bin"))
}

fn app_arg(value: &Option<String>) -> Result<String, Failure> {
    required_clean(value, "app", MAX_APP)
}

fn key_arg(value: &Option<String>) -> Result<String, Failure> {
    required_clean(value, "key", MAX_KEY)
}

fn label_arg(value: &Option<String>) -> Result<String, Failure> {
    optional_clean(value, MAX_LABEL)
}

fn text_arg(value: &Option<String>) -> Result<String, Failure> {
    let text = value.as_deref().unwrap_or("");
    if text.is_empty() {
        return Err(Failure::bad_request("text is required"));
    }
    if text.len() > MAX_TEXT {
        return Err(Failure::bad_request(format!(
            "text must be at most {MAX_TEXT} bytes"
        )));
    }
    if text
        .chars()
        .any(|character| character.is_control() && character != '\n' && character != '\t')
    {
        return Err(Failure::bad_request(
            "text contains unsupported control characters",
        ));
    }
    Ok(text.to_string())
}

fn value_arg(value: &Option<String>) -> Result<String, Failure> {
    let value = value.as_deref().unwrap_or("");
    if value.len() > MAX_TEXT {
        return Err(Failure::bad_request(format!(
            "value must be at most {MAX_TEXT} bytes"
        )));
    }
    if value
        .chars()
        .any(|character| character.is_control() && character != '\n' && character != '\t')
    {
        return Err(Failure::bad_request(
            "value contains unsupported control characters",
        ));
    }
    Ok(value.to_string())
}

fn script_arg(value: &Option<String>) -> Result<String, Failure> {
    let source = value.as_deref().unwrap_or("");
    if source.trim().is_empty() {
        return Err(Failure::bad_request("AppleScript source is required"));
    }
    if source.len() > MAX_APPLESCRIPT {
        return Err(Failure::bad_request(format!(
            "AppleScript must be at most {MAX_APPLESCRIPT} bytes"
        )));
    }
    Ok(source.to_string())
}

fn required_clean(value: &Option<String>, name: &str, max: usize) -> Result<String, Failure> {
    let value = optional_clean(value, max)?;
    if value.is_empty() {
        return Err(Failure::bad_request(format!("{name} is required")));
    }
    Ok(value)
}

fn optional_clean(value: &Option<String>, max: usize) -> Result<String, Failure> {
    let value = value.as_deref().unwrap_or("").trim();
    if value.len() > max {
        return Err(Failure::bad_request(format!(
            "value must be at most {max} bytes"
        )));
    }
    if value.chars().any(char::is_control) {
        return Err(Failure::bad_request("value contains control characters"));
    }
    Ok(value.to_string())
}

fn button_arg(value: &Option<String>) -> Result<String, Failure> {
    match value
        .as_deref()
        .unwrap_or("left")
        .trim()
        .to_lowercase()
        .as_str()
    {
        "left" => Ok("left".to_string()),
        "right" => Ok("right".to_string()),
        "middle" | "center" => Ok("middle".to_string()),
        _ => Err(Failure::bad_request(
            "button must be left, right, or middle",
        )),
    }
}

fn coordinate(value: Option<i32>, name: &str) -> Result<i32, Failure> {
    let value = value.ok_or_else(|| Failure::bad_request(format!("{name} is required")))?;
    if !(-100_000..=100_000).contains(&value) {
        return Err(Failure::bad_request(format!(
            "{name} is outside the supported screen range"
        )));
    }
    Ok(value)
}

fn summarize(request: &ActionRequest) -> String {
    let app = request.app.as_deref().unwrap_or("").trim();
    match request.action.as_str() {
        "focus" | "open" | "windows" | "tree" => app.to_string(),
        "key" => request.key.as_deref().unwrap_or("").trim().to_string(),
        "type" => format!(
            "{} characters",
            request.text.as_deref().unwrap_or("").chars().count()
        ),
        "click" | "move" => format!(
            "x={}, y={}",
            request.x.map(|value| value.to_string()).unwrap_or_default(),
            request.y.map(|value| value.to_string()).unwrap_or_default()
        ),
        "scroll" => format!(
            "dy={}, dx={}",
            request.dy.unwrap_or(0),
            request.dx.unwrap_or(0)
        ),
        "drag" => format!(
            "({}, {}) → ({}, {})",
            request.x1.unwrap_or(0),
            request.y1.unwrap_or(0),
            request.x2.unwrap_or(0),
            request.y2.unwrap_or(0)
        ),
        "click_ui" => format!(
            "{} · {} · {}",
            app,
            request.role.as_deref().unwrap_or("").trim(),
            request.title.as_deref().unwrap_or("").trim()
        ),
        "set_value" => format!(
            "{} · {} · {} · {} characters",
            app,
            request.role.as_deref().unwrap_or("").trim(),
            request.title.as_deref().unwrap_or("").trim(),
            request.value.as_deref().unwrap_or("").chars().count()
        ),
        "applescript" => "AppleScript action".to_string(),
        "screenshot" => "full screen".to_string(),
        _ => String::new(),
    }
}

fn clip(value: &str, max: usize) -> String {
    let mut clipped = String::new();
    for (index, character) in value.chars().enumerate() {
        if index >= max {
            clipped.push('…');
            break;
        }
        clipped.push(character);
    }
    clipped
}

#[cfg(unix)]
fn restrict_dir(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
}

#[cfg(not(unix))]
fn restrict_dir(_path: &Path) {}

#[cfg(unix)]
fn restrict_file(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_file(_path: &Path) {}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn random_hex(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buffer);
    hex(&buffer)
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn test_vault(label: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "engram-takeover-{label}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create test vault");
        dir
    }

    #[test]
    fn session_lifecycle_requires_the_right_token() {
        let vault = test_vault("session");
        let takeover = Takeover::new(vault.clone());
        let armed = takeover.arm(15).expect("arm session");
        let token = armed["token"].as_str().expect("token").to_string();
        assert!(takeover.ensure_session(&token).is_ok());
        assert_eq!(
            takeover.ensure_session("not-the-token").unwrap_err().status,
            403
        );
        takeover.disarm();
        assert_eq!(takeover.ensure_session(&token).unwrap_err().status, 403);
        let audit = fs::read_to_string(vault.join(".engram/takeover/audit.jsonl"))
            .expect("durable takeover audit");
        assert!(audit.contains("session_armed"));
        assert!(audit.contains("session_disarmed"));
        let _ = fs::remove_dir_all(vault);
    }

    #[test]
    fn open_requires_an_app_bundle_for_paths() {
        let vault = test_vault("open-path");
        let takeover = Takeover::new(vault.clone());
        let error = takeover
            .open_app("/tmp/not-an-app", 1)
            .expect_err("path without .app must be rejected");
        assert_eq!(error.status, 400);
        let _ = fs::remove_dir_all(vault);
    }

    #[test]
    fn expired_sessions_are_cleared_and_audited() {
        let vault = test_vault("expired");
        let takeover = Takeover::new(vault.clone());
        let armed = takeover.arm(1).expect("arm session");
        let token = armed["token"].as_str().expect("token").to_string();
        {
            let mut state = takeover.lock();
            if let Some(session) = state.session.as_mut() {
                session.expires_at = 0;
            }
        }
        assert_eq!(takeover.ensure_session(&token).unwrap_err().status, 403);
        let audit = fs::read_to_string(vault.join(".engram/takeover/audit.jsonl"))
            .expect("durable takeover audit");
        assert!(audit.contains("session_expired"));
        let _ = fs::remove_dir_all(vault);
    }

    #[test]
    fn session_minutes_are_bounded() {
        let vault = test_vault("minutes");
        let takeover = Takeover::new(vault.clone());
        assert_eq!(takeover.arm(0).unwrap_err().status, 400);
        assert_eq!(takeover.arm(121).unwrap_err().status, 400);
        assert!(takeover.arm(1).is_ok());
        let _ = fs::remove_dir_all(vault);
    }

    #[test]
    fn actions_require_an_armed_session() {
        let vault = test_vault("unarmed");
        let takeover = Takeover::new(vault.clone());
        let error = takeover
            .action(r#"{"token":"","action":"focus","app":"Safari"}"#)
            .unwrap_err();
        assert_eq!(error.status, 403);
        let _ = fs::remove_dir_all(vault);
    }

    #[test]
    fn unknown_actions_are_rejected_before_dispatch() {
        let vault = test_vault("unknown");
        let takeover = Takeover::new(vault.clone());
        let token = takeover.arm(15).expect("arm")["token"]
            .as_str()
            .expect("token")
            .to_string();
        let error = takeover
            .action(&format!(
                r#"{{"token":"{token}","action":"rm_rf_everything"}}"#
            ))
            .unwrap_err();
        assert_eq!(error.status, 400);
        let _ = fs::remove_dir_all(vault);
    }

    #[test]
    fn summaries_do_not_store_typed_text() {
        let request: ActionRequest =
            serde_json::from_str(r#"{"action":"type","text":"super secret"}"#).expect("request");
        let summary = summarize(&request);
        assert_eq!(summary, "12 characters");
        assert!(!summary.contains("secret"));
    }

    #[test]
    fn summaries_do_not_store_applescript_source() {
        let request: ActionRequest = serde_json::from_str(
            r#"{"action":"applescript","source":"do shell script \"echo private-token\""}"#,
        )
        .expect("request");

        let summary = summarize(&request);
        assert_eq!(summary, "AppleScript action");
        assert!(!summary.contains("private-token"));
    }

    #[test]
    fn cancellable_helper_fixture() {
        if std::env::var_os("ENGRAM_CANCELLABLE_HELPER_CHILD").is_some() {
            std::thread::sleep(Duration::from_secs(30));
        }
    }

    #[test]
    fn disarming_stops_an_in_flight_helper_process() {
        let vault = test_vault("cancel-action");
        let takeover = std::sync::Arc::new(Takeover::new(vault.clone()));
        let armed = takeover.arm(15).expect("arm session");
        let token = armed["token"].as_str().expect("token");
        let epoch = takeover.ensure_session(token).expect("session epoch");

        let test_binary = std::env::current_exe().expect("current test binary");
        let mut command = std::process::Command::new(test_binary);
        command
            .args([
                "--exact",
                "takeover::tests::cancellable_helper_fixture",
                "--nocapture",
            ])
            .env("ENGRAM_CANCELLABLE_HELPER_CHILD", "1");

        let stopper = std::sync::Arc::clone(&takeover);
        let stop_thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            stopper.disarm();
        });
        let started = Instant::now();
        let error = takeover
            .run_command(&mut command, Some(epoch))
            .expect_err("disarm should cancel the child process");
        stop_thread.join().expect("stop thread");

        assert_eq!(error.status, 403);
        assert!(started.elapsed() < Duration::from_secs(3));
        let _ = fs::remove_dir_all(vault);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_build_embeds_a_runnable_control_helper() {
        let vault = test_vault("embedded-helper");
        let helper = vault.join("engram-control");
        fs::write(&helper, HELPER_BYTES).expect("write embedded helper");
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o700))
            .expect("make helper executable");

        let output = Command::new(&helper)
            .arg("status")
            .output()
            .expect("run embedded helper");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let status: Value = serde_json::from_slice(&output.stdout).expect("helper JSON");
        assert!(status.get("accessibility").is_some());

        let _ = fs::remove_dir_all(vault);
    }
}
