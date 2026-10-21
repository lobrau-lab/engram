use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use engram_core::ai::ChatOpts;
use engram_core::index::sanitize_fts;
use engram_core::{
    build_context_prompt, cosine, Config, Embedder, FolderSync, HashingEmbedder, Index, LlmClient,
    OpenAiCompat, SyncBackend, Vault,
};

mod serve;
mod takeover;

#[derive(Parser, Debug)]
#[command(
    name = "engram",
    version,
    about = "Local-first, AI-native second brain"
)]
struct Cli {
    /// Vault root (Markdown source of truth).
    #[arg(long, global = true, default_value = ".")]
    vault: PathBuf,

    /// Explicit config path (defaults to <vault>/.engram/config.toml).
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Initialize a vault (config plus a starter note).
    Init,
    /// Build or refresh the SQLite FTS index.
    Index,
    /// Full-text search.
    Search {
        query: String,
        /// Require ALL terms (AND) instead of any (OR).
        #[arg(long)]
        all: bool,
    },
    /// Lexical similarity via hash embeddings (brute-force cosine).
    Lexical { query: String },
    /// Ask the configured model a question, grounded in the vault.
    Ask { question: String },
    /// Export the vault to a sync folder.
    Export {
        dest: PathBuf,
        #[arg(long)]
        dry_run: bool,
    },
    /// Import from a sync folder.
    Import {
        src: PathBuf,
        #[arg(long)]
        dry_run: bool,
    },
    /// Open a local browser workspace for the vault.
    Serve {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 0)]
        port: u16,
        /// Optional project root granted to Threadline's local file tools.
        #[arg(long)]
        project_root: Option<PathBuf>,
    },
    /// Start the local workspace and open it in the system browser.
    Open {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 0)]
        port: u16,
        /// Optional project root granted to Threadline's local file tools.
        #[arg(long)]
        project_root: Option<PathBuf>,
    },
    /// Create an encrypted Markdown backup. Reads ENGRAM_BACKUP_PASSWORD.
    Backup { dest: PathBuf },
    /// Restore an encrypted Markdown backup. Reads ENGRAM_BACKUP_PASSWORD.
    Restore { src: PathBuf },
    /// Copy supported local text files into the vault and index them.
    ImportFiles { src: PathBuf },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let vault = cli.vault.clone();
    let config_path = cli
        .config
        .clone()
        .unwrap_or_else(|| vault.join(".engram").join("config.toml"));

    match cli.cmd {
        Cmd::Init => cmd_init(&vault, &config_path),
        Cmd::Index => cmd_index(&vault, &config_path),
        Cmd::Search { query, all } => cmd_search(&vault, &config_path, &query, all),
        Cmd::Lexical { query } => cmd_lexical(&vault, &config_path, &query),
        Cmd::Ask { question } => cmd_ask(&vault, &config_path, &question),
        Cmd::Export { dest, dry_run } => cmd_export(&vault, &config_path, &dest, dry_run),
        Cmd::Import { src, dry_run } => cmd_import(&vault, &config_path, &src, dry_run),
        Cmd::Serve {
            host,
            port,
            project_root,
        } => serve::run(&vault, project_root.as_deref(), &host, port),
        Cmd::Open {
            host,
            port,
            project_root,
        } => {
            ensure_open_vault(&vault, &config_path)?;
            serve::open(&vault, project_root.as_deref(), &host, port)
        }
        Cmd::Backup { dest } => cmd_backup(&vault, &dest),
        Cmd::Restore { src } => cmd_restore(&vault, &src),
        Cmd::ImportFiles { src } => cmd_import_files(&vault, &config_path, &src),
    }
}

fn backup_password() -> Result<String> {
    std::env::var("ENGRAM_BACKUP_PASSWORD")
        .context("set ENGRAM_BACKUP_PASSWORD; passwords are never accepted as CLI arguments")
}

fn cmd_backup(vault: &Path, dest: &Path) -> Result<()> {
    let manifest = engram_core::backup::create(vault, dest, &backup_password()?)?;
    println!(
        "encrypted backup: {} Markdown files -> {}",
        manifest.files,
        dest.display()
    );
    Ok(())
}

fn cmd_restore(vault: &Path, src: &Path) -> Result<()> {
    let manifest = engram_core::backup::restore(src, vault, &backup_password()?)?;
    println!(
        "restored {} Markdown files into {}",
        manifest.files,
        vault.display()
    );
    Ok(())
}

fn cmd_import_files(vault: &Path, config_path: &Path, source: &Path) -> Result<()> {
    let root = source
        .canonicalize()
        .with_context(|| format!("reading import root {}", source.display()))?;
    let destination = vault.join("Imported");
    std::fs::create_dir_all(&destination)?;
    let mut copied = 0usize;
    for entry in walkdir::WalkDir::new(&root)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        if !matches!(
            entry.path().extension().and_then(|x| x.to_str()),
            Some("md") | Some("markdown") | Some("txt")
        ) {
            continue;
        }
        let target = destination.join(entry.path().strip_prefix(&root)?);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if target.exists() {
            anyhow::bail!("import destination already exists: {}", target.display());
        }
        std::fs::copy(entry.path(), &target)?;
        copied += 1;
    }
    cmd_index(vault, config_path)?;
    println!(
        "imported {copied} local text files into {}",
        destination.display()
    );
    Ok(())
}

