//! SQLite FTS5 index plus stored hash embeddings.

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashSet;
use std::path::Path;

use crate::embed::Embedder;
use crate::vault::Note;

pub const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "but", "by", "for", "from", "has", "have", "he",
    "her", "his", "i", "if", "in", "into", "is", "it", "its", "me", "my", "no", "not", "of", "on",
    "or", "our", "she", "so", "that", "the", "their", "them", "then", "there", "these", "they",
    "this", "to", "was", "we", "were", "what", "when", "where", "which", "who", "whom", "why",
    "will", "with", "you", "your",
];

/// Result of filtering a raw query against the stopword list.
///
/// `all_stopwords` is true when the query contained words but *every* one of
/// them was a stopword — the query fails closed (0 matches) rather than
/// silently matching everything.
#[derive(Debug, Clone, Default)]
pub struct QueryFilterResult {
    pub terms: Vec<String>,
    pub all_stopwords: bool,
}

impl QueryFilterResult {
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct NoteLite {
    pub id: String,
    pub title: String,
    pub tags: Vec<String>,
    pub links: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Hit {
    pub id: String,
    pub title: String,
    pub snippet: String,
    pub score: f64,
}

pub struct Index {
    conn: Connection,
}

impl Index {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let conn =
            Connection::open(path).with_context(|| format!("opening index {}", path.display()))?;
        let idx = Self { conn };
        idx.init_schema()?;
        Ok(idx)
    }

    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            CREATE TABLE IF NOT EXISTS meta (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS notes (
                id        TEXT PRIMARY KEY,
                title     TEXT NOT NULL,
                body      TEXT NOT NULL,
                tags      TEXT NOT NULL,
                links     TEXT NOT NULL,
                mtime     INTEGER NOT NULL,
                embedding BLOB
            );
            CREATE VIRTUAL TABLE IF NOT EXISTS notes_fts USING fts5(
                id UNINDEXED,
                title,
                body,
                tags,
                tokenize = 'porter unicode61'
            );
            "#,
        )?;
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        self.conn
            .execute_batch("DELETE FROM notes; DELETE FROM notes_fts;")?;
        Ok(())
    }

    /// Replace the derived index in one transaction. If any note fails, the
    /// previous searchable index remains available.
    pub fn rebuild(&self, notes: &[Note], embedder: &dyn Embedder, indexed_at: i64) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute_batch("DELETE FROM notes; DELETE FROM notes_fts; DELETE FROM meta WHERE key = 'last_indexed';")?;
        for note in notes {
            let tags = note.tags.join(",");
            let links = note.links.join(",");
            let embedding =
                f32s_to_bytes(&embedder.embed(&format!("{} {} {}", note.title, note.body, tags)));
            tx.execute(
                "INSERT INTO notes(id, title, body, tags, links, mtime, embedding) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![note.id, note.title, note.body, tags, links, note.mtime, embedding],
            )?;
            tx.execute(
                "INSERT INTO notes_fts(id, title, body, tags) VALUES(?1, ?2, ?3, ?4)",
                params![note.id, note.title, note.body, tags],
            )?;
        }
        tx.execute(
            "INSERT INTO meta(key, value) VALUES('last_indexed', ?1)",
            params![indexed_at.to_string()],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn set_last_indexed(&self, ts: i64) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO meta(key, value) VALUES('last_indexed', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![ts.to_string()],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn get_last_indexed(&self) -> Result<Option<i64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM meta WHERE key = 'last_indexed'")?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            let v: String = row.get(0)?;
            Ok(v.parse::<i64>().ok())
        } else {
            Ok(None)
        }
    }

