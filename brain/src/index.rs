//! Walks the brain, and embeds what changed since the last run. Reads files; never writes to them.

use anyhow::{Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    path::Path,
    process::Command,
    time::Instant,
};

use crate::{
    chunk::{self, Params},
    doc,
    embed::Embedder,
    store::{NewChunk, NewFile, Store},
};

/// Chunks embedded together; also how much work a crash can lose.
const GROUP: usize = 128;

#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub files: usize,
    pub chunks: usize,
    pub files_changed: usize,
    pub files_removed: usize,
    pub embedded_chunks: usize,
    /// Files with an extension no converter exists for yet, by extension.
    pub not_indexed: BTreeMap<String, usize>,
    pub seconds: f64,
}

fn is_text(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(str::to_lowercase)
            .as_deref(),
        Some("md" | "markdown" | "txt" | "text")
    )
}

/// Text files (relative `/` paths, sorted) and a count of the other files by extension.
pub fn walk(root: &Path) -> Result<(Vec<String>, BTreeMap<String, usize>)> {
    let mut files = Vec::new();
    let mut other = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
            let entry = entry?;
            let name = entry.file_name();
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            let path = entry.path();
            let kind = entry.file_type()?;
            let (is_dir, is_file) = if kind.is_symlink() {
                match fs::metadata(&path) {
                    Ok(m) => (false, m.is_file()),
                    Err(_) => (false, false),
                }
            } else {
                (kind.is_dir(), kind.is_file())
            };
            if is_dir {
                stack.push(path);
            } else if is_file {
                if is_text(&path) {
                    let rel = path
                        .strip_prefix(root)?
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy())
                        .collect::<Vec<_>>()
                        .join("/");
                    files.push(rel);
                } else {
                    let ext = path
                        .extension()
                        .map_or("(none)".to_string(), |e| e.to_string_lossy().to_lowercase());
                    *other.entry(ext).or_insert(0) += 1;
                }
            }
        }
    }
    files.sort();
    Ok((files, other))
}

/// Last commit date of each file, when the brain is in a git repo. Read-only.
fn git_dates(root: &Path) -> HashMap<String, String> {
    let mut dates = HashMap::new();
    let Ok(out) = Command::new("git")
        .args([
            "-c",
            "core.quotepath=off",
            "log",
            "--relative",
            "--format=%x00%cs",
            "--name-only",
        ])
        .current_dir(root)
        .output()
    else {
        return dates;
    };
    if !out.status.success() {
        return dates;
    }
    let mut current = String::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some(date) = line.strip_prefix('\0') {
            current = date.to_string();
        } else if !line.is_empty() {
            dates
                .entry(line.to_string())
                .or_insert_with(|| current.clone());
        }
    }
    dates
}

struct Pending {
    file: NewFile,
    texts: Vec<String>,
}

pub fn index(
    root: &Path,
    store: &mut Store,
    embedder: &mut dyn Embedder,
    progress: bool,
) -> Result<Report> {
    let started = Instant::now();
    let (files, not_indexed) = walk(root)?;
    let known = store.hashes()?;
    let on_disk: BTreeSet<&str> = files.iter().map(String::as_str).collect();
    let mut report = Report {
        files: files.len(),
        not_indexed,
        ..Default::default()
    };

    for path in known.keys().filter(|p| !on_disk.contains(p.as_str())) {
        store.remove_file(path)?;
        report.files_removed += 1;
    }

    let git = git_dates(root);
    let params = Params::default();
    let mut pending: Vec<Pending> = Vec::new();
    let mut queued = 0;
    for (n, rel) in files.iter().enumerate() {
        let full = root.join(rel);
        let bytes = fs::read(&full).with_context(|| format!("reading {}", full.display()))?;
        let hash = format!("{:x}", Sha256::digest(&bytes));
        if known.get(rel) == Some(&hash) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        let parsed = doc::parse(&text);
        let title = parsed.title.clone().unwrap_or_else(|| {
            Path::new(rel)
                .file_stem()
                .map_or(rel.clone(), |s| s.to_string_lossy().into_owned())
        });
        let date = parsed
            .date
            .clone()
            .or_else(|| git.get(rel).cloned())
            .unwrap_or_else(|| {
                doc::date_of(
                    fs::metadata(&full)
                        .and_then(|m| m.modified())
                        .unwrap_or(std::time::UNIX_EPOCH),
                )
            });
        let count = |s: &str| embedder.count_tokens(s);
        let chunks = chunk::chunk(parsed.body, &count, &params);
        let mut texts = Vec::new();
        let new_chunks = chunks
            .into_iter()
            .map(|c| {
                let header = if c.heading.is_empty() {
                    format!("{title} · {rel} · {date}")
                } else {
                    format!("{title} › {} · {rel} · {date}", c.heading)
                };
                texts.push(format!("{header}\n{}", c.text));
                NewChunk {
                    heading: c.heading,
                    text: c.text,
                    header,
                    vector: Vec::new(),
                }
            })
            .collect::<Vec<_>>();
        queued += new_chunks.len();
        report.files_changed += 1;
        pending.push(Pending {
            file: NewFile {
                path: rel.clone(),
                hash,
                title,
                date,
                kind: doc::kind_of(rel).to_string(),
                chunks: new_chunks,
            },
            texts,
        });
        if queued >= GROUP {
            report.embedded_chunks += flush(store, embedder, &mut pending)?;
            queued = 0;
            if progress {
                eprintln!("indexed {}/{} files", n + 1, files.len());
            }
        }
    }
    report.embedded_chunks += flush(store, embedder, &mut pending)?;

    report.chunks = store.counts()?.1 as usize;
    report.seconds = started.elapsed().as_secs_f64();
    store.set_meta("last_index", &serde_json::to_string(&report)?)?;
    store.set_meta("not_indexed", &serde_json::to_string(&report.not_indexed)?)?;
    Ok(report)
}

