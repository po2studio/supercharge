//! The index: one SQLite file with FTS5 and the vectors. It is derived data: any mismatch in model,
//! binary version or chunker version drops it and it is rebuilt from the files.

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use crate::{chunk, dedup, embed, vecs};

const SCHEMA: &str = "
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE files (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    hash TEXT NOT NULL,
    title TEXT NOT NULL,
    date TEXT NOT NULL,
    kind TEXT NOT NULL,
    -- The date the document gives itself (frontmatter, or Office or email properties), if any.
    own_date TEXT,
    -- A fingerprint of the text for near-copies; none for a text too short to have one.
    simhash INTEGER,
    -- The path of the newest copy of this file, when a newer copy exists: this file is hidden.
    dup_of TEXT
);
CREATE INDEX files_dup ON files(dup_of);
-- Files that have no text to index, with why; kept by content hash so they are not tried again.
CREATE TABLE unconverted (
    path TEXT PRIMARY KEY,
    hash TEXT NOT NULL,
    reason TEXT NOT NULL
);
CREATE TABLE chunks (
    id INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    ord INTEGER NOT NULL,
    heading TEXT NOT NULL,
    text TEXT NOT NULL,
    scale REAL NOT NULL,
    vec BLOB NOT NULL,
    code BLOB NOT NULL
);
CREATE INDEX chunks_file ON chunks(file_id);
CREATE VIRTUAL TABLE fts USING fts5(header, body, tokenize = 'unicode61 remove_diacritics 2');
";

pub struct Store {
    pub conn: Connection,
    /// The folder of the index file: the conversion cache lives beside it.
    pub dir: PathBuf,
}

/// What the index holds about a file, to tell whether it changed.
#[derive(Debug, PartialEq)]
pub struct FileRow {
    pub hash: String,
    pub date: String,
    pub own_date: Option<String>,
}

/// A chunk ready to be stored.
pub struct NewChunk {
    pub heading: String,
    pub text: String,
    /// What FTS indexes besides the text: `title › heading · path · date`.
    pub header: String,
    pub vector: Vec<f32>,
}

pub struct NewFile {
    pub path: String,
    pub hash: String,
    pub title: String,
    pub date: String,
    pub own_date: Option<String>,
    pub kind: String,
    pub simhash: Option<u64>,
    pub chunks: Vec<NewChunk>,
}

pub fn cache_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("SUPERCHARGE_CACHE_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let home = std::env::var_os("HOME").context("HOME is not set; set SUPERCHARGE_CACHE_DIR")?;
    Ok(PathBuf::from(home).join(".cache/supercharge"))
}

/// Names a source's index folder: the first 16 hex digits of the hash of its key
/// (`Source::key`), so projects that list the same folder share one index.
pub fn key_hash(key: &str) -> String {
    let hash = format!("{:x}", Sha256::digest(key.as_bytes()));
    hash[..16].to_string()
}

/// `<cache>/index/<source-hash>/index.sqlite`
pub fn index_path(key: &str) -> Result<PathBuf> {
    Ok(cache_dir()?
        .join("index")
        .join(key_hash(key))
        .join("index.sqlite"))
}

