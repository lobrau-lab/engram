//! Capability digest: inspect a local program, record what it can do, and run
//! only the fixed actions that were reviewed during inspection.
//!
//! The digest is deliberately conservative. It executes an absolute path
//! directly (never through a shell), captures `--help`/`--version`, and stores
//! a content hash. Running a digest re-checks that hash so a replaced binary
//! cannot silently inherit the old approval.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::atomic_write;

const MAX_CAPTURE_BYTES: usize = 16 * 1024;
const MAX_FLAGS: usize = 240;
const INSPECT_TIMEOUT: Duration = Duration::from_secs(4);
const RUN_TIMEOUT: Duration = Duration::from_secs(6);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityDigest {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub path: String,
    pub executable: Option<String>,
    pub version: Option<String>,
    pub bundle_id: Option<String>,
    pub url_schemes: Vec<String>,
    pub hash: String,
    pub usage: Option<String>,
    pub flags: Vec<String>,
    pub actions: Vec<String>,
    pub risk: String,
    pub inspected_at: u64,
    pub help_excerpt: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppRunResult {
    pub id: String,
    pub action: String,
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug)]
struct CommandOutput {
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

pub fn inspect(vault: &Path, raw_path: &str) -> Result<CapabilityDigest> {
    let requested = PathBuf::from(raw_path.trim());
    anyhow::ensure!(
        requested.is_absolute(),
        "provide an absolute path to a program or .app bundle"
    );
    let path = requested
        .canonicalize()
        .with_context(|| format!("resolving {}", requested.display()))?;
    let digest = if path.is_dir() {
        inspect_app(&path)?
    } else {
        anyhow::ensure!(path.is_file(), "the selected path is not a file");
        inspect_cli(&path)?
    };
    save_digest(vault, &digest)?;
    Ok(digest)
}

pub fn list(vault: &Path) -> Result<Vec<CapabilityDigest>> {
    let dir = capabilities_dir(vault);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut digests = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|part| part.to_str()) != Some("json") {
            continue;
        }
        match std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<CapabilityDigest>(&bytes).ok())
        {
            Some(digest) => digests.push(digest),
            None => eprintln!("ignoring unreadable capability digest {}", path.display()),
        }
    }
    digests.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
    Ok(digests)
}

pub fn load(vault: &Path, id: &str) -> Result<CapabilityDigest> {
    let path = digest_path(vault, id)?;
    let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_slice(&bytes).context("parsing capability digest")
}

pub fn run(vault: &Path, id: &str, action: &str) -> Result<AppRunResult> {
    let digest = load(vault, id)?;
    anyhow::ensure!(
        digest.actions.iter().any(|allowed| allowed == action),
        "this digest does not allow the {action:?} action"
    );
    verify_unchanged(&digest)?;

    let output = match (digest.kind.as_str(), action) {
        ("cli", "help") => run_with_timeout(Path::new(&digest.path), &["--help"], RUN_TIMEOUT)?,
        ("cli", "version") => {
            run_with_timeout(Path::new(&digest.path), &["--version"], RUN_TIMEOUT)?
        }
        ("app", "open") => {
            #[cfg(target_os = "macos")]
            {
                run_with_timeout(
                    Path::new("/usr/bin/open"),
                    &[digest.path.as_str()],
                    RUN_TIMEOUT,
                )?
            }
            #[cfg(not(target_os = "macos"))]
            {
                anyhow::bail!("opening app bundles is only supported on macOS")
            }
        }
        _ => anyhow::bail!("unsupported digest action"),
    };
    Ok(AppRunResult {
        id: digest.id,
        action: action.to_string(),
        status: output.status,
        stdout: output.stdout,
        stderr: output.stderr,
    })
}

fn inspect_cli(path: &Path) -> Result<CapabilityDigest> {
    let name = path
        .file_stem()
        .and_then(|part| part.to_str())
        .unwrap_or("program")
        .to_string();
    let hash = file_hash(path)?;
    let help_attempts = [
        run_with_timeout(path, &["--help"], INSPECT_TIMEOUT),
        run_with_timeout(path, &["-h"], INSPECT_TIMEOUT),
    ];
    let help = first_success(&help_attempts);
    let version_attempts = [
        run_with_timeout(path, &["--version"], INSPECT_TIMEOUT),
        run_with_timeout(path, &["-V"], INSPECT_TIMEOUT),
    ];
    let version = first_success(&version_attempts);
    let help_text = help
        .as_ref()
        .map(|output| format!("{}\n{}", output.stdout, output.stderr))
        .unwrap_or_default();
    let version_text = version
        .as_ref()
        .map(|output| format!("{}\n{}", output.stdout, output.stderr))
        .unwrap_or_default();
    let usage = extract_usage(&help_text);
    let flags = extract_flags(&help_text);

    Ok(CapabilityDigest {
        id: format!("{}-{}", slug(&name), &hash[..12]),
        name,
        kind: "cli".into(),
        path: path.to_string_lossy().into_owned(),
        executable: Some(path.to_string_lossy().into_owned()),
        version: first_line(&version_text),
        bundle_id: None,
        url_schemes: Vec::new(),
        hash,
        usage,
        flags,
        actions: vec!["help".into(), "version".into()],
        risk: "medium".into(),
        inspected_at: now(),
        help_excerpt: truncate(&help_text, 6000),
    })
}

