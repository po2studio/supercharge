//! `brain bench`: how fast a brain of a given size is, on this machine. A synthetic brain of N
//! chunks (random vectors, text drawn like prose, so nothing is embedded to build it) is stored
//! the way `index` stores one, and searched; indexing speed is measured on real documents with the
//! real model, because the model is what bounds it. `write_brain` writes a synthetic brain of
//! documents instead, for timing a first index at the size of a company's folder.

use crate::{
    adr::Status,
    embed::{self, Embedder},
    eval::p50,
    index,
    search::{self, Mode, Opened, Query},
    sources::Source,
    store::{self, NewChunk, NewFile, Store},
};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};

/// Chunks in a synthetic file.
const PER_FILE: usize = 8;
/// Chunks stored per transaction.
const BATCH: usize = 4000;
/// Words in the synthetic vocabulary: a few common ones, then a long tail, drawn as prose is.
const VOCABULARY: usize = 50_000;
/// Words in a stored synthetic chunk, which is as long as a chunk of about 400 tokens is.
const CHUNK_WORDS: usize = 200;
/// Words in a sample document: one chunk of real-looking text, so the model sees what it sees.
const SAMPLE_WORDS: usize = 230;

#[rustfmt::skip]
const COMMON: &[&str] = &[
    "the", "of", "and", "to", "in", "is", "that", "for", "it", "with", "as", "was", "on", "are",
    "by", "this", "be", "or", "from", "at", "an", "have", "not", "we", "can", "all", "will",
    "customer", "invoice", "policy", "team", "project", "meeting", "budget", "report", "decision",
    "process", "review", "approval", "contract", "supplier", "delivery", "payment", "account",
    "service", "support", "request", "change", "release", "feature", "design", "product", "market",
    "quarter", "plan", "risk", "audit", "security", "access", "travel", "holiday", "expense",
    "office", "laptop", "training", "hiring", "salary", "benefit", "vendor", "order", "stock",
    "price", "discount", "refund", "complaint", "ticket", "incident", "backup", "server", "network",
    "database", "deploy", "monitor", "alert", "owner", "manager", "director", "engineer", "analyst",
    "legal", "finance", "sales", "marketing", "operations", "quality", "safety", "compliance",
    "regulation", "agreement", "proposal", "schedule", "deadline", "milestone", "priority",
    "status", "update", "summary", "notes", "action", "follow", "question", "answer", "example",
    "standard", "template", "guideline", "exception", "rule", "limit", "threshold", "approved",
    "rejected",
];

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// A word rank, most often a small one.
    fn zipf(&mut self, vocabulary: usize) -> usize {
        ((vocabulary as f64).powf(self.unit()) as usize).clamp(1, vocabulary) - 1
    }

    fn vector(&mut self) -> Vec<f32> {
        let mut v: Vec<f32> = (0..embed::DIM)
            .map(|_| (self.unit() + self.unit() + self.unit() - 1.5) as f32)
            .collect();
        embed::normalize(&mut v);
        v
    }
}

fn word(rank: usize) -> String {
    let base = COMMON[rank % COMMON.len()];
    match rank / COMMON.len() {
        0 => base.to_string(),
        n => format!("{base}{n}"),
    }
}

fn text(rng: &mut Rng, words: usize, vocabulary: usize) -> String {
    let mut out = String::with_capacity(words * 8);
    for i in 0..words {
        if i > 0 {
            out.push(if i % 14 == 0 { '\n' } else { ' ' });
        }
        out.push_str(&word(rng.zipf(vocabulary)));
    }
    out
}

#[derive(Debug, Serialize)]
pub struct Results {
    pub chunks: usize,
    pub files: usize,
    /// Seconds to store the synthetic chunks (nothing is embedded).
    pub fill_seconds: f64,
    pub index_bytes: u64,
    /// The real model and the whole pipeline, on `index_sample_chunks` chunks of real-looking text.
    pub index_chunks_per_s: f64,
    pub index_sample_chunks: usize,
    /// In one process, after a first query: both halves together, then each alone.
    pub search_p50_ms: f64,
    pub search_full_text_p50_ms: f64,
    pub search_meaning_p50_ms: f64,
    /// Of `search_meaning_p50_ms`, what embedding the question takes: the rest is the vector scan.
    pub query_embed_p50_ms: f64,
    pub threads: usize,
}