    pub fn has_state_notes(&self) -> Result<bool> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM notes", [], |r| r.get(0))?;
        Ok(count > 0)
    }

    pub fn upsert(&self, note: &Note, embedder: &dyn Embedder) -> Result<()> {
        let tags = note.tags.join(",");
        let links = note.links.join(",");
        let emb = embedder.embed(&format!("{} {} {}", note.title, note.body, tags));
        let emb_bytes = f32s_to_bytes(&emb);

        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO notes(id, title, body, tags, links, mtime, embedding)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                body = excluded.body,
                tags = excluded.tags,
                links = excluded.links,
                mtime = excluded.mtime,
                embedding = excluded.embedding",
            params![note.id, note.title, note.body, tags, links, note.mtime, emb_bytes],
        )?;

        tx.execute("DELETE FROM notes_fts WHERE id = ?1", params![note.id])?;
        tx.execute(
            "INSERT INTO notes_fts(id, title, body, tags) VALUES(?1, ?2, ?3, ?4)",
            params![note.id, note.title, note.body, tags],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn get_note_body(&self, id: &str) -> Result<Option<String>> {
        let body: Option<String> = self
            .conn
            .query_row("SELECT body FROM notes WHERE id = ?1", params![id], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(body)
    }

    pub fn all_notes_lite(&self) -> Result<Vec<NoteLite>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, title, tags, links FROM notes ORDER BY id")?;
        let rows = stmt.query_map([], |r| {
            let tags: String = r.get(2)?;
            let links: String = r.get(3)?;
            Ok(NoteLite {
                id: r.get(0)?,
                title: r.get(1)?,
                tags: split_csv(&tags),
                links: split_csv(&links),
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Notes paired with their stored embeddings, for brute-force cosine search.
    pub fn notes_with_embeddings(&self) -> Result<Vec<(NoteLite, Vec<f32>)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, title, tags, links, embedding FROM notes ORDER BY id")?;
        let rows = stmt.query_map([], |r| {
            let tags: String = r.get(2)?;
            let links: String = r.get(3)?;
            let emb: Option<Vec<u8>> = r.get(4)?;
            let lite = NoteLite {
                id: r.get(0)?,
                title: r.get(1)?,
                tags: split_csv(&tags),
                links: split_csv(&links),
            };
            Ok((lite, emb.map(|b| bytes_to_f32s(&b)).unwrap_or_default()))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn search(&self, query: &str, all: bool, limit: usize) -> Result<Vec<Hit>> {
        let filtered = sanitize_fts(query);
        if filtered.is_empty() {
            return Ok(Vec::new());
        }
        let expr = build_match(&filtered.terms, all);

        let mut stmt = self.conn.prepare(
            "SELECT n.id, n.title, snippet(notes_fts, 2, '[', ']', '...', 12), bm25(notes_fts)
             FROM notes_fts
             JOIN notes n ON n.id = notes_fts.id
             WHERE notes_fts MATCH ?1
             ORDER BY bm25(notes_fts)
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![expr, limit as i64], |r| {
            Ok(Hit {
                id: r.get(0)?,
                title: r.get(1)?,
                snippet: r.get(2)?,
                score: r.get(3)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}

/// Tokenize a query, strip English stopwords, and report whether it was
/// stopword-only. Callers convert `terms` into an OR (default) or AND
/// (`--all`) FTS expression.
pub fn sanitize_fts(query: &str) -> QueryFilterResult {
    let stop: HashSet<&str> = STOPWORDS.iter().copied().collect();
    let mut terms = Vec::new();
    let mut saw_word = false;
    for raw in query.split(|c: char| !c.is_alphanumeric()) {
        if raw.is_empty() {
            continue;
        }
        saw_word = true;
        let low = raw.to_lowercase();
        if stop.contains(low.as_str()) {
            continue;
        }
        terms.push(low);
    }
    QueryFilterResult {
        all_stopwords: saw_word && terms.is_empty(),
        terms,
    }
}

fn build_match(terms: &[String], all: bool) -> String {
    let joiner = if all { " AND " } else { " OR " };
    terms
        .iter()
        .map(|t| format!("\"{}\"", t.replace('"', "")))
        .collect::<Vec<_>>()
        .join(joiner)
}

fn split_csv(s: &str) -> Vec<String> {
    if s.is_empty() {
        return Vec::new();
    }
    s.split(',')
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

fn f32s_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

fn bytes_to_f32s(b: &[u8]) -> Vec<f32> {
    b.as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::Index;

    #[test]
    fn set_last_indexed_survives_reopening_the_index() {
        let dir = std::env::temp_dir().join(format!(
            "engram-index-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("index.sqlite");

        {
            let index = Index::open(&path).unwrap();
            index.set_last_indexed(42).unwrap();
        }
        let reopened = Index::open(&path).unwrap();
        assert_eq!(reopened.get_last_indexed().unwrap(), Some(42));
        drop(reopened);
        let _ = std::fs::remove_dir_all(dir);
    }
}