fn load_config(vault: &Path, config_path: &Path) -> Result<Config> {
    if config_path.exists() {
        Config::load(config_path)
    } else {
        // Deterministic defaults rooted at the vault, so read-only commands
        // work before `init` without minting a stored identity.
        Ok(Config::new(vault.to_path_buf()))
    }
}

fn ensure_open_vault(vault: &Path, config_path: &Path) -> Result<()> {
    if config_path.exists() {
        return Ok(());
    }

    if vault_has_visible_content(vault) {
        eprintln!(
            "warning: {} is not initialized; Threadline chat will work, but run `engram --vault {} init` to add config, a starter note, and indexing",
            vault.display(),
            vault.display()
        );
        return Ok(());
    }

    // `open` is the first-run path for a new vault. Initialization is best-effort:
    // a read-only or otherwise unavailable vault can still serve the chat UI.
    if let Err(error) = cmd_init(vault, config_path) {
        eprintln!(
            "warning: could not initialize {}: {error:#}; continuing without a config",
            vault.display()
        );
    }
    Ok(())
}

fn vault_has_visible_content(vault: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(vault) else {
        return true;
    };
    entries
        .filter_map(Result::ok)
        .any(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
}

fn cmd_init(vault: &Path, config_path: &Path) -> Result<()> {
    std::fs::create_dir_all(vault.join(".engram"))
        .with_context(|| format!("creating {}", vault.display()))?;

    let cfg = Config::new(vault.to_path_buf());
    if config_path.exists() {
        println!("config already exists at {}", config_path.display());
    } else {
        cfg.save(config_path)?;
        println!("wrote config {}", config_path.display());
    }

    let welcome = vault.join("Welcome.md");
    if !welcome.exists() {
        std::fs::write(
            &welcome,
            "# Welcome to Engram\n\n\
             Welcome! This vault is your local-first second brain.\n\n\
             Try: `engram index` then `engram search welcome`.\n\n\
             ## How it works\n\n\
             Markdown files are the source of truth. A SQLite FTS5 index powers full-text \
             search. Threadline can answer questions grounded in these notes with the built-in \
             on-device model on macOS 26+, or a local OpenAI-compatible endpoint you configure.\n",
        )
        .with_context(|| format!("writing {}", welcome.display()))?;
        println!("created {}", welcome.display());
    }

    println!("host: {}", hostname());
    println!("device id: {}", cfg.device_id);
    println!("vault ready: {}", vault.display());
    Ok(())
}

fn cmd_index(vault: &Path, config_path: &Path) -> Result<()> {
    let cfg = load_config(vault, config_path)?;
    let v = Vault::open(vault.to_path_buf())?;
    let idx = Index::open(&cfg.db)?;
    let embedder = HashingEmbedder::new(cfg.embed_dim);
    let count = idx.rebuild_from_vault(&v, &embedder, unix_now())?;
    println!("indexed {} notes into {}", count, cfg.db.display());
    Ok(())
}

fn cmd_search(vault: &Path, config_path: &Path, query: &str, all: bool) -> Result<()> {
    let cfg = load_config(vault, config_path)?;
    let idx = Index::open(&cfg.db)?;
    stale_warning(&idx, vault)?;

    let filter = sanitize_fts(query);
    if filter.all_stopwords {
        println!(
            "0 matches: \"{query}\" contains only stopwords, so there is nothing to search for. \
             Add at least one content word."
        );
        return Ok(());
    }

    let hits = idx.search(query, all, 20)?;
    if hits.is_empty() {
        println!("0 matches for \"{query}\"");
        return Ok(());
    }
    println!("{} match(es) for \"{query}\":", hits.len());
    for h in &hits {
        println!("- {} [{}]", h.title, h.id);
        let snippet = h.snippet.replace('\n', " ");
        let snippet = snippet.trim();
        if !snippet.is_empty() {
            println!("    {snippet}");
        }
    }
    Ok(())
}

fn cmd_lexical(vault: &Path, config_path: &Path, query: &str) -> Result<()> {
    let cfg = load_config(vault, config_path)?;
    let idx = Index::open(&cfg.db)?;
    stale_warning(&idx, vault)?;

    let embedder = HashingEmbedder::new(cfg.embed_dim);
    let query_vec = embedder.embed(query);

    let mut scored: Vec<(f32, engram_core::NoteLite)> = Vec::new();
    for (lite, emb) in idx.notes_with_embeddings()? {
        if emb.is_empty() {
            continue;
        }
        scored.push((cosine(&query_vec, &emb), lite));
    }
    if scored.is_empty() {
        println!("0 notes indexed — run `engram index` first");
        return Ok(());
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(10);

    println!("top lexical matches for \"{query}\":");
    for (score, lite) in scored {
        println!("- [{score:.3}] {} ({})", lite.title, lite.id);
    }
    Ok(())
}

fn cmd_ask(vault: &Path, config_path: &Path, question: &str) -> Result<()> {
    let cfg = load_config(vault, config_path)?;
    let idx = Index::open(&cfg.db)?;
    stale_warning(&idx, vault)?;

    let hits = idx.search(question, false, 6)?;
    let mut notes: Vec<(String, String)> = Vec::with_capacity(hits.len());
    for h in &hits {
        let body = idx.get_note_body(&h.id)?.unwrap_or_default();
        notes.push((h.id.clone(), body));
    }

    let max_chars = cfg.context_window.saturating_mul(4); // ~4 chars/token
    let prompt = build_context_prompt(question, &notes, max_chars);

    let client = OpenAiCompat::new(
        cfg.llm_base.clone(),
        cfg.llm_model.clone(),
        cfg.llm_key.clone(),
        cfg.provider,
    );
    let opts = ChatOpts {
        ..Default::default()
    };
    let answer = client.complete(&prompt, &opts)?;
    println!("{answer}");
    Ok(())
}

fn cmd_export(vault: &Path, config_path: &Path, dest: &Path, dry_run: bool) -> Result<()> {
    let cfg = load_config(vault, config_path)?;
    let sync = FolderSync::new(cfg.device_id, dry_run);
    let stats = sync.export(vault, dest)?;
    let tag = if dry_run { "[dry-run] " } else { "" };
    println!(
        "{tag}export: {} copied, {} skipped",
        stats.copied, stats.skipped
    );
    Ok(())
}

fn cmd_import(vault: &Path, config_path: &Path, src: &Path, dry_run: bool) -> Result<()> {
    let cfg = load_config(vault, config_path)?;
    let sync = FolderSync::new(cfg.device_id, dry_run);
    let stats = sync.import(vault, src)?;
    let tag = if dry_run { "[dry-run] " } else { "" };
    println!(
        "{tag}import: {} copied ({} overwrites), {} skipped, {} conflicts",
        stats.copied, stats.overwrites, stats.skipped, stats.conflicts
    );
    Ok(())
}

fn stale_warning(idx: &Index, vault: &Path) -> Result<()> {
    match idx.get_last_indexed()? {
        None => eprintln!("warning: this vault has never been indexed (run `engram index`)"),
        Some(ts) => {
            let v = Vault::open(vault.to_path_buf())?;
            let mut newest = 0i64;
            v.scan_each(|n| {
                if n.mtime > newest {
                    newest = n.mtime;
                }
                Ok(())
            })?;
            if newest > ts {
                eprintln!(
                    "warning: index may be stale (vault changed after last index; run `engram index`)"
                );
            }
        }
    }
    Ok(())
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "node".to_string())
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn test_vault(label: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "engram-open-{label}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create test vault");
        dir
    }

    fn config_path(vault: &Path) -> PathBuf {
        vault.join(".engram").join("config.toml")
    }

    #[test]
    fn open_initializes_an_empty_vault() {
        let vault = test_vault("empty");
        let config = config_path(&vault);
        ensure_open_vault(&vault, &config).expect("initialize empty vault");
        assert!(config.exists());
        assert!(vault.join("Welcome.md").exists());
        let _ = std::fs::remove_dir_all(vault);
    }

    #[test]
    fn open_ignores_hidden_files_when_deciding_whether_a_vault_is_empty() {
        let vault = test_vault("hidden");
        std::fs::write(vault.join(".DS_Store"), b"finder metadata").unwrap();
        let config = config_path(&vault);
        ensure_open_vault(&vault, &config).expect("initialize vault with hidden metadata");
        assert!(config.exists());
        assert!(vault.join("Welcome.md").exists());
        let _ = std::fs::remove_dir_all(vault);
    }

    #[test]
    fn open_does_not_inject_a_starter_note_into_an_existing_vault() {
        let vault = test_vault("existing");
        std::fs::write(vault.join("note.md"), "# Existing note\n").unwrap();
        let config = config_path(&vault);
        ensure_open_vault(&vault, &config).expect("leave existing vault explicit");
        assert!(!config.exists());
        assert!(!vault.join("Welcome.md").exists());
        assert_eq!(
            std::fs::read_to_string(vault.join("note.md")).unwrap(),
            "# Existing note\n"
        );
        let _ = std::fs::remove_dir_all(vault);
    }

    #[test]
    fn open_does_not_overwrite_an_existing_config() {
        let vault = test_vault("configured");
        let config = config_path(&vault);
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::fs::write(&config, "sentinel").unwrap();
        ensure_open_vault(&vault, &config).expect("leave existing config alone");
        assert_eq!(std::fs::read_to_string(&config).unwrap(), "sentinel");
        assert!(!vault.join("Welcome.md").exists());
        let _ = std::fs::remove_dir_all(vault);
    }
}
