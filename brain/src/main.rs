mod adr;
mod bench;
mod chunk;
mod convert;
mod dedup;
mod doc;
mod embed;
mod eval;
mod index;
mod search;
mod shared;
mod sources;
mod store;
mod vecs;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use embed::Embedder;
use search::Opened;
use serde::Serialize;
use sources::{Flags, Resolved, Source};
use std::path::{Path, PathBuf};
use store::Store;

#[derive(Parser)]
#[command(
    version,
    about = "Search a project's docs and the folders it lists as sources, on the CPU, offline: brain search \"<question>\"",
    long_about = "Run inside a project: brain search \"<question>\" brings the index up to date, then returns the best \
matches for the question as asked. It reads the project's docs folder (the `docs` key in AGENTS.md), \
README.md and AGENTS.md, and each folder in the `brain:` list of AGENTS.md's `## Supercharge` block. \
Indexes live under ~/.cache/supercharge/; nothing is written into a project or a source."
)]
struct Cli {
    /// The project's folder; by default the git repo (or the folder with an AGENTS.md) containing the current directory.
    #[arg(long, global = true)]
    repo: Option<PathBuf>,
    /// A folder to read as a source; repeat for several. Replaces the `brain:` list.
    #[arg(long, global = true)]
    source: Vec<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Ask a question in plain words: the index is brought up to date first, then the best matches
    /// by meaning, and by any ID or code in the question, come back, one per file.
    Search {
        /// The question, as you would ask a colleague.
        #[arg(required = true, num_args = 1.., value_name = "QUESTION")]
        question: Vec<String>,
        /// How many files to return.
        #[arg(short, default_value_t = 5)]
        k: usize,
        #[arg(long)]
        json: bool,
    },
    /// Bring the index of each source up to date: embed only the files whose content changed.
    Index {
        #[arg(long)]
        json: bool,
    },
    /// The sources this project reads, what is indexed in each, and what the last index run did.
    Stats {
        #[arg(long)]
        json: bool,
    },
    /// Index a corpus, ask its golden questions, report recall@5, MRR, latency and speed; fail on a regression.
    Eval {
        /// The outside source.
        #[arg(long, default_value = "eval/corpus")]
        corpus: PathBuf,
        /// The project, whose docs are the second source.
        #[arg(long, default_value = "eval/project")]
        project: PathBuf,
        #[arg(long, default_value = "eval/questions")]
        questions: PathBuf,
        #[arg(long, default_value = "eval/baseline.json")]
        baseline: PathBuf,
        /// Write the results as the new baseline instead of comparing.
        #[arg(long)]
        update_baseline: bool,
        #[arg(long)]
        json: bool,
        /// Print, per question, the rank of the first expected file in each mode.
        #[arg(long)]
        details: bool,
    },
    /// Index the brain repo you are in (or the one `--source` names) and write its index as the
    /// asset to publish: what the brain's CI runs. It starts from the index the last run published.
    Pack {
        /// Where to write the asset.
        #[arg(long, default_value = ".")]
        out: PathBuf,
        /// Minutes after which indexing stops at the next group of chunks and the index so far is
        /// written; the report says it is incomplete, and the next run carries on from it.
        #[arg(long, value_name = "MINUTES")]
        budget: Option<u64>,
        /// The most one part of the asset may hold (a release asset may not pass 2 GiB).
        #[arg(long, default_value_t = shared::PART_BYTES, hide = true)]
        part_bytes: u64,
        #[arg(long)]
        json: bool,
    },
    /// How fast a brain of a given size is on this machine: a synthetic brain of --chunks chunks,
    /// searched, and indexing speed with the real model. Or, with --write-brain, write a synthetic
    /// brain repo of --docs documents to time a first index.
    Bench {
        /// Chunks in the synthetic brain.
        #[arg(long, default_value_t = 100_000)]
        chunks: usize,
        /// Chunks of real-looking text to index with the model, for the indexing speed.
        #[arg(long, default_value_t = 512)]
        embed_sample: usize,
        /// Searches timed in each mode.
        #[arg(long, default_value_t = 30)]
        queries: usize,
        /// Where the synthetic brain is built; by default under the cache folder. It needs about
        /// 4 GB per million chunks, and is removed afterwards.
        #[arg(long)]
        dir: Option<PathBuf>,
        /// Leave the synthetic brain and its index where they are.
        #[arg(long)]
        keep: bool,
        /// Chunks up to which a search scores every vector exactly; above it the 1-bit codes
        /// choose. Lower it to time that pass on a brain too small to need it.
        #[arg(long, default_value_t = search::EXACT_LIMIT, hide = true)]
        exact_limit: usize,
        /// Write a synthetic brain repo to this empty folder instead of benchmarking.
        #[arg(long, value_name = "FOLDER", requires = "docs")]
        write_brain: Option<PathBuf>,
        /// Documents (one chunk each) in the brain --write-brain writes.
        #[arg(long)]
        docs: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// Converts the file on stdin of one format to JSON: how `index` keeps a damaged file's parser
    /// from taking the indexer along. Limited in memory.
    #[command(hide = true)]
    ConvertChild { format: String },
    /// Writes the centre for the 1-bit codes (src/center.f32): the mean embedding of a corpus.
    #[command(hide = true)]
    Center { corpus: PathBuf, out: PathBuf },
}

fn main() {
    // `brain search | head` should end quietly, not panic on the closed pipe.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    if let Err(e) = run() {
        eprintln!("brain: {e:#}");
        std::process::exit(1);
    }
}

