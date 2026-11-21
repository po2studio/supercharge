mod chunk;
mod doc;
mod embed;
mod eval;
mod index;
mod search;
mod store;
mod vecs;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use embed::Embedder;
use std::path::{Path, PathBuf};
use store::Store;

#[derive(Parser)]
#[command(
    version,
    about = "Index a folder of documents and search it, on the CPU, offline"
)]
struct Cli {
    /// The brain: the folder to index and search.
    #[arg(long, global = true, default_value = ".")]
    root: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Bring the index up to date: embed only the files whose content changed.
    Index {
        #[arg(long)]
        json: bool,
    },
    /// Search the index. Give --terms (full-text, FTS5 syntax), --about (a plain-language description), or both.
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
    /// What is indexed, and what the last index run did.
    Stats {
        #[arg(long)]
        json: bool,
    },
    /// Index a corpus, ask its golden questions, report recall@5, MRR, latency and speed; fail on a regression.
    Eval {
        #[arg(long, default_value = "eval/corpus")]
        corpus: PathBuf,
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
    if let Err(e) = run() {
        eprintln!("brain: {e:#}");
        std::process::exit(1);
    }
}

fn root(cli: &Cli) -> Result<PathBuf> {
    cli.root
        .canonicalize()
        .with_context(|| format!("the brain folder {} does not exist", cli.root.display()))
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match &cli.command {
        Command::Index { json } => {
            let root = root(&cli)?;
            let mut store = Store::create(&store::index_path(&root)?, &root)?;
            let mut embedder = embed::Onnx::load()?;
            let r = index::index(&root, &mut store, &mut embedder, true)?;
            if *json {
                println!("{}", serde_json::to_string(&r)?);
            } else {
                println!(
                    "{} files, {} chunks; {} files changed, {} removed, {} chunks embedded in {:.1}s",
                    r.files, r.chunks, r.files_changed, r.files_removed, r.embedded_chunks, r.seconds
                );
            }
        }
        Command::Search {
            terms,
            about,
            k,
            json,
        } => {
            let root = root(&cli)?;
            let path = store::index_path(&root)?;
            let Some(store) = Store::open(&path)? else {
                bail!(
                    "no index for {} yet (or it is out of date); run `brain index` first",
                    root.display()
                );
            };
            let query = search::Query {
                terms: terms.as_deref(),
                about: about.as_deref(),
                k: *k,
            };
            let hits = if about.is_some() {
                search::search(
                    &store,
                    &mut embed::Onnx::load()?,
                    &query,
                    search::EXACT_LIMIT,
                )?
            } else {
                search::search(&store, &mut NoEmbedder, &query, search::EXACT_LIMIT)?
            };
            if *json {
                println!("{}", serde_json::to_string(&hits)?);
            } else {
                for (i, h) in hits.iter().enumerate() {
                    let heading = if h.heading.is_empty() {
                        String::new()
                    } else {
                        format!(" › {}", h.heading)
                    };
                    println!(
                        "{}. {}{} ({}, score {:.3})\n   {}",
                        i + 1,
                        h.path,
                        heading,
                        h.date,
                        h.score,
                        h.snippet
                    );
                }
            }
        }
        Command::Stats { json } => stats(&root(&cli)?, *json)?,
        Command::Eval {
            corpus,
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

fn stats(root: &Path, json: bool) -> Result<()> {
    let path = store::index_path(root)?;
    let Some(store) = Store::open(&path)? else {
        bail!(
            "no index for {} yet (or it is out of date); run `brain index` first",
            root.display()
        );
    };
    let (files, chunks) = store.counts()?;
    let parse = |key: &str| -> Result<serde_json::Value> {
        Ok(store.meta(key)?.map_or(serde_json::Value::Null, |s| {
            serde_json::from_str(&s).unwrap_or_default()
        }))
    };
    let out = serde_json::json!({
        "root": root,
        "index": path,
        "index_bytes": std::fs::metadata(&path)?.len(),
        "model": embed::MODEL_ID,
        "version": env!("CARGO_PKG_VERSION"),
        "files": files,
        "chunks": chunks,
        "not_indexed": parse("not_indexed")?,
        "last_index": parse("last_index")?,
    });
    if json {
        println!("{out}");
    } else {
        println!(
            "brain      {}\nindex      {} ({} bytes)\nmodel      {} (binary {})",
            root.display(),
            path.display(),
            out["index_bytes"],
            embed::MODEL_ID,
            env!("CARGO_PKG_VERSION")
        );
        println!("indexed    {files} files, {chunks} chunks");
        let last = &out["last_index"];
        println!(
            "last run   {} files changed, {} removed, {} chunks embedded, {:.1}s",
            last["files_changed"],
            last["files_removed"],
            last["embedded_chunks"],
            last["seconds"].as_f64().unwrap_or(0.0)
        );
        if let Some(m) = out["not_indexed"].as_object().filter(|m| !m.is_empty()) {
            let list: Vec<String> = m.iter().map(|(k, v)| format!("{v} .{k}")).collect();
            println!("not indexed (no converter yet): {}", list.join(", "));
        }
    }
    Ok(())
}

/// Mean embedding of every chunk in `corpus`, as little-endian f32.
fn center(corpus: &Path, out: &Path) -> Result<()> {
    let mut embedder = embed::Onnx::load()?;
    let (files, _) = index::walk(corpus)?;
    let mut sum = vec![0f32; embed::DIM];
    let mut n = 0f32;
    for rel in files {
        let text = std::fs::read_to_string(corpus.join(&rel))?;
        let parsed = doc::parse(&text);
        let count = |s: &str| embedder.count_tokens(s);
        let chunks = chunk::chunk(parsed.body, &count, &chunk::Params::default());
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
