//! Walks a source, and embeds what changed since the last run. Reads files; never writes to them.

use anyhow::{Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};

use crate::{
    chunk::{self, Params},
    doc,
    embed::Embedder,
    sources::{Kind, Source, PROJECT_FILES},
    store::{self, NewChunk, NewFile, Store},
};

/// Chunks embedded together; also how much work a crash can lose.
const GROUP: usize = 128;

/// Larger text files are reported, not indexed: they are logs and dumps, and the time to chunk
/// and embed them grows with their size.
pub const MAX_FILE_BYTES: u64 = 1 << 20;

/// The most tokens the context header may take in what is embedded. With the 400-token chunks
/// and the few tokens the model adds, header and text then always fit its 512.
const HEADER_TOKENS: usize = 96;
/// A header is cut to this many characters before it is measured.
const HEADER_CHARS: usize = 1000;

#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub files: usize,
    pub chunks: usize,
    pub files_changed: usize,
    pub files_removed: usize,
    pub embedded_chunks: usize,
    /// Unchanged files whose date moved (a new commit date, say): the date is refreshed without
    /// embedding again.
    pub dates_refreshed: usize,
    /// Files with an extension no converter exists for yet, by extension.
    pub not_indexed: BTreeMap<String, usize>,
    /// Text files left out, each with the reason: too large, or unreadable.
    pub skipped: Vec<String>,
    pub seconds: f64,
}

impl Report {
    /// Adds another source's run to this total; `skipped` entries say which source they are from.
    pub fn add(&mut self, label: &str, other: &Report) {
        self.files += other.files;
        self.chunks += other.chunks;
        self.files_changed += other.files_changed;
        self.files_removed += other.files_removed;
        self.embedded_chunks += other.embedded_chunks;
        self.dates_refreshed += other.dates_refreshed;
        self.seconds += other.seconds;
        for (ext, n) in &other.not_indexed {
            *self.not_indexed.entry(ext.clone()).or_insert(0) += n;
        }
        self.skipped
            .extend(other.skipped.iter().map(|s| format!("{label}: {s}")));
    }
}

/// A text file in the brain: the key it is stored under, and where it is.
#[derive(Debug, PartialEq)]
pub struct Found {
    /// Relative path with `/` separators; a name that isn't UTF-8 is shown lossily.
    pub rel: String,
    pub path: PathBuf,
}

#[derive(Debug, Default)]
pub struct Walk {
    /// Sorted by `rel`.
    pub files: Vec<Found>,
    /// Files no converter exists for, counted by extension.
    pub other: BTreeMap<String, usize>,
    pub skipped: Vec<String>,
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

/// Finds the text files of a source, and counts the others by extension: all of an outside
/// source, or a project's docs folder, `README.md` and `AGENTS.md`. Hidden names are skipped, and
/// so are symlinks to folders and broken ones; a symlink to a file is read as that file.
pub fn walk(source: &Source) -> Result<Walk> {
    let root = source.root.as_path();
    let mut out = Walk::default();
    match &source.kind {
        Kind::Outside => walk_from(root, root, &mut out)?,
        Kind::Project { docs } => {
            let start = root.join(docs);
            if start.is_dir() {
                walk_from(root, &start, &mut out)?;
            }
            for name in PROJECT_FILES {
                let path = root.join(name);
                if fs::metadata(&path).is_ok_and(|m| m.is_file())
                    && out.files.iter().all(|f| f.rel != name)
                {
                    out.files.push(Found {
                        rel: name.to_string(),
                        path,
                    });
                }
            }
        }
    }
    out.files.sort_by(|a, b| a.rel.cmp(&b.rel));
    out.skipped.sort();
    // Two names that differ only in bytes that aren't UTF-8 would share a key.
    let before = out.files.len();
    out.files.dedup_by(|b, a| a.rel == b.rel);
    if out.files.len() < before {
        out.skipped.push(format!(
            "{} file(s) with a name that clashes with another once its non-UTF-8 bytes are replaced",
            before - out.files.len()
        ));
    }
    Ok(out)
}

fn walk_from(root: &Path, start: &Path, out: &mut Walk) -> Result<()> {
    let mut stack = vec![start.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
            let entry = entry?;
            let name = entry.file_name();
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            let path = entry.path();
            let kind = entry.file_type()?;
            let (is_dir, meta) = if kind.is_symlink() {
                match fs::metadata(&path) {
                    Ok(m) if m.is_file() => (false, Some(m)),
                    _ => continue,
                }
            } else if kind.is_dir() {
                (true, None)
            } else if kind.is_file() {
                (false, Some(entry.metadata()?))
            } else {
                continue;
            };
            if is_dir {
                stack.push(path);
                continue;
            }
            let rel = path
                .strip_prefix(root)?
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            if !is_text(&path) {
                let ext = path
                    .extension()
                    .map_or("(none)".to_string(), |e| e.to_string_lossy().to_lowercase());
                *out.other.entry(ext).or_insert(0) += 1;
            } else if meta.is_some_and(|m| m.len() > MAX_FILE_BYTES) {
                out.skipped
                    .push(format!("{rel} (over {} MiB)", MAX_FILE_BYTES >> 20));
            } else {
                out.files.push(Found { rel, path });
            }
        }
    }
    Ok(())
}

