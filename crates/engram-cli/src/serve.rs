use anyhow::{Context, Result};
use engram_core::ai::ChatOpts;
use engram_core::index::Index;
use engram_core::vault::validate_relative_path;
use engram_core::{build_context_prompt, Config, HashingEmbedder, LlmClient, OpenAiCompat, Vault};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;

pub fn run(vault: &Path, host: &str, port: u16) -> Result<()> {
    let listener = TcpListener::bind((host, port))
        .with_context(|| format!("binding local workspace at {host}:{port}"))?;
    let address = listener.local_addr()?;
    println!("Engram workspace: http://{address}");
    println!("Local-only server; press Ctrl-C to stop.");
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
        ("GET", "/") => render_index(vault, query)?,
        ("GET", "/note") => render_note(vault, query)?,
        ("POST", "/save") => save_note(vault, body)?,
        ("GET", "/ask") => render_ask()?,
        ("POST", "/ask") => ask(vault, body)?,
        _ => response(404, "text/plain; charset=utf-8", "Not found"),
    };
    stream.write_all(response.as_bytes())?;
    Ok(())
}

fn render_index(vault: &Path, query: &str) -> Result<String> {
    let search = parameter(query, "q").unwrap_or_default();
    let notes = Vault::open(vault.to_path_buf())?.scan()?;
    let mut body = String::from("<h1>Engram</h1><form><input name=\"q\" placeholder=\"Search\" value=\"");
    body.push_str(&escape(&search));
    body.push_str("\"><button>Search</button></form><p><a href=\"/ask\">Ask the local assistant</a></p><ul>");
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

fn render_note(vault: &Path, query: &str) -> Result<String> {
    let relative = parameter(query, "path").context("missing note path")?;
    let relative = validate_relative_path(Path::new(&relative))?;
    let path = vault.join(&relative);
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
        "<a href=\"/\">← Vault</a><h1>{}</h1><form method=\"post\" action=\"/save\"><input type=\"hidden\" name=\"path\" value=\"{}\"><textarea name=\"body\" autofocus>{}</textarea><button>Save</button></form>{}",
        escape(relative.to_string_lossy().as_ref()),
        encode(relative.to_string_lossy().as_ref()),
        escape(&note),
        backlinks
    );
    Ok(page("Edit note", &body))
}

fn save_note(vault: &Path, body: &str) -> Result<String> {
    let relative = parameter(body, "path").context("missing note path")?;
    let relative = validate_relative_path(Path::new(&relative))?;
    let content = parameter(body, "body").context("missing note body")?;
    let path = vault.join(&relative);
    std::fs::write(&path, content)
        .with_context(|| format!("writing note {}", relative.display()))?;
    let config_path = vault.join(".engram").join("config.toml");
    let config = if config_path.exists() { Config::load(&config_path)? } else { Config::new(vault) };
    let index = Index::open(&config.db)?;
    let note = Vault::open(vault.to_path_buf())?.read_note(&relative.to_string_lossy())?;
    index.upsert(&note, &HashingEmbedder::new(config.embed_dim))?;
    Ok(redirect("/"))
}

fn render_ask() -> Result<String> {
    Ok(page("Ask Engram", "<a href=\"/\">← Vault</a><h1>Ask Engram</h1><form method=\"post\" action=\"/ask\"><textarea name=\"question\" placeholder=\"Ask about your local notes\" autofocus></textarea><button>Ask</button></form><p>Answers use the configured on-device OpenAI-compatible endpoint and cite note ids.</p>"))
}

fn ask(vault: &Path, body: &str) -> Result<String> {
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
    Ok(page("Answer", &format!("<a href=\"/ask\">← Ask again</a><h1>Answer</h1><pre>{}</pre>", escape(&answer))))
}

fn page(title: &str, body: &str) -> String {
    response(200, "text/html; charset=utf-8", &format!(
        "<!doctype html><meta charset=utf-8><title>{}</title><style>body{{font:16px system-ui;max-width:900px;margin:40px auto;padding:0 20px}}textarea{{width:100%;height:70vh;font:15px monospace}}a{{color:#36c}}</style>{}",
        escape(title), body
    ))
}

fn response(status: u16, content_type: &str, body: &str) -> String {
    let reason = if status == 200 { "OK" } else { "Not Found" };
    format!("HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
}

fn redirect(location: &str) -> String {
    format!("HTTP/1.1 303 See Other\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
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