pub struct Opts {
    pub chunks: usize,
    pub embed_sample: usize,
    pub queries: usize,
    pub dir: PathBuf,
    pub keep: bool,
    pub exact_limit: usize,
}

/// Removes a folder when dropped, unless it is to be kept.
struct Scratch {
    dir: PathBuf,
    keep: bool,
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }
}

pub fn run(opts: &Opts, embedder: &mut dyn Embedder) -> Result<Results> {
    if opts.chunks == 0 {
        bail!("--chunks must be at least 1");
    }
    // Absolute and without symlinks: the command line the bench starts names the brain by its
    // canonical path, and an index is found by that name.
    fs::create_dir_all(&opts.dir)?;
    let work = fs::canonicalize(&opts.dir)?.join(format!("bench-{}", std::process::id()));
    fs::create_dir_all(&work)?;
    let _scratch = Scratch {
        dir: work.clone(),
        keep: opts.keep,
    };

    // The synthetic brain, stored as a source's index, so the command line can search it too.
    let root = work.join("brain");
    fs::create_dir_all(&root)?;
    let source = Source::outside("bench", &root);
    let index_path = store::index_path(&source.key())?;
    let _index = Scratch {
        dir: index_path
            .parent()
            .context("no index folder")?
            .to_path_buf(),
        keep: opts.keep,
    };
    let mut store = Store::create(&index_path, &root)?;
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let started = Instant::now();
    let files = opts.chunks.div_ceil(PER_FILE);
    let (mut stored, mut batch) = (0, Vec::new());
    for f in 0..files {
        let path = format!("area-{:03}/doc-{f:07}.md", f % 500);
        let title = format!("Document {f}");
        let n = PER_FILE.min(opts.chunks - stored);
        let chunks = (0..n)
            .map(|c| {
                let heading = format!("Section {c}");
                NewChunk {
                    header: store::header(&title, &heading, &path, "2026-01-01"),
                    heading,
                    text: text(&mut rng, CHUNK_WORDS, VOCABULARY),
                    vector: rng.vector(),
                }
            })
            .collect::<Vec<_>>();
        stored += n;
        batch.push(NewFile {
            hash: format!("{f:064x}"),
            path,
            title,
            date: "2026-01-01".into(),
            own_date: None,
            kind: "raw".into(),
            meta: Status::default(),
            simhash: None,
            chunks,
        });
        if batch.iter().map(|b| b.chunks.len()).sum::<usize>() >= BATCH || stored == opts.chunks {
            store.put_files(&batch)?;
            batch.clear();
        }
    }
    store.checkpoint()?;
    let fill_seconds = started.elapsed().as_secs_f64();
    let index_bytes = fs::metadata(&index_path)?.len();

    // Searches over it, in this process: a question of common words and two rare ones.
    let mut queries: Vec<String> = Vec::new();
    for _ in 0..opts.queries.max(1) {
        queries.push(format!(
            "{} {} {}",
            text(&mut rng, 9, 400),
            word(30 + rng.zipf(VOCABULARY - 30)),
            word(30 + rng.zipf(VOCABULARY - 30))
        ));
    }
    let opened = vec![Opened::new(&source, store)];
    let time = |embedder: &mut dyn Embedder, mode: Mode| -> Result<f64> {
        let mut ms = Vec::new();
        for q in &queries {
            let t = Instant::now();
            let query = Query {
                question: q,
                k: 10,
                mode,
            };
            search::search(&opened, embedder, &query, opts.exact_limit)?;
            ms.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        Ok(p50(ms))
    };
    // The first query pays for pages not yet read; a person's second search pays for it too, so
    // it is left out here.
    search::search(
        &opened,
        embedder,
        &Query {
            question: "the",
            k: 10,
            mode: Mode::Both,
        },
        opts.exact_limit,
    )?;
    let both = time(embedder, Mode::Both)?;
    let full_text = time(embedder, Mode::FullText)?;
    let meaning = time(embedder, Mode::Meaning)?;
    let mut embed_ms = Vec::new();
    for q in &queries {
        let t = Instant::now();
        embedder.embed_query(q)?;
        embed_ms.push(t.elapsed().as_secs_f64() * 1000.0);
    }

    let (sample_chunks, per_s) = indexing_speed(&work, opts.embed_sample, embedder)?;
    Ok(Results {
        chunks: opts.chunks,
        files,
        fill_seconds,
        index_bytes,
        index_chunks_per_s: per_s,
        index_sample_chunks: sample_chunks,
        search_p50_ms: both,
        search_full_text_p50_ms: full_text,
        search_meaning_p50_ms: meaning,
        query_embed_p50_ms: p50(embed_ms),
        threads: std::thread::available_parallelism().map_or(1, |n| n.get()),
    })
}

/// Writes `chunks` worth of real-looking documents, indexes them with the model as `index` does,
/// and returns `(chunks, chunks per second)`.
fn indexing_speed(work: &Path, chunks: usize, embedder: &mut dyn Embedder) -> Result<(usize, f64)> {
    let dir = work.join("sample");
    write_docs(&dir, chunks.max(1), 7)?;
    let source = Source::outside("sample", &dir);
    let mut store = Store::create(&work.join("sample-index.sqlite"), &dir)?;
    // A first batch is not counted: the model's first run sets up its threads.
    let warm = work.join("warm");
    write_docs(&warm, 8, 99)?;
    let warm_source = Source::outside("warm", &warm);
    let mut warm_store = Store::create(&work.join("warm-index.sqlite"), &warm)?;
    index::index(&warm_source, &mut warm_store, embedder, false)?;
    let report = index::index(&source, &mut store, embedder, false)?;
    Ok((
        report.embedded_chunks,
        report.embedded_chunks as f64 / report.seconds.max(1e-9),
    ))
}

/// `count` documents of one chunk of prose each, distinct from each other.
fn write_docs(dir: &Path, count: usize, seed: u64) -> Result<()> {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    for n in 0..count {
        let folder = dir.join(format!("area-{:03}", n / 100));
        fs::create_dir_all(&folder)?;
        let body = text(&mut rng, SAMPLE_WORDS, 3000);
        fs::write(
            folder.join(format!("doc-{n:07}.md")),
            format!("# Document {n}\n\n{body}\n"),
        )?;
    }
    Ok(())
}

/// A synthetic brain repo of `docs` documents in `dir`: a git repo whose origin is on GitHub and
/// that has the CI workflow, so it counts as a brain repo, with one commit.
pub fn write_brain(dir: &Path, docs: usize) -> Result<()> {
    if dir.exists() && fs::read_dir(dir)?.next().is_some() {
        bail!("{} is not empty", dir.display());
    }
    write_docs(dir, docs, 1)?;
    fs::create_dir_all(dir.join(".github/workflows"))?;
    fs::write(dir.join(".github/workflows/brain.yml"), "name: brain\n")?;
    let git = |args: &[&str]| -> Result<()> {
        let out = Command::new("git")
            .current_dir(dir)
            .args([
                "-c",
                "user.name=bench",
                "-c",
                "user.email=bench@example.com",
            ])
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .output()
            .context("cannot run git")?;
        if !out.status.success() {
            bail!(
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(())
    };
    git(&["init", "-q"])?;
    git(&[
        "remote",
        "add",
        "origin",
        "https://github.com/bench/brain.git",
    ])?;
    git(&["add", "-A"])?;
    git(&["commit", "-qm", "synthetic brain"])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_are_drawn_like_prose_most_often_a_common_one() {
        let mut rng = Rng(42);
        let ranks: Vec<usize> = (0..10_000).map(|_| rng.zipf(VOCABULARY)).collect();
        assert!(ranks.iter().all(|&r| r < VOCABULARY));
        let head = ranks.iter().filter(|&&r| r < 100).count();
        assert!(head > 3000 && head < 7000, "{head}");
        assert_eq!(word(0), "the");
        assert_eq!(word(COMMON.len()), "the1");
    }

    #[test]
    fn a_vector_has_unit_length() {
        let v = Rng(7).vector();
        assert_eq!(v.len(), embed::DIM);
        assert!((v.iter().map(|x| x * x).sum::<f32>() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn a_written_brain_is_a_repo_with_one_commit_and_the_workflow() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("brain");
        write_brain(&root, 250).unwrap();
        assert!(root.join(".github/workflows/brain.yml").is_file());
        assert!(root.join("area-002/doc-0000249.md").is_file());
        let log = Command::new("git")
            .args(["log", "--oneline"])
            .current_dir(&root)
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&log.stdout).lines().count(), 1);
        assert!(write_brain(&root, 1).is_err());
    }
}
