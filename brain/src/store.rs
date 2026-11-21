//! The index: one SQLite file with FTS5 and the vectors. It is derived data: any mismatch in model,
//! binary version or chunker version drops it and it is rebuilt from the files.

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use crate::{chunk, embed, vecs};

const SCHEMA: &str = "
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE files (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    hash TEXT NOT NULL,
    title TEXT NOT NULL,
    date TEXT NOT NULL,
    kind TEXT NOT NULL
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
    pub kind: String,
    pub chunks: Vec<NewChunk>,
}

pub fn cache_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("SUPERCHARGE_CACHE_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let home = std::env::var_os("HOME").context("HOME is not set; set SUPERCHARGE_CACHE_DIR")?;
    Ok(PathBuf::from(home).join(".cache/supercharge"))
}

/// `<cache>/index/<root-hash>/index.sqlite`
pub fn index_path(root: &Path) -> Result<PathBuf> {
    let hash = format!("{:x}", Sha256::digest(root.to_string_lossy().as_bytes()));
    Ok(cache_dir()?
        .join("index")
        .join(&hash[..16])
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
        std::fs::create_dir_all(path.parent().context("index path has no parent")?)?;
        let mut store = Store {
            conn: connect(path)?,
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

    /// path → content hash of every file in the index.
    pub fn hashes(&self) -> Result<BTreeMap<String, String>> {
        let mut stmt = self.conn.prepare("SELECT path, hash FROM files")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
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
                "INSERT INTO files(path, hash, title, date, kind) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![file.path, file.hash, file.title, file.date, file.kind],
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

fn remove(tx: &rusqlite::Transaction, path: &str) -> Result<()> {
    tx.execute(
        "DELETE FROM fts WHERE rowid IN (SELECT c.id FROM chunks c JOIN files f ON f.id = c.file_id WHERE f.path = ?1)",
        [path],
    )?;
    tx.execute("DELETE FROM files WHERE path = ?1", [path])?;
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
            kind: "raw".into(),
            chunks: texts
                .iter()
                .map(|t| {
                    let mut v = vec![0f32; embed::DIM];
                    v[0] = 1.0;
                    NewChunk {
                        heading: "".into(),
                        text: t.to_string(),
                        header: format!("T · {path}"),
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
        assert_eq!(s.hashes().unwrap()["a.md"], "h3");
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
    fn the_index_lives_under_the_cache_keyed_by_the_root() {
        std::env::set_var("SUPERCHARGE_CACHE_DIR", "/tmp/sc-cache-test");
        let a = index_path(Path::new("/x/brain")).unwrap();
        let b = index_path(Path::new("/y/brain")).unwrap();
        assert_ne!(a, b);
        assert!(a.starts_with("/tmp/sc-cache-test/index"));
        assert!(a.ends_with("index.sqlite"));
    }
}