fn expected_meta() -> [(&'static str, String); 3] {
    [
        ("model_id", embed::MODEL_ID.to_string()),
        ("binary_version", env!("CARGO_PKG_VERSION").to_string()),
        ("chunker_version", chunk::VERSION.to_string()),
    ]
}

impl Store {
    /// Opens the index at `path`, creating it, or recreating it when it was built by a different
    /// model, binary version or chunker.
    pub fn create(path: &Path, root: &Path) -> Result<Store> {
        let dir = path.parent().context("index path has no parent")?;
        std::fs::create_dir_all(dir)?;
        let mut store = Store {
            conn: connect(path)?,
            dir: dir.to_path_buf(),
        };
        if !store.matches()? {
            drop(store);
            for suffix in ["", "-wal", "-shm"] {
                let mut p = path.as_os_str().to_owned();
                p.push(suffix);
                let _ = std::fs::remove_file(p);
            }
            store = Store {
                conn: connect(path)?,
                dir: dir.to_path_buf(),
            };
            store.conn.execute_batch(SCHEMA)?;
            for (k, v) in expected_meta() {
                store.set_meta(k, &v)?;
            }
            store.set_meta("root", &root.to_string_lossy())?;
        }
        Ok(store)
    }

    /// Opens an existing index for reading; `None` when there is none, or it is out of date.
    pub fn open(path: &Path) -> Result<Option<Store>> {
        if !path.exists() {
            return Ok(None);
        }
        let store = Store {
            conn: connect(path)?,
            dir: path.parent().map(Path::to_path_buf).unwrap_or_default(),
        };
        Ok(store.matches()?.then_some(store))
    }

    fn matches(&self) -> Result<bool> {
        let has_meta: bool = self.conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name = 'meta'",
            [],
            |r| r.get::<_, i64>(0),
        )? > 0;
        if !has_meta {
            return Ok(false);
        }
        for (k, v) in expected_meta() {
            if self.meta(k)?.as_deref() != Some(v.as_str()) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
            .optional()?)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, value],
        )?;
        Ok(())
    }

    /// What the index holds about each file in it, by path.
    pub fn files(&self) -> Result<BTreeMap<String, FileRow>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path, hash, date, own_date FROM files")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get(0)?,
                FileRow {
                    hash: r.get(1)?,
                    date: r.get(2)?,
                    own_date: r.get(3)?,
                },
            ))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// path → (content hash, reason) of every file that has no text to index.
    pub fn unconverted(&self) -> Result<BTreeMap<String, (String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path, hash, reason FROM unconverted")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?))))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Notes that a file has no text to index, replacing whatever the index held for it.
    pub fn put_unconverted(&mut self, path: &str, hash: &str, reason: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        remove(&tx, path)?;
        tx.execute(
            "INSERT INTO unconverted(path, hash, reason) VALUES (?1, ?2, ?3)",
            [path, hash, reason],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Every indexed file as the duplicate grouping sees it.
    pub fn dedup_items(&self) -> Result<Vec<dedup::Item>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path, hash, date, simhash FROM files")?;
        let rows = stmt.query_map([], |r| {
            Ok(dedup::Item {
                path: r.get(0)?,
                hash: r.get(1)?,
                date: r.get(2)?,
                fingerprint: r.get::<_, Option<i64>>(3)?.map(|f| f as u64),
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Records which files are copies of a newer one: `(copy, newest)`. Replaces the earlier groups.
    pub fn set_copies(&mut self, copies: &[(String, String)]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE files SET dup_of = NULL WHERE dup_of IS NOT NULL",
            [],
        )?;
        for (copy, newest) in copies {
            tx.execute(
                "UPDATE files SET dup_of = ?1 WHERE path = ?2",
                [newest, copy],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// How many files are hidden behind a newer copy.
    pub fn duplicates(&self) -> Result<usize> {
        Ok(self.conn.query_row(
            "SELECT count(*) FROM files WHERE dup_of IS NOT NULL",
            [],
            |r| r.get::<_, i64>(0),
        )? as usize)
    }

    /// Sets a file's date, in the file and in every chunk's full-text header, without embedding
    /// again: the vectors keep the date they were embedded with, which is a minor part of them.
    pub fn refresh_date(&mut self, path: &str, date: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        let (file_id, title): (i64, String) =
            tx.query_row("SELECT id, title FROM files WHERE path = ?1", [path], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        tx.execute(
            "UPDATE files SET date = ?1 WHERE id = ?2",
            params![date, file_id],
        )?;
        let chunks: Vec<(i64, String)> = {
            let mut stmt = tx.prepare("SELECT id, heading FROM chunks WHERE file_id = ?1")?;
            let rows = stmt.query_map([file_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<Result<_, _>>()?
        };
        for (id, heading) in chunks {
            tx.execute(
                "UPDATE fts SET header = ?1 WHERE rowid = ?2",
                params![header(&title, &heading, path, date), id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn remove_file(&mut self, path: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        remove(&tx, path)?;
        tx.commit()?;
        Ok(())
    }

    /// Replaces each file's rows in one transaction.
    pub fn put_files(&mut self, files: &[NewFile]) -> Result<()> {
        let tx = self.conn.transaction()?;
        for file in files {
            remove(&tx, &file.path)?;
            tx.execute(
                "INSERT INTO files(path, hash, title, date, kind, own_date, simhash) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    file.path,
                    file.hash,
                    file.title,
                    file.date,
                    file.kind,
                    file.own_date,
                    file.simhash.map(|f| f as i64)
                ],
            )?;
            let file_id = tx.last_insert_rowid();
            for (ord, c) in file.chunks.iter().enumerate() {
                let (scale, q) = vecs::quantize(&c.vector);
                tx.execute(
                    "INSERT INTO chunks(file_id, ord, heading, text, scale, vec, code) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![file_id, ord as i64, c.heading, c.text, scale, vecs::i8_bytes(&q), vecs::code(&c.vector).to_vec()],
                )?;
                let chunk_id = tx.last_insert_rowid();
                tx.execute(
                    "INSERT INTO fts(rowid, header, body) VALUES (?1, ?2, ?3)",
                    params![chunk_id, c.header, c.text],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn counts(&self) -> Result<(i64, i64)> {
        let files = self
            .conn
            .query_row("SELECT count(*) FROM files", [], |r| r.get(0))?;
        let chunks = self
            .conn
            .query_row("SELECT count(*) FROM chunks", [], |r| r.get(0))?;
        Ok((files, chunks))
    }
}

/// What FTS indexes besides the text, and what is embedded with it: `title › heading · path · date`.
pub fn header(title: &str, heading: &str, path: &str, date: &str) -> String {
    if heading.is_empty() {
        format!("{title} · {path} · {date}")
    } else {
        format!("{title} › {heading} · {path} · {date}")
    }
}

fn remove(tx: &rusqlite::Transaction, path: &str) -> Result<()> {
    tx.execute(
        "DELETE FROM fts WHERE rowid IN (SELECT c.id FROM chunks c JOIN files f ON f.id = c.file_id WHERE f.path = ?1)",
        [path],
    )?;
    tx.execute("DELETE FROM files WHERE path = ?1", [path])?;
    tx.execute("DELETE FROM unconverted WHERE path = ?1", [path])?;
    Ok(())
}

fn connect(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn file(path: &str, hash: &str, texts: &[&str]) -> NewFile {
        NewFile {
            path: path.into(),
            hash: hash.into(),
            title: "T".into(),
            date: "2024-01-01".into(),
            own_date: None,
            kind: "raw".into(),
            simhash: None,
            chunks: texts
                .iter()
                .map(|t| {
                    let mut v = vec![0f32; embed::DIM];
                    v[0] = 1.0;
                    NewChunk {
                        heading: "".into(),
                        text: t.to_string(),
                        header: format!("T · {path} · 2024-01-01"),
                        vector: v,
                    }
                })
                .collect(),
        }
    }

    #[test]
    fn put_replaces_a_files_rows_and_leaves_no_orphans() {
        let dir = TempDir::new().unwrap();
        let mut s = Store::create(&dir.path().join("i/index.sqlite"), dir.path()).unwrap();
        s.put_files(&[
            file("a.md", "h1", &["one", "two"]),
            file("b.md", "h2", &["three"]),
        ])
        .unwrap();
        assert_eq!(s.counts().unwrap(), (2, 3));
        s.put_files(&[file("a.md", "h3", &["uno"])]).unwrap();
        assert_eq!(s.counts().unwrap(), (2, 2));
        assert_eq!(s.files().unwrap()["a.md"].hash, "h3");
        let fts: i64 = s
            .conn
            .query_row("SELECT count(*) FROM fts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fts, 2);
        let hits: i64 = s
            .conn
            .query_row("SELECT count(*) FROM fts WHERE fts MATCH 'two'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(hits, 0);
        s.remove_file("b.md").unwrap();
        assert_eq!(s.counts().unwrap(), (1, 1));
    }

    #[test]
    fn refreshing_a_date_changes_the_file_and_what_full_text_matches_without_touching_vectors() {
        let dir = TempDir::new().unwrap();
        let mut s = Store::create(&dir.path().join("i/index.sqlite"), dir.path()).unwrap();
        s.put_files(&[file("a.md", "h1", &["one", "two"])]).unwrap();
        let before: Vec<Vec<u8>> = {
            let mut stmt = s
                .conn
                .prepare("SELECT vec FROM chunks ORDER BY id")
                .unwrap();
            let rows = stmt.query_map([], |r| r.get(0)).unwrap();
            rows.collect::<Result<_, _>>().unwrap()
        };
        s.refresh_date("a.md", "2031-02-03").unwrap();
        assert_eq!(
            s.files().unwrap()["a.md"],
            FileRow {
                hash: "h1".into(),
                date: "2031-02-03".into(),
                own_date: None
            }
        );
        let matching = |q: &str| -> i64 {
            s.conn
                .query_row("SELECT count(*) FROM fts WHERE fts MATCH ?1", [q], |r| {
                    r.get(0)
                })
                .unwrap()
        };
        assert_eq!(matching("\"2031-02-03\""), 2);
        assert_eq!(matching("\"2024-01-01\""), 0);
        let after: Vec<Vec<u8>> = {
            let mut stmt = s
                .conn
                .prepare("SELECT vec FROM chunks ORDER BY id")
                .unwrap();
            let rows = stmt.query_map([], |r| r.get(0)).unwrap();
            rows.collect::<Result<_, _>>().unwrap()
        };
        assert_eq!(before, after);
    }

    #[test]
    fn a_different_model_or_version_drops_the_index() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("i/index.sqlite");
        let mut s = Store::create(&path, dir.path()).unwrap();
        s.put_files(&[file("a.md", "h1", &["one"])]).unwrap();
        drop(s);
        assert_eq!(
            Store::create(&path, dir.path()).unwrap().counts().unwrap(),
            (1, 1),
            "same build keeps it"
        );
        let s = Store::open(&path).unwrap().unwrap();
        s.set_meta("model_id", "some-other-model").unwrap();
        drop(s);
        assert!(
            Store::open(&path).unwrap().is_none(),
            "reading refuses an out-of-date index"
        );
        assert_eq!(
            Store::create(&path, dir.path()).unwrap().counts().unwrap(),
            (0, 0),
            "writing rebuilds"
        );
    }

    #[test]
    fn the_index_lives_under_the_cache_keyed_by_the_source() {
        std::env::set_var("SUPERCHARGE_CACHE_DIR", "/tmp/sc-cache-test");
        let a = index_path("/x/brain").unwrap();
        let b = index_path("/y/brain").unwrap();
        assert_ne!(a, b);
        assert!(a.starts_with("/tmp/sc-cache-test/index"));
        assert!(a.ends_with("index.sqlite"));
    }

    #[test]
    fn a_file_is_either_indexed_or_unconverted_and_copies_are_recorded() {
        let dir = TempDir::new().unwrap();
        let mut s = Store::create(&dir.path().join("i/index.sqlite"), dir.path()).unwrap();
        s.put_files(&[file("a.md", "h1", &["one"]), file("b.md", "h2", &["two"])])
            .unwrap();
        s.put_unconverted("a.md", "h3", "now a scan").unwrap();
        assert_eq!(s.counts().unwrap(), (1, 1), "its text left the index");
        assert_eq!(
            s.unconverted().unwrap()["a.md"],
            ("h3".into(), "now a scan".into())
        );
        s.put_files(&[file("a.md", "h4", &["uno"])]).unwrap();
        assert!(s.unconverted().unwrap().is_empty(), "and back");
        s.set_copies(&[("a.md".into(), "b.md".into())]).unwrap();
        assert_eq!(s.duplicates().unwrap(), 1);
        s.set_copies(&[]).unwrap();
        assert_eq!(s.duplicates().unwrap(), 0);
        s.put_unconverted("b.md", "h5", "x").unwrap();
        s.remove_file("b.md").unwrap();
        assert!(s.unconverted().unwrap().is_empty());
    }
}