fn flush(
    store: &mut Store,
    embedder: &mut dyn Embedder,
    pending: &mut Vec<Pending>,
) -> Result<usize> {
    if pending.is_empty() {
        return Ok(0);
    }
    let refs: Vec<&str> = pending
        .iter()
        .flat_map(|p| p.texts.iter().map(String::as_str))
        .collect();
    let mut vectors = embedder.embed_passages(&refs)?.into_iter();
    let mut total = 0;
    let mut files = Vec::new();
    for mut p in pending.drain(..) {
        for c in &mut p.file.chunks {
            c.vector = vectors
                .next()
                .context("embedder returned too few vectors")?;
            total += 1;
        }
        files.push(p.file);
    }
    store.put_files(&files)?;
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::Fake;
    use tempfile::TempDir;

    fn setup(files: &[(&str, &str)]) -> (TempDir, TempDir) {
        let brain = TempDir::new().unwrap();
        for (p, t) in files {
            let full = brain.path().join(p);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, t).unwrap();
        }
        (brain, TempDir::new().unwrap())
    }

    fn run(brain: &TempDir, cache: &TempDir) -> Report {
        let mut store = Store::create(&cache.path().join("index.sqlite"), brain.path()).unwrap();
        index(brain.path(), &mut store, &mut Fake, false).unwrap()
    }

    #[test]
    fn walk_finds_text_files_and_counts_the_rest_and_skips_hidden() {
        let (brain, _) = setup(&[
            ("a.md", "x"),
            ("sub/b.TXT", "x"),
            ("sub/deep/c.markdown", "x"),
            (".git/config", "x"),
            (".hidden/d.md", "x"),
            ("scan.pdf", "x"),
            ("scan2.PDF", "x"),
            ("Makefile", "x"),
        ]);
        let (files, other) = walk(brain.path()).unwrap();
        assert_eq!(files, ["a.md", "sub/b.TXT", "sub/deep/c.markdown"]);
        assert_eq!(other["pdf"], 2);
        assert_eq!(other["(none)"], 1);
    }

    #[test]
    fn second_run_embeds_nothing_and_a_change_embeds_one_files_chunks() {
        let (brain, cache) = setup(&[
            ("a.md", "# A\n\nfirst file text"),
            ("b.md", "# B\n\nsecond file text"),
        ]);
        let r1 = run(&brain, &cache);
        assert_eq!((r1.files, r1.embedded_chunks, r1.files_changed), (2, 2, 2));
        let r2 = run(&brain, &cache);
        assert_eq!((r2.embedded_chunks, r2.files_changed, r2.chunks), (0, 0, 2));
        fs::write(brain.path().join("a.md"), "# A\n\nfirst file text, edited").unwrap();
        let r3 = run(&brain, &cache);
        assert_eq!((r3.embedded_chunks, r3.files_changed), (1, 1));
    }

    #[test]
    fn deleted_files_leave_the_index() {
        let (brain, cache) = setup(&[("a.md", "alpha"), ("b.md", "beta")]);
        run(&brain, &cache);
        fs::remove_file(brain.path().join("b.md")).unwrap();
        let r = run(&brain, &cache);
        assert_eq!((r.files, r.chunks, r.files_removed), (1, 1, 1));
    }

    #[test]
    fn title_date_and_kind_come_from_the_file() {
        let (brain, cache) = setup(&[
            (
                "decisions/2024-05-14-x.md",
                "---\ntitle: Second phone\ndate: 2024-05-14\n---\nWe added it.",
            ),
            ("plain.txt", "no structure at all"),
        ]);
        run(&brain, &cache);
        let store = Store::open(&cache.path().join("index.sqlite"))
            .unwrap()
            .unwrap();
        let row = |p: &str| -> (String, String, String) {
            store
                .conn
                .query_row(
                    "SELECT title, date, kind FROM files WHERE path = ?1",
                    [p],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap()
        };
        assert_eq!(
            row("decisions/2024-05-14-x.md"),
            (
                "Second phone".into(),
                "2024-05-14".into(),
                "decision".into()
            )
        );
        let (title, date, kind) = row("plain.txt");
        assert_eq!((title.as_str(), kind.as_str()), ("plain", "raw"));
        assert_eq!(date.len(), 10);
    }

    #[test]
    fn the_brain_is_left_untouched() {
        let (brain, cache) = setup(&[("a.md", "alpha")]);
        let before = fs::read_dir(brain.path()).unwrap().count();
        run(&brain, &cache);
        assert_eq!(fs::read_dir(brain.path()).unwrap().count(), before);
    }
}
