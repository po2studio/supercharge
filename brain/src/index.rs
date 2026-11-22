//! Walks a source, and embeds what changed since the last run. Reads files; never writes to them.

use anyhow::{Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};

use crate::{
    chunk::{self, Params},
    convert::{self, Format, Outcome},
    decisions, dedup, doc,
    embed::Embedder,
    notes,
    sources::{Kind, Source, PROJECT_FILES},
    store::{self, FileRow, NewChunk, NewFile, Store},
};

/// Chunks embedded together; also how much work a crash can lose.
const GROUP: usize = 128;

/// The most tokens the context header may take in what is embedded. With the 400-token chunks
/// and the few tokens the model adds, header and text then always fit its 512.
const HEADER_TOKENS: usize = 96;
/// A header is cut to this many characters before it is measured.
const HEADER_CHARS: usize = 1000;

/// A file with no text to index, and why.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Unconverted {
    /// The source's label, in the total of several sources' runs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Default, Serialize)]
pub struct Report {
    /// Files with text in the index.
    pub files: usize,
    pub chunks: usize,
    pub files_changed: usize,
    pub files_removed: usize,
    pub embedded_chunks: usize,
    /// Files converted to text in this run: the files that changed, less those whose conversion
    /// was cached.
    pub converted: usize,
    /// Unchanged files whose date moved (a new commit date, say): the date is refreshed without
    /// embedding again.
    pub dates_refreshed: usize,
    /// Files in the index that are a copy of a newer file, so only the newest ranks.
    pub duplicates: usize,
    /// Files that cannot be turned into text: scans, images, audio, video, encrypted and corrupt
    /// files, and formats no converter reads.
    pub unconverted: Vec<Unconverted>,
    /// Files left out for being too large, or unreadable, each with the reason.
    pub skipped: Vec<String>,
    pub seconds: f64,
    /// A time budget ran out before every file was looked at: those not yet indexed are missing,
    /// not wrong, and the next run carries on.
    pub incomplete: bool,
}

impl Report {
    /// Adds another source's run to this total; entries say which source they are from.
    pub fn add(&mut self, label: &str, other: &Report) {
        self.files += other.files;
        self.chunks += other.chunks;
        self.files_changed += other.files_changed;
        self.files_removed += other.files_removed;
        self.embedded_chunks += other.embedded_chunks;
        self.converted += other.converted;
        self.dates_refreshed += other.dates_refreshed;
        self.duplicates += other.duplicates;
        self.seconds += other.seconds;
        self.incomplete |= other.incomplete;
        self.unconverted
            .extend(other.unconverted.iter().map(|u| Unconverted {
                source: Some(label.to_string()),
                ..u.clone()
            }));
        self.skipped
            .extend(other.skipped.iter().map(|s| format!("{label}: {s}")));
    }
}

/// A file in a source: the key it is stored under, where it is, and what it is.
#[derive(Debug, PartialEq)]
pub struct Found {
    /// Relative path with `/` separators; a name that isn't UTF-8 is shown lossily.
    pub rel: String,
    pub path: PathBuf,
    pub format: Format,
}

#[derive(Debug, Default)]
pub struct Walk {
    /// Sorted by `rel`.
    pub files: Vec<Found>,
    /// Files no converter can ever read, by what they are (images, audio, video, archives).
    pub unconverted: Vec<Unconverted>,
    pub skipped: Vec<String>,
}

fn skipped_note(rel: &str, format: Format) -> String {
    format!("{rel} (over {} MiB)", format.max_bytes() >> 20)
}

/// Finds the files of a source: all of an outside source, or a project's docs folder,
/// `README.md` and `AGENTS.md`. Hidden names are skipped, and so are symlinks to folders and
/// broken ones; a symlink to a file is read as that file. Files that are too large are left out
/// unread, and those no converter can read are reported.
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
                let Ok(meta) = fs::metadata(&path) else {
                    continue;
                };
                if !meta.is_file() || out.files.iter().any(|f| f.rel == name) {
                    continue;
                }
                let format = Format::of(&path);
                if meta.len() > format.max_bytes() {
                    let note = skipped_note(name, format);
                    if !out.skipped.contains(&note) {
                        out.skipped.push(note);
                    }
                } else {
                    out.files.push(Found {
                        rel: name.to_string(),
                        path,
                        format,
                    });
                }
            }
        }
    }
    out.files.sort_by(|a, b| a.rel.cmp(&b.rel));
    out.unconverted.sort_by(|a, b| a.path.cmp(&b.path));
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
            let format = Format::of(&path);
            if let Format::Unconvertible(reason) = format {
                out.unconverted.push(Unconverted {
                    source: None,
                    path: rel,
                    reason: reason.to_string(),
                });
            } else if meta.is_some_and(|m| m.len() > format.max_bytes()) {
                out.skipped.push(skipped_note(&rel, format));
            } else {
                out.files.push(Found { rel, path, format });
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
    index_until(source, store, embedder, progress, None)
}