/// One source's part of an index run.
#[derive(Serialize)]
struct Named {
    source: String,
    #[serde(flatten)]
    report: index::Report,
    /// The shared index this source's was started from, when one was pulled.
    #[serde(skip_serializing_if = "Option::is_none")]
    shared: Option<shared::Pulled>,
}

/// All sources' index runs: their sum, and each source's own.
#[derive(Serialize)]
struct Combined {
    #[serde(flatten)]
    total: index::Report,
    sources: Vec<Named>,
}

/// Brings the index of every live source up to date, and opens each for reading.
fn index_sources(
    resolved: &Resolved,
    embedder: &mut dyn embed::Embedder,
) -> Result<(Combined, Vec<Opened>)> {
    let mut combined = Combined {
        total: index::Report::default(),
        sources: Vec::new(),
    };
    let mut opened = Vec::new();
    for source in resolved.live() {
        let path = store::index_path(&source.key())?;
        let shared = pull_shared(source, &path)?;
        let mut store = Store::create(&path, &source.root)?;
        let report = index::index(source, &mut store, embedder, true)?;
        combined.total.add(&source.label, &report);
        combined.sources.push(Named {
            source: source.label.clone(),
            report,
            shared,
        });
        opened.push(Opened::new(source, store));
    }
    Ok((combined, opened))
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    if let Command::ConvertChild { format } = &cli.command {
        return convert::child(format);
    }
    if let Ok(exe) = std::env::current_exe() {
        convert::isolate_with(exe);
    }
    match &cli.command {
        Command::ConvertChild { .. } => unreachable!("handled above"),
        Command::Search { question, k, json } => {
            let resolved = resolve(&cli)?;
            let mut embedder = embed::Onnx::load()?;
            let (_, opened) = index_sources(&resolved, &mut embedder)?;
            let question = question.join(" ");
            let hits = search::search(
                &opened,
                &mut embedder,
                &search::Query {
                    question: &question,
                    k: *k,
                    mode: search::Mode::Both,
                },
                search::EXACT_LIMIT,
            )?;
            if *json {
                // One hit per line, so a tool that cuts long lines never swallows later hits.
                let lines = hits
                    .iter()
                    .map(serde_json::to_string)
                    .collect::<Result<Vec<_>, _>>()?;
                println!("[\n{}\n]", lines.join(",\n"));
            } else {
                for (i, h) in hits.iter().enumerate() {
                    let heading = if h.heading.is_empty() {
                        String::new()
                    } else {
                        format!(" › {}", h.heading)
                    };
                    // What an ADR is today: its status, or what replaced it.
                    let mut notes = match (&h.superseded_by, h.status.as_deref()) {
                        (Some(by), _) => format!("; superseded by {by}"),
                        (None, Some(status)) => format!("; {status}"),
                        (None, None) => String::new(),
                    };
                    if !h.amended_by.is_empty() {
                        notes += &format!("; amended by {}", h.amended_by.join(", "));
                    }
                    println!(
                        "{}. [{}] {}{} ({}, score {:.3}{})",
                        i + 1,
                        h.source,
                        h.path,
                        heading,
                        h.date,
                        h.score,
                        notes
                    );
                    for line in search::wrap(&h.snippet, 100) {
                        println!("   {line}");
                    }
                }
            }
        }
        Command::Index { json } => {
            let resolved = resolve(&cli)?;
            let (combined, _) = index_sources(&resolved, &mut embed::Onnx::load()?)?;
            if *json {
                println!("{}", serde_json::to_string(&combined)?);
            } else {
                for n in &combined.sources {
                    let r = &n.report;
                    if let Some(p) = &n.shared {
                        println!(
                            "{}: pulled the shared index built at {} ({:.1} MB)",
                            n.source,
                            &p.commit[..p.commit.len().min(12)],
                            p.bytes as f64 / 1e6
                        );
                    }
                    println!(
                        "{}: {} files, {} chunks; {} files changed, {} removed, {} chunks embedded in {:.1}s",
                        n.source, r.files, r.chunks, r.files_changed, r.files_removed, r.embedded_chunks, r.seconds
                    );
                    if r.duplicates > 0 {
                        println!("  {} files are copies of a newer file", r.duplicates);
                    }
                }
                print_unconverted(
                    &combined
                        .total
                        .unconverted
                        .iter()
                        .map(|u| {
                            format!(
                                "{}: {}: {}",
                                u.source.as_deref().unwrap_or(""),
                                u.path,
                                u.reason
                            )
                        })
                        .collect::<Vec<_>>(),
                );
                print_skipped(&combined.total.skipped);
            }
        }
        Command::Stats { json } => stats(&resolve(&cli)?, *json)?,
        Command::Eval {
            corpus,
            project,
            questions,
            baseline,
            update_baseline,
            json,
            details,
        } => {
            let mut embedder = embed::Onnx::load()?;
            eval::run(
                &eval::Opts {
                    corpus,
                    project,
                    questions,
                    baseline,
                    update_baseline: *update_baseline,
                    json: *json,
                    details: *details,
                },
                &mut embedder,
            )?;
        }
        Command::Pack {
            out,
            budget,
            part_bytes,
            json,
        } => pack(&cli, out, *budget, *part_bytes, *json)?,
        Command::Bench {
            chunks,
            embed_sample,
            queries,
            dir,
            keep,
            exact_limit,
            write_brain,
            docs,
            json,
        } => {
            if let (Some(folder), Some(docs)) = (write_brain, docs) {
                bench::write_brain(folder, *docs)?;
                println!("wrote a brain of {docs} documents to {}", folder.display());
                return Ok(());
            }
            let dir = match dir {
                Some(d) => d.clone(),
                None => store::cache_dir()?,
            };
            let r = bench::run(
                &bench::Opts {
                    chunks: *chunks,
                    embed_sample: *embed_sample,
                    queries: *queries,
                    dir,
                    keep: *keep,
                    exact_limit: *exact_limit,
                },
                &mut embed::Onnx::load()?,
            )?;
            if *json {
                println!("{}", serde_json::to_string(&r)?);
            } else {
                print_bench(&r);
            }
            // The cache folder is left as it was found.
            if let Ok(cache) = store::cache_dir() {
                let _ = std::fs::remove_dir(cache.join("index"));
            }
        }
        Command::Center { corpus, out } => center(corpus, out)?,
    }
    Ok(())
}

