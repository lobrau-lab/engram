use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CrawlReport {
    pub documents: usize,
    pub skipped: usize,
    pub links: usize,
    pub tags: usize,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Crawler;

impl Crawler {
    pub fn scan_roots(&self, roots: &[PathBuf]) -> Result<CrawlReport> {
        let mut report = CrawlReport::default();
        for root in roots {
            let text = root.to_string_lossy();
            if text.starts_with("http://") || text.starts_with("https://") {
                anyhow::bail!("network roots are disabled: {root:?}");
            }
            let root = root
                .canonicalize()
                .with_context(|| format!("canonicalizing crawl root {}", root.display()))?;
            if !root.is_dir() {
                anyhow::bail!("crawl root is not a directory: {}", root.display());
            }
            for entry in WalkDir::new(&root)
                .into_iter()
                .filter_entry(|entry| !is_hidden(entry.path(), &root))
            {
                let entry = entry?;
                if !entry.file_type().is_file() {
                    continue;
                }
                let supported = matches!(
                    entry.path().extension().and_then(|ext| ext.to_str()),
                    Some("md") | Some("markdown") | Some("txt")
                );
                if !supported {
                    report.skipped += 1;
                    continue;
                }
                let body = std::fs::read_to_string(entry.path())
                    .with_context(|| format!("reading {}", entry.path().display()))?;
                report.documents += 1;
                report.links += body.matches("[[").count();
                report.tags += body
                    .split_whitespace()
                    .filter(|token| token.starts_with('#') && token.len() > 1)
                    .count();
            }
        }
        Ok(report)
    }
}

fn is_hidden(path: &Path, root: &Path) -> bool {
    path != root
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with('.'))
}