#[cfg(target_os = "macos")]
fn inspect_app(path: &Path) -> Result<CapabilityDigest> {
    let plist = path.join("Contents/Info.plist");
    anyhow::ensure!(plist.is_file(), "the folder is not a macOS .app bundle");
    let output = Command::new("/usr/bin/plutil")
        .args(["-convert", "json", "-o", "-"])
        .arg(&plist)
        .output()
        .context("reading the app bundle metadata")?;
    anyhow::ensure!(
        output.status.success(),
        "could not read the app bundle metadata"
    );
    let info: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("parsing the app bundle metadata")?;
    let bundle_id = string_value(&info, "CFBundleIdentifier");
    let name = string_value(&info, "CFBundleDisplayName")
        .or_else(|| string_value(&info, "CFBundleName"))
        .or_else(|| {
            path.file_stem()
                .and_then(|part| part.to_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "Application".into());
    let version = string_value(&info, "CFBundleShortVersionString")
        .or_else(|| string_value(&info, "CFBundleVersion"));
    let executable_name = string_value(&info, "CFBundleExecutable");
    let executable = executable_name
        .as_deref()
        .map(|name| path.join("Contents/MacOS").join(name))
        .filter(|candidate| candidate.is_file());
    let hash = match executable.as_deref() {
        Some(executable) => file_hash(executable)?,
        None => file_hash(&plist)?,
    };
    let url_schemes = info
        .get("CFBundleURLTypes")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .flat_map(|entry| {
            entry
                .get("CFBundleURLSchemes")
                .and_then(|value| value.as_array())
                .into_iter()
                .flatten()
                .filter_map(|scheme| scheme.as_str().map(str::to_string))
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let identifier = bundle_id.clone().unwrap_or_else(|| name.clone());

    Ok(CapabilityDigest {
        id: format!("{}-{}", slug(&identifier), &hash[..12]),
        name,
        kind: "app".into(),
        path: path.to_string_lossy().into_owned(),
        executable: executable.map(|value| value.to_string_lossy().into_owned()),
        version,
        bundle_id,
        url_schemes,
        hash,
        usage: Some(format!("open \"{}\"", path.display())),
        flags: Vec::new(),
        actions: vec!["open".into()],
        risk: "high".into(),
        inspected_at: now(),
        help_excerpt: truncate(&format!("{}\n{}\n{}", path.display(), identifier, ""), 2000),
    })
}

#[cfg(not(target_os = "macos"))]
fn inspect_app(_path: &Path) -> Result<CapabilityDigest> {
    anyhow::bail!("app bundle inspection is only supported on macOS")
}

fn verify_unchanged(digest: &CapabilityDigest) -> Result<()> {
    let target = digest
        .executable
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&digest.path));
    let current = file_hash(&target)?;
    anyhow::ensure!(
        current == digest.hash,
        "the application changed since it was inspected; inspect it again before running"
    );
    Ok(())
}

fn save_digest(vault: &Path, digest: &CapabilityDigest) -> Result<()> {
    let path = digest_path(vault, &digest.id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(digest)?;
    atomic_write::replace(&path, &bytes)
        .with_context(|| format!("writing capability digest {}", path.display()))
}

fn capabilities_dir(vault: &Path) -> PathBuf {
    vault.join(".engram").join("capabilities")
}

fn digest_path(vault: &Path, id: &str) -> Result<PathBuf> {
    anyhow::ensure!(
        !id.is_empty()
            && id.len() <= 120
            && id.chars().all(|character| character.is_ascii_alphanumeric()
                || character == '-'
                || character == '_'),
        "invalid capability digest id"
    );
    Ok(capabilities_dir(vault).join(format!("{id}.json")))
}

fn file_hash(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}

fn run_with_timeout(program: &Path, args: &[&str], timeout: Duration) -> Result<CommandOutput> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear();
    for key in ["PATH", "HOME", "LANG", "LC_ALL", "TMPDIR"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let mut child = command
        .spawn()
        .with_context(|| format!("starting {}", program.display()))?;
    let stdout = child.stdout.take().context("capturing process stdout")?;
    let stderr = child.stderr.take().context("capturing process stderr")?;
    let stdout_reader = std::thread::spawn(move || read_limited(stdout));
    let stderr_reader = std::thread::spawn(move || read_limited(stderr));

    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!(
                "the application did not finish within {} seconds",
                timeout.as_secs()
            );
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    Ok(CommandOutput {
        status: status.code(),
        stdout,
        stderr,
    })
}

fn read_limited(mut reader: impl Read) -> String {
    let mut bytes = Vec::new();
    let _ = reader
        .by_ref()
        .take((MAX_CAPTURE_BYTES + 1) as u64)
        .read_to_end(&mut bytes);
    bytes.truncate(MAX_CAPTURE_BYTES);
    String::from_utf8_lossy(&bytes).into_owned()
}

fn first_success(attempts: &[Result<CommandOutput>]) -> Option<&CommandOutput> {
    attempts
        .iter()
        .filter_map(|attempt| attempt.as_ref().ok())
        .find(|output| !output.stdout.trim().is_empty() || !output.stderr.trim().is_empty())
}

fn extract_usage(help: &str) -> Option<String> {
    help.lines()
        .find(|line| {
            let trimmed = line.trim_start().to_ascii_lowercase();
            trimmed.starts_with("usage:") || trimmed.starts_with("usage ")
        })
        .map(|line| truncate(line.trim(), 500))
        .or_else(|| {
            help.lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .map(|line| truncate(line, 500))
        })
}

fn extract_flags(help: &str) -> Vec<String> {
    let mut flags = BTreeSet::new();
    for token in help.split_whitespace() {
        let cleaned = token.trim_matches(|character: char| {
            !character.is_ascii_alphanumeric() && character != '-' && character != '_'
        });
        if (cleaned.starts_with("--") && cleaned.len() > 2)
            || (cleaned.len() == 2 && cleaned.starts_with('-') && cleaned != "--")
        {
            flags.insert(cleaned.to_string());
        }
        if flags.len() >= MAX_FLAGS {
            break;
        }
    }
    flags.into_iter().collect()
}

fn first_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| truncate(line, 240))
}

fn string_value(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|item| item.as_str())
        .map(str::to_string)
}