/// `index`, but once `deadline` has passed it stops after the group of chunks it is on: a brain
/// too big for one run is then built over several, each carrying on from the index the last left.
pub fn index_until(
    source: &Source,
    store: &mut Store,
    embedder: &mut dyn Embedder,
    progress: bool,
    deadline: Option<Instant>,
) -> Result<Report> {
    let started = Instant::now();
    let Walk {
        files,
        mut unconverted,
        mut skipped,
    } = walk(source)?;
    let known = store.files()?;
    let known_unconverted = store.unconverted()?;
    let on_disk: BTreeSet<&str> = files.iter().map(|f| f.rel.as_str()).collect();
    let mut report = Report::default();

    let gone = known
        .keys()
        .chain(known_unconverted.keys())
        .filter(|p| !on_disk.contains(p.as_str()));
    for path in gone {
        store.remove_file(path)?;
        report.files_removed += 1;
    }

    let git = git_dates(source);
    let params = Params::default();
    let cache = store.dir.join("convert");
    let mut cached_keys: HashSet<String> = HashSet::new();
    let mut pending: Vec<Pending> = Vec::new();
    let mut queued = 0;
    for (
        n,
        Found {
            rel,
            path: full,
            format,
        },
    ) in files.iter().enumerate()
    {
        let bytes = match fs::read(full) {
            Ok(b) => b,
            Err(e) => {
                // Removed meanwhile, or not ours to read: leave it out, and the rest is indexed.
                skipped.push(format!("{rel} (cannot read: {e})"));
                if known.contains_key(rel) || known_unconverted.contains_key(rel) {
                    store.remove_file(rel)?;
                    report.files_removed += 1;
                }
                continue;
            }
        };
        let hash = format!("{:x}", Sha256::digest(&bytes));
        if let Some((known_hash, reason)) = known_unconverted.get(rel) {
            if *known_hash == hash {
                unconverted.push(Unconverted {
                    source: None,
                    path: rel.clone(),
                    reason: reason.clone(),
                });
                continue;
            }
        }
        // What a conversion is kept under: the file's content, read as the format its name says.
        let cache_key = format!("{hash}-{}", format.name());
        if format.cached() {
            cached_keys.insert(cache_key.clone());
        }
        let fallback_date = |own: Option<&String>| {
            own.cloned()
                .or_else(|| git.get(rel).cloned())
                .unwrap_or_else(|| {
                    doc::date_of(
                        fs::metadata(full)
                            .and_then(|m| m.modified())
                            .unwrap_or(std::time::UNIX_EPOCH),
                    )
                })
        };
        if let Some(FileRow {
            hash: known_hash,
            date: known_date,
            own_date,
        }) = known.get(rel)
        {
            if *known_hash == hash {
                let date = fallback_date(own_date.as_ref());
                if *known_date != date {
                    store.refresh_date(rel, &date)?;
                    report.dates_refreshed += 1;
                }
                continue;
            }
        }
        let mut converted = match format
            .cached()
            .then(|| convert::cache_get(&cache, &cache_key))
        {
            Some(Some(c)) => c,
            _ => {
                report.converted += 1;
                match convert::convert(*format, &bytes) {
                    Outcome::Text(c) => {
                        if format.cached() {
                            convert::cache_put(&cache, &cache_key, &c);
                        }
                        *c
                    }
                    Outcome::Unconverted(reason) => {
                        store.put_unconverted(rel, &hash, &reason)?;
                        if known.contains_key(rel) {
                            report.files_changed += 1;
                        }
                        unconverted.push(Unconverted {
                            source: None,
                            path: rel.clone(),
                            reason,
                        });
                        continue;
                    }
                }
            }
        };
        let (kind, meta) = entry_of(source, rel, &converted);
        if let Some(note) = &meta.note {
            // A note is as old as it was last checked against its sources, and is found by what
            // it says it answers.
            converted.date = note.verified.clone().or(converted.date);
            converted.text = notes::indexed_text(note, &converted.text);
        }
        let date = fallback_date(converted.date.as_ref());
        let title = converted.title.clone().unwrap_or_else(|| {
            Path::new(rel)
                .file_stem()
                .map_or(rel.clone(), |s| s.to_string_lossy().into_owned())
        });
        let count = |s: &str| embedder.count_tokens(s);
        let chunks = chunk::chunk(
            &converted.text,
            &count,
            &Params {
                headings: converted.prose,
                ..params
            },
        );
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
                own_date: converted.date.clone(),
                kind,
                meta,
                simhash: dedup::simhash(&converted.text),
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
            if deadline.is_some_and(|d| Instant::now() >= d) && n + 1 < files.len() {
                report.incomplete = true;
                break;
            }
        }
    }
    report.embedded_chunks += flush(store, embedder, &mut pending)?;
    if store.dir.join("convert").is_dir() {
        convert::cache_prune(&cache, &cached_keys);
    }

    // Of files that are copies of each other only the newest ranks; dates may have moved.
    let copies = dedup::copies(&store.dedup_items()?);
    store.set_copies(&copies)?;
    decisions::resolve(store)?;

    skipped.sort();
    unconverted.sort_by(|a, b| a.path.cmp(&b.path));
    report.skipped = skipped;
    report.unconverted = unconverted;
    (report.files, report.chunks) = {
        let (f, c) = store.counts()?;
        (f as usize, c as usize)
    };
    report.duplicates = store.duplicates()?;
    report.seconds = started.elapsed().as_secs_f64();
    store.set_meta("last_index", &serde_json::to_string(&report)?)?;
    Ok(report)
}