/// Last commit date of each file, when the source is in a git repo. Read-only. A project looks
/// only at the history of the paths it reads, not at the whole repo's.
fn git_dates(source: &Source) -> HashMap<String, String> {
    let mut dates = HashMap::new();
    let mut args: Vec<String> = [
        "-c",
        "core.quotepath=off",
        "log",
        "--relative",
        "--format=%x00%cs",
        "--name-only",
    ]
    .map(String::from)
    .into();
    if let Kind::Project { docs } = &source.kind {
        args.push("--".into());
        args.push(if docs.is_empty() {
            ".".into()
        } else {
            docs.clone()
        });
        args.extend(PROJECT_FILES.map(String::from));
    }
    let Ok(out) = Command::new("git")
        .args(&args)
        .current_dir(&source.root)
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

/// What is embedded for a chunk: its context header (cut to `HEADER_TOKENS`), then its text.
fn embedded_text(header: &str, text: &str, count: &dyn Fn(&str) -> usize) -> String {
    let mut head: String = header.chars().take(HEADER_CHARS).collect();
    while count(&head) > HEADER_TOKENS && !head.is_empty() {
        let keep = head.chars().count() * 9 / 10;
        head = head.chars().take(keep).collect();
    }
    format!("{head}\n{text}")
}

pub fn index(
    source: &Source,
    store: &mut Store,
    embedder: &mut dyn Embedder,
    progress: bool,
) -> Result<Report> {
    let started = Instant::now();
    let Walk {
        files,
        other: not_indexed,
        mut skipped,
    } = walk(source)?;
    let known = store.files()?;
    let on_disk: BTreeSet<&str> = files.iter().map(|f| f.rel.as_str()).collect();
    let mut report = Report {
        files: files.len(),
        not_indexed,
        ..Default::default()
    };

    for path in known.keys().filter(|p| !on_disk.contains(p.as_str())) {
        store.remove_file(path)?;
        report.files_removed += 1;
    }

    let git = git_dates(source);
    let params = Params::default();
    let mut pending: Vec<Pending> = Vec::new();
    let mut queued = 0;
    for (n, Found { rel, path: full }) in files.iter().enumerate() {
        let bytes = match fs::read(full) {
            Ok(b) => b,
            Err(e) => {
                // Removed meanwhile, or not ours to read: leave it out, and the rest is indexed.
                report.files -= 1;
                skipped.push(format!("{rel} (cannot read: {e})"));
                if known.contains_key(rel) {
                    store.remove_file(rel)?;
                    report.files_removed += 1;
                }
                continue;
            }
        };
        let hash = format!("{:x}", Sha256::digest(&bytes));
        let text = String::from_utf8_lossy(&bytes);
        let parsed = doc::parse(&text);
        let date = parsed
            .date
            .clone()
            .or_else(|| git.get(rel).cloned())
            .unwrap_or_else(|| {
                doc::date_of(
                    fs::metadata(full)
                        .and_then(|m| m.modified())
                        .unwrap_or(std::time::UNIX_EPOCH),
                )
            });
        if let Some((known_hash, known_date)) = known.get(rel) {
            if *known_hash == hash {
                if *known_date != date {
                    store.refresh_date(rel, &date)?;
                    report.dates_refreshed += 1;
                }
                continue;
            }
        }
        let title = parsed.title.clone().unwrap_or_else(|| {
            Path::new(rel)
                .file_stem()
                .map_or(rel.clone(), |s| s.to_string_lossy().into_owned())
        });
        let count = |s: &str| embedder.count_tokens(s);
        let chunks = chunk::chunk(parsed.body, &count, &params);
        let mut texts = Vec::new();
        let new_chunks = chunks
            .into_iter()
            .map(|c| {
                let header = store::header(&title, &c.heading, rel, &date);
                texts.push(embedded_text(&header, &c.text, &count));
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
                kind: if source.is_project() {
                    "raw"
                } else {
                    doc::kind_of(rel)
                }
                .to_string(),
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

    skipped.sort();
    report.skipped = skipped;
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
        index(
            &Source::outside("t", brain.path()),
            &mut store,
            &mut Fake,
            false,
        )
        .unwrap()
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
        let Walk { files, other, .. } = walk(&Source::outside("t", brain.path())).unwrap();
        let files: Vec<&str> = files.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(files, ["a.md", "sub/b.TXT", "sub/deep/c.markdown"]);
        assert_eq!(other["pdf"], 2);
        assert_eq!(other["(none)"], 1);
    }

    fn project(brain: &TempDir, docs: &str) -> Source {
        Source {
            label: crate::sources::PROJECT.into(),
            kind: Kind::Project { docs: docs.into() },
            root: brain.path().to_path_buf(),
            exists: true,
            update: false,
        }
    }

    #[test]
    fn a_project_reads_its_docs_folder_readme_and_agents_md_and_nothing_else() {
        let (dir, cache) = setup(&[
            ("README.md", "# Readme"),
            ("AGENTS.md", "# Agents"),
            ("CHANGELOG.md", "# Not read"),
            ("docs/PRODUCT.md", "# Product"),
            ("docs/adr/0001-x.md", "# ADR"),
            ("docs/diagram.png", "x"),
            ("src/notes.md", "# Not read"),
            ("handbook/guide.md", "# Not read by default"),
        ]);
        let rels = |source: &Source| -> Vec<String> {
            walk(source)
                .unwrap()
                .files
                .into_iter()
                .map(|f| f.rel)
                .collect()
        };
        assert_eq!(
            rels(&project(&dir, "docs")),
            [
                "AGENTS.md",
                "README.md",
                "docs/PRODUCT.md",
                "docs/adr/0001-x.md"
            ]
        );
        assert_eq!(
            rels(&project(&dir, "handbook")),
            ["AGENTS.md", "README.md", "handbook/guide.md"]
        );
        assert_eq!(walk(&project(&dir, "docs")).unwrap().other["png"], 1);
        // The root as the docs folder reads each file once.
        assert_eq!(
            rels(&project(&dir, ""))
                .iter()
                .filter(|r| *r == "README.md")
                .count(),
            1
        );
        // Run against a project: files are keyed by their path in it, and the brain's own folders
        // mean nothing there.
        let source = project(&dir, "docs");
        let mut store = Store::create(&cache.path().join("index.sqlite"), dir.path()).unwrap();
        let r = index(&source, &mut store, &mut Fake, false).unwrap();
        assert_eq!((r.files, r.files_changed), (4, 4));
        assert!(store.files().unwrap().contains_key("docs/adr/0001-x.md"));
        let kind: String = store
            .conn
            .query_row("SELECT DISTINCT kind FROM files", [], |r| r.get(0))
            .unwrap();
        assert_eq!(kind, "raw");
    }

    #[test]
    fn a_project_without_docs_or_readme_has_nothing_to_read() {
        let (dir, _) = setup(&[("src/main.md", "x")]);
        assert!(walk(&project(&dir, "docs")).unwrap().files.is_empty());
    }

    #[test]
    fn a_projects_dates_come_from_the_history_of_the_paths_it_reads() {
        let (dir, cache) = setup(&[("README.md", "# R"), ("src/other.md", "x")]);
        git(dir.path(), &[], &["init", "-q"]);
        git(dir.path(), &[], &["add", "."]);
        let old = [("GIT_COMMITTER_DATE", "2021-03-04T12:00:00Z")];
        git(dir.path(), &old, &["commit", "-q", "-m", "one"]);
        let mut store = Store::create(&cache.path().join("index.sqlite"), dir.path()).unwrap();
        index(&project(&dir, "docs"), &mut store, &mut Fake, false).unwrap();
        assert_eq!(store.files().unwrap()["README.md"].1, "2021-03-04");
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

    /// The embedded texts of a run, as the embedder saw them.
    struct Spy(Vec<String>);

    impl Embedder for Spy {
        fn embed_passages(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
            self.0.extend(texts.iter().map(|t| t.to_string()));
            Fake.embed_passages(texts)
        }
        fn embed_query(&mut self, text: &str) -> Result<Vec<f32>> {
            Fake.embed_query(text)
        }
        fn count_tokens(&self, text: &str) -> usize {
            Fake.count_tokens(text)
        }
    }

    #[test]
    fn a_long_header_never_pushes_chunk_text_past_the_models_limit() {
        let words = |tag: &str, n: usize| {
            (0..n)
                .map(|i| format!("{tag}{i}"))
                .collect::<Vec<_>>()
                .join(" ")
        };
        // Six headings of forty words each, and a title as long as a title may be.
        let mut doc = format!("# {}\n\n", words("a", 40));
        for (level, tag) in ["##", "###", "####", "#####", "######"]
            .iter()
            .zip("bcdef".chars())
        {
            doc.push_str(&format!("{level} {}\n\n", words(&tag.to_string(), 40)));
        }
        doc.push_str(&words("body", 1500));
        let (brain, cache) = setup(&[("deep/ly/nested/folder/file.md", &doc)]);
        let mut store = Store::create(&cache.path().join("index.sqlite"), brain.path()).unwrap();
        let mut spy = Spy(Vec::new());
        index(
            &Source::outside("t", brain.path()),
            &mut store,
            &mut spy,
            false,
        )
        .unwrap();
        assert!(spy.0.len() >= 3);
        for t in &spy.0 {
            assert!(
                Fake.count_tokens(t) <= crate::embed::MAX_TOKENS - 10,
                "{} tokens",
                Fake.count_tokens(t)
            );
        }
        // Nothing of the body is lost to the cut: every body word is embedded somewhere.
        let all = spy.0.join(" ");
        for i in 0..1500 {
            assert!(all.contains(&format!("body{i} ")) || all.ends_with(&format!("body{i}")));
        }
    }

    #[test]
    fn a_date_that_moves_is_refreshed_without_embedding_again() {
        let (brain, cache) = setup(&[("a.md", "# A\n\nsome text here")]);
        run(&brain, &cache);
        let when =
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_950_000_000);
        fs::File::options()
            .write(true)
            .open(brain.path().join("a.md"))
            .unwrap()
            .set_modified(when)
            .unwrap();
        let r = run(&brain, &cache);
        assert_eq!(
            (r.embedded_chunks, r.files_changed, r.dates_refreshed),
            (0, 0, 1)
        );
        let store = Store::open(&cache.path().join("index.sqlite"))
            .unwrap()
            .unwrap();
        assert_eq!(store.files().unwrap()["a.md"].1, doc::date_of(when));
        assert_eq!(
            run(&brain, &cache).dates_refreshed,
            0,
            "and then it is stable"
        );
    }

    fn git(dir: &Path, envs: &[(&str, &str)], args: &[&str]) {
        let out = Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .envs(envs.iter().copied())
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn a_new_commit_date_refreshes_the_date_in_a_git_brain() {
        let (brain, cache) = setup(&[("a.md", "# A\n\nsome text here")]);
        git(brain.path(), &[], &["init", "-q"]);
        git(brain.path(), &[], &["add", "."]);
        let old = [("GIT_COMMITTER_DATE", "2020-01-02T12:00:00Z")];
        git(brain.path(), &old, &["commit", "-q", "-m", "one"]);
        run(&brain, &cache);
        let date = |cache: &TempDir| {
            let store = Store::open(&cache.path().join("index.sqlite"))
                .unwrap()
                .unwrap();
            store.files().unwrap()["a.md"].1.clone()
        };
        assert_eq!(date(&cache), "2020-01-02");
        let new = [("GIT_COMMITTER_DATE", "2024-06-07T12:00:00Z")];
        git(
            brain.path(),
            &new,
            &["commit", "-q", "--amend", "--no-edit"],
        );
        let r = run(&brain, &cache);
        assert_eq!((r.embedded_chunks, r.dates_refreshed), (0, 1));
        assert_eq!(date(&cache), "2024-06-07");
    }

    #[test]
    fn symlinks_to_files_are_read_and_to_folders_or_nowhere_are_left_alone() {
        let (brain, cache) = setup(&[("real/a.md", "alpha"), ("elsewhere/loop.md", "x")]);
        let root = brain.path();
        std::os::unix::fs::symlink(root.join("real/a.md"), root.join("link.md")).unwrap();
        std::os::unix::fs::symlink(root.join("real"), root.join("dirlink")).unwrap();
        std::os::unix::fs::symlink(root, root.join("real/up")).unwrap();
        std::os::unix::fs::symlink(root.join("gone.md"), root.join("broken.md")).unwrap();
        let r = run(&brain, &cache);
        let rels: Vec<String> = walk(&Source::outside("t", root))
            .unwrap()
            .files
            .into_iter()
            .map(|f| f.rel)
            .collect();
        assert_eq!(rels, ["elsewhere/loop.md", "link.md", "real/a.md"]);
        assert_eq!(r.files, 3);
        assert!(r.skipped.is_empty());
    }

    #[test]
    fn a_file_name_that_is_not_utf8_is_indexed_and_does_not_stop_the_run() {
        use std::os::unix::ffi::OsStrExt;
        let (brain, cache) = setup(&[("fine.md", "good file")]);
        let odd = std::ffi::OsStr::from_bytes(b"caf\xe9 notes.md");
        fs::write(brain.path().join(odd), "the odd one").unwrap();
        let r = run(&brain, &cache);
        assert_eq!((r.files, r.files_changed, r.chunks), (2, 2, 2));
        assert!(r.skipped.is_empty(), "{:?}", r.skipped);
        assert_eq!(run(&brain, &cache).files_changed, 0, "and it is stable");
    }

    #[test]
    fn a_text_file_over_the_size_cap_is_reported_not_read() {
        let (brain, cache) = setup(&[("small.md", "small")]);
        let big = brain.path().join("dump.txt");
        let f = fs::File::create(&big).unwrap();
        f.set_len(MAX_FILE_BYTES + 1).unwrap();
        let r = run(&brain, &cache);
        assert_eq!((r.files, r.chunks), (1, 1));
        assert_eq!(r.skipped, ["dump.txt (over 1 MiB)"]);
    }

    #[test]
    fn a_file_that_cannot_be_read_is_reported_and_the_rest_is_indexed() {
        use std::os::unix::fs::PermissionsExt;
        let (brain, cache) = setup(&[("ok.md", "fine"), ("locked.md", "secret")]);
        let locked = brain.path().join("locked.md");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read(&locked).is_ok() {
            return; // running as root: permissions don't apply
        }
        let r = run(&brain, &cache);
        assert_eq!((r.files, r.chunks), (1, 1));
        assert_eq!(r.skipped.len(), 1);
        assert!(
            r.skipped[0].starts_with("locked.md (cannot read"),
            "{:?}",
            r.skipped
        );
        // Indexed before it became unreadable: it leaves the index, and the run says so.
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o644)).unwrap();
        let r = run(&brain, &cache);
        assert_eq!((r.files, r.chunks, r.files_removed), (2, 2, 0));
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let r = run(&brain, &cache);
        assert_eq!((r.files, r.chunks, r.files_removed), (1, 1, 1));
    }

    #[test]
    fn a_text_that_is_not_utf8_or_has_odd_metadata_still_indexes() {
        let (brain, cache) = setup(&[
            ("empty.md", ""),
            ("crlf.md", "# T\r\n\r\ntext with windows endings\r\n"),
            (
                "badmeta.md",
                "---\ntitle:\ndate: abcdéxxxx\nbroken\n---\nbody",
            ),
        ]);
        fs::write(brain.path().join("latin1.txt"), b"caf\xe9 au lait \xff\xfe").unwrap();
        let r = run(&brain, &cache);
        assert_eq!(r.files, 4);
        assert!(r.chunks >= 3);
    }
}