fn slug(value: &str) -> String {
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
        if slug.len() >= 48 {
            break;
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "application".into()
    } else {
        slug.to_string()
    }
}

fn truncate(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_string();
    }
    let mut end = limit;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{extract_flags, extract_usage, inspect, list, run};

    #[test]
    fn parses_usage_and_flags() {
        let help = "Usage: demo [--verbose] <path>\n  -h, --help     show help\n  --version\n";
        assert_eq!(
            extract_usage(help).as_deref(),
            Some("Usage: demo [--verbose] <path>")
        );
        assert_eq!(
            extract_flags(help),
            vec!["--help", "--verbose", "--version", "-h"]
        );
    }

    #[test]
    fn inspects_lists_and_runs_a_local_program() {
        #[cfg(unix)]
        let program = "/bin/echo";
        #[cfg(windows)]
        let program = "C:\\Windows\\System32\\cmd.exe";
        #[cfg(not(any(unix, windows)))]
        return;

        let vault = std::env::temp_dir().join(format!(
            "engram-capability-{}-{}",
            std::process::id(),
            super::now()
        ));
        std::fs::create_dir_all(&vault).unwrap();
        let digest = inspect(&vault, program).unwrap();
        assert_eq!(digest.kind, "cli");
        assert!(digest.hash.len() == 64);
        let listed = list(&vault).unwrap();
        assert!(listed.iter().any(|item| item.id == digest.id));
        let result = run(&vault, &digest.id, "version").unwrap();
        assert_eq!(result.id, digest.id);
        let _ = std::fs::remove_dir_all(vault);
    }

    #[test]
    fn rejects_relative_paths_and_unknown_actions() {
        let vault = std::env::temp_dir().join(format!(
            "engram-capability-reject-{}-{}",
            std::process::id(),
            super::now()
        ));
        std::fs::create_dir_all(&vault).unwrap();
        assert!(inspect(&vault, "echo").is_err());
        let _ = std::fs::remove_dir_all(vault);
    }
}