/// What a file is, and what it says about being decided or replaced, or made from. Only the
/// files of `decisions/` and a project's ADRs carry the first, and those of `notes/` the
/// second; a status in any other file is just text.
fn entry_of(source: &Source, rel: &str, converted: &convert::Converted) -> (String, doc::Meta) {
    match &source.kind {
        Kind::Project { docs } if doc::is_adr(rel, docs) => {
            let mut meta = doc::adr_status(&converted.text);
            meta.project = None;
            ("adr".into(), meta)
        }
        Kind::Project { .. } => ("raw".into(), doc::Meta::default()),
        Kind::Outside => {
            let kind = doc::kind_of(rel, &converted.meta);
            let meta = match kind.as_str() {
                "raw" => doc::Meta::default(),
                // A note says what it was made from, not whether it is decided or replaced.
                "note" => doc::Meta {
                    project: converted.meta.project.clone(),
                    note: Some(converted.meta.note.clone().unwrap_or_default()),
                    ..doc::Meta::default()
                },
                _ => converted.meta.clone(),
            };
            (kind, meta)
        }
    }
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
    fn a_time_budget_that_ran_out_stops_after_a_group_and_the_next_run_carries_on() {
        let names: Vec<String> = (0..400).map(|n| format!("doc-{n:03}.md")).collect();
        let files: Vec<(&str, &str)> = names
            .iter()
            .map(|n| (n.as_str(), "# A document\n\nSome words in it.\n"))
            .collect();
        let (brain, cache) = setup(&files);
        let source = Source::outside("t", brain.path());
        let mut store = Store::create(&cache.path().join("index.sqlite"), brain.path()).unwrap();
        let past = Instant::now();
        let first = index_until(&source, &mut store, &mut Fake, false, Some(past)).unwrap();
        assert!(first.incomplete);
        assert_eq!(first.embedded_chunks, GROUP);
        assert_eq!(first.files, GROUP);
        // Without a budget the rest is embedded, and the group already there is not.
        let second = index(&source, &mut store, &mut Fake, false).unwrap();
        assert!(!second.incomplete);
        assert_eq!(second.files, 400);
        assert_eq!(second.embedded_chunks, 400 - GROUP);
    }

    #[test]
    fn walk_finds_every_file_that_might_hold_text_and_skips_hidden() {
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
        let Walk {
            files, unconverted, ..
        } = walk(&Source::outside("t", brain.path())).unwrap();
        let files: Vec<&str> = files.iter().map(|f| f.rel.as_str()).collect();
        // Every file that might hold text is found, whatever its extension, for the converters.
        assert_eq!(
            files,
            [
                "Makefile",
                "a.md",
                "scan.pdf",
                "scan2.PDF",
                "sub/b.TXT",
                "sub/deep/c.markdown"
            ]
        );
        assert!(unconverted.is_empty());
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
        assert_eq!(walk(&project(&dir, "docs")).unwrap().unconverted.len(), 1);
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
    fn an_oversized_readme_or_agents_md_is_reported_not_read() {
        let (dir, _) = setup(&[("AGENTS.md", "# Agents"), ("docs/a.md", "# A")]);
        let big = fs::File::create(dir.path().join("README.md")).unwrap();
        big.set_len(convert::MAX_TEXT_BYTES + 1).unwrap();
        for docs in ["docs", ""] {
            let w = walk(&project(&dir, docs)).unwrap();
            let rels: Vec<&str> = w.files.iter().map(|f| f.rel.as_str()).collect();
            assert!(!rels.contains(&"README.md"), "docs {docs:?}: {rels:?}");
            assert_eq!(
                w.skipped
                    .iter()
                    .filter(|s| s.starts_with("README.md"))
                    .count(),
                1,
                "docs {docs:?}: {:?}",
                w.skipped
            );
        }
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
        assert_eq!(store.files().unwrap()["README.md"].date, "2021-03-04");
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
    fn only_decisions_and_adrs_carry_a_status_and_a_decisions_kind_comes_from_its_frontmatter() {
        let status = |store: &Store, p: &str| -> (String, String, String) {
            store
                .conn
                .query_row(
                    "SELECT kind, status, superseded_by FROM files WHERE path = ?1",
                    [p],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap()
        };
        let front = "---\nkind: process\nstatus: superseded\nsuperseded-by: later\n---\nx";
        let (brain, cache) = setup(&[
            ("decisions/2024-01-01-a.md", front),
            ("decisions/2024-01-02-b.md", "no frontmatter"),
            ("notes/n.md", front),
            ("policy.md", front),
        ]);
        run(&brain, &cache);
        let store = Store::open(&cache.path().join("index.sqlite"))
            .unwrap()
            .unwrap();
        assert_eq!(
            status(&store, "decisions/2024-01-01-a.md"),
            ("process".into(), "superseded".into(), "later".into())
        );
        assert_eq!(
            status(&store, "decisions/2024-01-02-b.md"),
            ("decision".into(), "".into(), "".into())
        );
        assert_eq!(
            status(&store, "notes/n.md"),
            ("note".into(), "".into(), "".into())
        );
        assert_eq!(
            status(&store, "policy.md"),
            ("raw".into(), "".into(), "".into())
        );

        // A project's ADRs read their status line; its other docs say nothing.
        let (dir, cache) = setup(&[
            (
                "docs/adr/0001-a.md",
                "# 0001\n\nStatus: superseded by 0002\n",
            ),
            ("docs/adr/0002-b.md", "# 0002\n\nStatus: accepted\n"),
            (
                "docs/PRODUCT.md",
                "# Product\n\nStatus: superseded by 0002\n",
            ),
        ]);
        let source = project(&dir, "docs");
        let mut store = Store::create(&cache.path().join("index.sqlite"), dir.path()).unwrap();
        index(&source, &mut store, &mut Fake, false).unwrap();
        assert_eq!(
            status(&store, "docs/adr/0001-a.md"),
            (
                "adr".into(),
                "superseded".into(),
                "docs/adr/0002-b.md".into()
            )
        );
        assert_eq!(status(&store, "docs/adr/0002-b.md").1, "accepted");
        assert_eq!(
            status(&store, "docs/PRODUCT.md"),
            ("raw".into(), "".into(), "".into())
        );
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
        assert_eq!(store.files().unwrap()["a.md"].date, doc::date_of(when));
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
            store.files().unwrap()["a.md"].date.clone()
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
        f.set_len(convert::MAX_TEXT_BYTES + 1).unwrap();
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

    const DOCX: &[u8] = include_bytes!("../eval/corpus/formats/operator-onboarding-checklist.docx");
    const SCAN: &[u8] = include_bytes!("../eval/corpus/formats/scanned-contract.pdf");
    const PNG: &[u8] = include_bytes!("../eval/corpus/formats/office-photo.png");

    fn put(brain: &TempDir, name: &str, bytes: &[u8]) {
        let p = brain.path().join(name);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, bytes).unwrap();
    }

    #[test]
    fn a_converted_file_is_converted_once_and_again_only_when_it_changes() {
        let (brain, cache) = setup(&[("a.md", "# A\n\nplain")]);
        put(&brain, "office/checklist.docx", DOCX);
        let first = run(&brain, &cache);
        assert_eq!((first.files, first.converted), (2, 2));
        let second = run(&brain, &cache);
        assert_eq!(
            (
                second.converted,
                second.files_changed,
                second.embedded_chunks
            ),
            (0, 0, 0)
        );
        // The document's own date is its date, and an unchanged file keeps it.
        let store = Store::open(&cache.path().join("index.sqlite"))
            .unwrap()
            .unwrap();
        let row = &store.files().unwrap()["office/checklist.docx"];
        assert_eq!(row.date, "2024-10-02");
        assert_eq!(row.own_date.as_deref(), Some("2024-10-02"));
        // Rebuilding the index (another model, say) reads the conversion from the cache.
        drop(store);
        fs::remove_file(cache.path().join("index.sqlite")).unwrap();
        let rebuilt = run(&brain, &cache);
        assert_eq!(
            (rebuilt.files, rebuilt.converted),
            (2, 1),
            "only the Markdown is converted again"
        );
        // A change converts the file again, and a file that is gone takes its cached conversion along.
        put(&brain, "office/checklist.docx", &DOCX[..DOCX.len() - 3]);
        assert_eq!(run(&brain, &cache).converted, 1);
        fs::remove_file(brain.path().join("office/checklist.docx")).unwrap();
        run(&brain, &cache);
        let cached: Vec<_> = fs::read_dir(cache.path().join("convert"))
            .unwrap()
            .collect();
        assert!(cached.is_empty(), "{cached:?}");
    }

    #[test]
    fn files_that_cannot_be_converted_are_listed_with_why_and_not_tried_again() {
        let (brain, cache) = setup(&[("a.md", "fine")]);
        put(&brain, "scans/contract.pdf", SCAN);
        put(&brain, "photo.PNG", PNG);
        put(&brain, "broken.docx", b"PK\x03\x04 not a zip at all");
        put(&brain, "blob.dat", &[0, 159, 146, 150, 0, 1, 2]);
        let first = run(&brain, &cache);
        assert_eq!((first.files, first.converted), (1, 4));
        let listed: Vec<(&str, bool)> = first
            .unconverted
            .iter()
            .map(|u| (u.path.as_str(), !u.reason.is_empty()))
            .collect();
        assert_eq!(
            listed,
            [
                ("blob.dat", true),
                ("broken.docx", true),
                ("photo.PNG", true),
                ("scans/contract.pdf", true)
            ]
        );
        let reason = |report: &Report, path: &str| {
            report
                .unconverted
                .iter()
                .find(|u| u.path == path)
                .unwrap()
                .reason
                .clone()
        };
        assert!(reason(&first, "scans/contract.pdf").contains("no text layer"));
        assert!(reason(&first, "photo.PNG").contains("image"));
        let second = run(&brain, &cache);
        assert_eq!(second.converted, 0, "nothing is tried again");
        assert_eq!(
            second.unconverted, first.unconverted,
            "and everything is still listed"
        );
        // A file that becomes readable is indexed, and leaves the list.
        put(&brain, "broken.docx", DOCX);
        let third = run(&brain, &cache);
        assert_eq!((third.files, third.unconverted.len()), (2, 3));
        // And one that is deleted leaves it too.
        fs::remove_file(brain.path().join("blob.dat")).unwrap();
        assert_eq!(run(&brain, &cache).unconverted.len(), 2);
    }

    #[test]
    fn copies_are_found_after_every_run_and_the_newest_stays() {
        let policy = "Staff are entitled to twenty five days of paid leave each calendar year in addition to public holidays. \
            Leave is agreed with the team lead at least two weeks ahead, and no more than ten working days may be taken in a row without \
            approval from the head of department. Unused days carry over until the end of May the following year, after which they expire. \
            Sick leave is recorded separately and does not reduce the annual entitlement.";
        let (brain, cache) = setup(&[
            (
                "leave_v1.md",
                &format!("---\ndate: 2023-01-10\n---\n{policy}"),
            ),
            (
                "leave_final.md",
                &format!(
                    "---\ndate: 2024-02-02\n---\n{}",
                    policy.replace("twenty five", "thirty")
                ),
            ),
            (
                "other.md",
                "Completely unrelated text about the office plants.",
            ),
        ]);
        assert_eq!(run(&brain, &cache).duplicates, 1);
        let store = Store::open(&cache.path().join("index.sqlite"))
            .unwrap()
            .unwrap();
        let hidden: Vec<(String, Option<String>)> = store
            .conn
            .prepare("SELECT path, dup_of FROM files ORDER BY path")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            hidden,
            [
                ("leave_final.md".into(), None),
                ("leave_v1.md".into(), Some("leave_final.md".into())),
                ("other.md".into(), None)
            ]
        );
        drop(store);
        // The older one becomes the newer: the roles swap on the next run, with nothing embedded again.
        fs::write(
            brain.path().join("leave_v1.md"),
            format!("---\ndate: 2025-05-05\n---\n{policy}"),
        )
        .unwrap();
        let r = run(&brain, &cache);
        assert_eq!(r.duplicates, 1);
        let store = Store::open(&cache.path().join("index.sqlite"))
            .unwrap()
            .unwrap();
        let dup_of: Option<String> = store
            .conn
            .query_row(
                "SELECT dup_of FROM files WHERE path = 'leave_final.md'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(dup_of.as_deref(), Some("leave_v1.md"));
        // A copy that is removed leaves nothing hidden.
        drop(store);
        fs::remove_file(brain.path().join("leave_v1.md")).unwrap();
        assert_eq!(run(&brain, &cache).duplicates, 0);
    }

    #[test]
    fn source_code_has_no_headings_and_a_table_is_cut_with_its_header() {
        let mut csv = String::from("id,item\n");
        for i in 0..400 {
            csv.push_str(&format!("{i},widget number {i}\n"));
        }
        let (brain, cache) = setup(&[
            ("script.py", "# not a heading\nprint(1)\n"),
            ("stock.csv", &csv),
        ]);
        run(&brain, &cache);
        let store = Store::open(&cache.path().join("index.sqlite"))
            .unwrap()
            .unwrap();
        let headings: Vec<String> = store
            .conn
            .prepare("SELECT c.heading FROM chunks c JOIN files f ON f.id = c.file_id WHERE f.path = 'script.py'")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(headings, [""]);
        let groups: Vec<String> = store
            .conn
            .prepare("SELECT c.text FROM chunks c JOIN files f ON f.id = c.file_id WHERE f.path = 'stock.csv'")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(groups.len() > 3, "{}", groups.len());
        assert!(groups
            .iter()
            .all(|g| g.starts_with("| id | item |\n|---|---|\n| ")));
    }

    #[test]
    fn a_file_renamed_to_another_format_is_converted_as_that_format_not_from_the_cache() {
        let (brain, cache) = setup(&[("keep.md", "x")]);
        put(&brain, "a.docx", DOCX);
        assert_eq!(run(&brain, &cache).files, 2);
        fs::remove_file(brain.path().join("a.docx")).unwrap();
        put(&brain, "a.pptx", DOCX);
        let r = run(&brain, &cache);
        assert_eq!(r.files, 1);
        assert_eq!(r.unconverted.len(), 1);
        assert!(
            r.unconverted[0].reason.contains("not a PowerPoint"),
            "{:?}",
            r.unconverted
        );
    }

    #[test]
    fn a_decision_is_never_hidden_as_the_copy_of_a_file() {
        let decision = "---\ntitle: Same\nkind: product\nstatus: accepted\n---\nWe keep prices in whole krónur and never in fractions, because every ledger and invoice the company has ever issued is in whole krónur, and nobody could explain a fraction to a customer.";
        let (brain, cache) = setup(&[
            ("decisions/2024-01-01-prices.md", decision),
            ("z-copy.md", decision),
        ]);
        assert_eq!(run(&brain, &cache).duplicates, 0);
    }
}