/// What the current directory and the flags say to read; a source this machine lacks is warned about.
fn resolve(cli: &Cli) -> Result<Resolved> {
    let cwd = std::env::current_dir().context("cannot read the current directory")?;
    let resolved = sources::resolve(
        &cwd,
        &Flags {
            repo: cli.repo.clone(),
            sources: cli.source.clone(),
        },
    )?;
    for warning in &resolved.warnings {
        eprintln!("brain: warning: {warning}");
    }
    Ok(resolved)
}

/// Starts an outside source's index from the one its brain repo's CI published, when this machine
/// has none that fits; says why when it builds locally instead.
fn pull_shared(source: &Source, path: &Path) -> Result<Option<shared::Pulled>> {
    if source.is_project() || Store::open(path)?.is_some() {
        return Ok(None);
    }
    Ok(match shared::pull(&source.root, path) {
        shared::Outcome::NotABrain => None,
        shared::Outcome::Pulled(p) => Some(p),
        shared::Outcome::Declined(why) => {
            eprintln!(
                "brain: {}: no shared index ({why}); building the index locally",
                source.label
            );
            None
        }
    })
}

/// The brain repo's CI: pull the last published index, bring it up to date with this commit, and
/// write it as the asset to publish.
fn pack(cli: &Cli, out: &Path, budget: Option<u64>, part_bytes: u64, json: bool) -> Result<()> {
    let cwd = std::env::current_dir().context("cannot read the current directory")?;
    let root = cli
        .source
        .first()
        .or(cli.repo.as_ref())
        .cloned()
        .or_else(|| sources::find_project(&cwd))
        .unwrap_or(cwd);
    let root =
        std::fs::canonicalize(&root).with_context(|| format!("cannot read {}", root.display()))?;
    let commit = shared::clean_commit(&root)?;
    let source = Source::outside(&root.to_string_lossy(), &root);
    let path = store::index_path(&source.key())?;
    let mut embedder = embed::Onnx::load()?;
    let pulled = pull_shared(&source, &path)?;
    let mut store = Store::create(&path, &root)?;
    let deadline =
        budget.map(|m| std::time::Instant::now() + std::time::Duration::from_secs(m * 60));
    let report = index::index_until(&source, &mut store, &mut embedder, true, deadline)?;
    let assets = shared::pack(&store, &commit, out, part_bytes)?;
    let names: Vec<String> = assets
        .iter()
        .filter_map(|a| a.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .collect();
    let mut bytes = 0;
    for a in &assets {
        bytes += std::fs::metadata(a)?.len();
    }
    if json {
        println!(
            "{}",
            serde_json::json!({
                "assets": names,
                "bytes": bytes,
                "commit": commit,
                "pulled": pulled,
                "report": report,
            })
        );
    } else {
        for name in &names {
            println!("asset {name}");
        }
        println!(
            "{} files, {} chunks, {:.1} MB packed; {} chunks embedded in {:.1}s ({})",
            report.files,
            report.chunks,
            bytes as f64 / 1e6,
            report.embedded_chunks,
            report.seconds,
            match &pulled {
                Some(p) => format!(
                    "started from the index built at {}",
                    &p.commit[..p.commit.len().min(12)]
                ),
                None => "built from scratch".to_string(),
            }
        );
        if report.incomplete {
            println!("the time budget ran out: files not yet indexed are missing, and the next run carries on");
        }
        print_unconverted(
            &report
                .unconverted
                .iter()
                .map(|u| format!("{}: {}", u.path, u.reason))
                .collect::<Vec<_>>(),
        );
        print_skipped(&report.skipped);
    }
    Ok(())
}

fn print_bench(r: &bench::Results) {
    println!(
        "{} chunks in {} files ({:.1} GB), stored in {:.0}s; {} threads",
        r.chunks,
        r.files,
        r.index_bytes as f64 / 1e9,
        r.fill_seconds,
        r.threads
    );
    println!(
        "search p50   both halves {:.0} ms, full-text {:.0} ms, by meaning {:.0} ms, of which {:.0} ms embeds the question (one process)",
        r.search_p50_ms, r.search_full_text_p50_ms, r.search_meaning_p50_ms, r.query_embed_p50_ms
    );
    println!(
        "indexing     {:.1} chunks/s with the model, over {} chunks",
        r.index_chunks_per_s, r.index_sample_chunks
    );
    println!("targets at 1M chunks: search p50 ≤ 500 ms, indexing ≥ 12 chunks/s");
}

fn stats(resolved: &Resolved, json: bool) -> Result<()> {
    let mut sources = Vec::new();
    let (mut files, mut chunks) = (0, 0);
    for source in resolved.live() {
        let path = store::index_path(&source.key())?;
        let Some(store) = Store::open(&path)? else {
            sources.push(serde_json::json!({
                "source": source.label,
                "root": source.root,
                "indexed": false,
            }));
            continue;
        };
        let (f, c) = store.counts()?;
        files += f;
        chunks += c;
        let parse = |key: &str| -> Result<serde_json::Value> {
            Ok(store.meta(key)?.map_or(serde_json::Value::Null, |s| {
                serde_json::from_str(&s).unwrap_or_default()
            }))
        };
        sources.push(serde_json::json!({
            "source": source.label,
            "root": source.root,
            "indexed": true,
            "index": path,
            "index_bytes": std::fs::metadata(&path)?.len(),
            "files": f,
            "chunks": c,
            "duplicates": store.duplicates()?,
            "last_index": parse("last_index")?,
            "shared_index": parse("shared_index")?,
        }));
    }
    let out = serde_json::json!({
        "model": embed::MODEL_ID,
        "version": env!("CARGO_PKG_VERSION"),
        "files": files,
        "chunks": chunks,
        "sources": sources,
    });
    if json {
        println!("{out}");
        return Ok(());
    }
    println!(
        "model      {} (binary {})",
        embed::MODEL_ID,
        env!("CARGO_PKG_VERSION")
    );
    let mut skipped: Vec<String> = Vec::new();
    let mut unconverted: Vec<String> = Vec::new();
    for s in &sources {
        println!(
            "\n{}  {}",
            s["source"].as_str().unwrap_or(""),
            s["root"].as_str().unwrap_or("")
        );
        if s["indexed"] != true {
            println!("  not indexed yet; run `brain index`");
            continue;
        }
        println!(
            "  index    {} ({} bytes)",
            s["index"].as_str().unwrap_or(""),
            s["index_bytes"]
        );
        println!("  indexed  {} files, {} chunks", s["files"], s["chunks"]);
        if let Some(commit) = s["shared_index"]["commit"].as_str() {
            println!(
                "  started from the shared index built at {}",
                &commit[..commit.len().min(12)]
            );
        }
        let last = &s["last_index"];
        println!(
            "  last run {} files changed, {} removed, {} chunks embedded, {:.1}s",
            last["files_changed"],
            last["files_removed"],
            last["embedded_chunks"],
            last["seconds"].as_f64().unwrap_or(0.0)
        );
        if s["duplicates"].as_u64() > Some(0) {
            println!(
                "  {} files are copies of a newer file, and only the newest ranks",
                s["duplicates"]
            );
        }
        let label = s["source"].as_str().unwrap_or("");
        unconverted.extend(
            last["unconverted"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|u| {
                    format!(
                        "{label}: {}: {}",
                        u["path"].as_str().unwrap_or(""),
                        u["reason"].as_str().unwrap_or("")
                    )
                }),
        );
        skipped.extend(
            last["skipped"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str())
                .map(|v| format!("{}: {v}", s["source"].as_str().unwrap_or(""))),
        );
    }
    println!();
    print_unconverted(&unconverted);
    print_skipped(&skipped);
    Ok(())
}

fn print_list(title: &str, entries: &[String]) {
    if entries.is_empty() {
        return;
    }
    println!("{title} ({}):", entries.len());
    for s in entries.iter().take(10) {
        println!("  {s}");
    }
    if entries.len() > 10 {
        println!("  and {} more", entries.len() - 10);
    }
}

/// Files with no text to index (scans, images, audio, video, encrypted or corrupt files), with
/// why: they are reported, never silently dropped.
fn print_unconverted(entries: &[String]) {
    print_list("not converted", entries);
}

/// Files the last run left out for size or because they could not be read, with why.
fn print_skipped(skipped: &[String]) {
    print_list("left out", skipped);
}

/// Mean embedding of every chunk in `corpus`, as little-endian f32.
fn center(corpus: &Path, out: &Path) -> Result<()> {
    let mut embedder = embed::Onnx::load()?;
    let files = index::walk(&Source::outside("corpus", corpus))?.files;
    let mut sum = vec![0f32; embed::DIM];
    let mut n = 0f32;
    for file in files {
        let convert::Outcome::Text(converted) =
            convert::convert(file.format, &std::fs::read(&file.path)?)
        else {
            continue;
        };
        let count = |s: &str| embedder.count_tokens(s);
        let chunks = chunk::chunk(
            &converted.text,
            &count,
            &chunk::Params {
                headings: converted.prose,
                ..chunk::Params::default()
            },
        );
        let texts: Vec<&str> = chunks.iter().map(|c| c.text.as_str()).collect();
        for v in embedder.embed_passages(&texts)? {
            for (s, x) in sum.iter_mut().zip(v) {
                *s += x;
            }
            n += 1.0;
        }
    }
    let bytes: Vec<u8> = sum.iter().flat_map(|s| (s / n).to_le_bytes()).collect();
    std::fs::write(out, bytes)?;
    println!("centre of {n} chunks written to {}", out.display());
    Ok(())
}
