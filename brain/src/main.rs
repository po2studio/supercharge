mod chunk;
mod convert;
mod dedup;
mod doc;
mod embed;
mod eval;
mod index;
mod init;
mod search;
mod sources;
mod store;
mod vecs;

use anyhow::{bail, Context, Result};
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
    about = "Index and search a project's docs and the folders it lists as sources, on the CPU, offline",
    long_about = "Run inside a project: the project's docs folder (the `docs` key in AGENTS.md), README.md and AGENTS.md \
are read in place, and so is each folder in the `brain:` list of AGENTS.md's `## Supercharge` block. \
Indexes live under ~/.cache/supercharge/; nothing is written into a project or a source, except by `brain init`."
)]
struct Cli {
    /// The project's folder; by default the git repo (or the folder with an AGENTS.md) containing the current directory.
    #[arg(long, global = true)]
    repo: Option<PathBuf>,
    /// A folder to read as a source, read-only; repeat for several. Replaces the `brain:` list.
    #[arg(long, global = true)]
    source: Vec<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Bring the index of each source up to date: embed only the files whose content changed.
    Index {
        #[arg(long)]
        json: bool,
    },
    /// Search the project and its sources. Give --terms (full-text, FTS5 syntax), --about (a plain-language description), or both.
    Search {
        /// Full-text query: words are ANDed; use OR between alternatives, "quotes" for phrases.
        #[arg(long)]
        terms: Option<String>,
        /// What the answer is about, in plain language; matched by meaning.
        #[arg(long)]
        about: Option<String>,
        #[arg(short, default_value_t = 5)]
        k: usize,
        #[arg(long)]
        json: bool,
    },
    /// The sources this project reads, what is indexed in each, and what the last index run did.
    Stats {
        #[arg(long)]
        json: bool,
    },
    /// In the `update: true` source, create BRAIN.md, decisions/, notes/ and projects/ if missing. The only command that writes into a source.
    Init,
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

/// One source's part of an index run.
#[derive(Serialize)]
struct Named {
    source: String,
    #[serde(flatten)]
    report: index::Report,
}

/// All sources' index runs: their sum, and each source's own.
#[derive(Serialize)]
struct Combined {
    #[serde(flatten)]
    total: index::Report,
    sources: Vec<Named>,
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match &cli.command {
        Command::Index { json } => {
            let resolved = resolve(&cli)?;
            let mut embedder = embed::Onnx::load()?;
            let mut combined = Combined {
                total: index::Report::default(),
                sources: Vec::new(),
            };
            for source in resolved.live() {
                let mut store = Store::create(&store::index_path(&source.key())?, &source.root)?;
                let report = index::index(source, &mut store, &mut embedder, true)?;
                combined.total.add(&source.label, &report);
                combined.sources.push(Named {
                    source: source.label.clone(),
                    report,
                });
            }
            if *json {
                println!("{}", serde_json::to_string(&combined)?);
            } else {
                for n in &combined.sources {
                    let r = &n.report;
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
        Command::Search {
            terms,
            about,
            k,
            json,
        } => {
            let resolved = resolve(&cli)?;
            let mut opened = Vec::new();
            for source in resolved.live() {
                let Some(store) = Store::open(&store::index_path(&source.key())?)? else {
                    bail!(
                        "no index for {} yet (or it is out of date); run `brain index` first",
                        source.label
                    );
                };
                opened.push(Opened::new(source, store));
            }
            let query = search::Query {
                terms: terms.as_deref(),
                about: about.as_deref(),
                k: *k,
            };
            let hits = if about.is_some() {
                search::search(
                    &opened,
                    &mut embed::Onnx::load()?,
                    &query,
                    search::EXACT_LIMIT,
                )?
            } else {
                search::search(&opened, &mut NoEmbedder, &query, search::EXACT_LIMIT)?
            };
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
                    println!(
                        "{}. [{}] {}{} ({}, score {:.3})",
                        i + 1,
                        h.source,
                        h.path,
                        heading,
                        h.date,
                        h.score
                    );
                    for line in search::wrap(&h.snippet, 100) {
                        println!("   {line}");
                    }
                }
            }
        }
        Command::Stats { json } => stats(&resolve(&cli)?, *json)?,
        Command::Init => {
            let resolved = resolve(&cli)?;
            let Some(source) = resolved.update_source() else {
                println!("no source sets `update: true`: nothing to create, nothing written");
                return Ok(());
            };
            let created = init::init(source)?;
            if created.is_empty() {
                println!("{}: nothing to create", source.label);
            } else {
                println!("{}: created {}", source.label, created.join(", "));
            }
        }
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
        Command::Center { corpus, out } => center(corpus, out)?,
    }
    Ok(())
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
            "update": source.update,
            "index": path,
            "index_bytes": std::fs::metadata(&path)?.len(),
            "files": f,
            "chunks": c,
            "duplicates": store.duplicates()?,
            "last_index": parse("last_index")?,
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
            "  index    {} ({} bytes){}",
            s["index"].as_str().unwrap_or(""),
            s["index_bytes"],
            if s["update"] == true {
                "; agents write here"
            } else {
                ""
            }
        );
        println!("  indexed  {} files, {} chunks", s["files"], s["chunks"]);
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

/// For a search with no `--about`: nothing is embedded, so the model isn't loaded.
struct NoEmbedder;

impl Embedder for NoEmbedder {
    fn embed_passages(&mut self, _: &[&str]) -> Result<Vec<Vec<f32>>> {
        bail!("no embedder")
    }
    fn embed_query(&mut self, _: &str) -> Result<Vec<f32>> {
        bail!("no embedder")
    }
    fn count_tokens(&self, text: &str) -> usize {
        text.len() / 4 + 1
    }
}
