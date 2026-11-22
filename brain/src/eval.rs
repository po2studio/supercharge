//! `brain eval`: index a corpus, ask the golden questions as plain questions, report recall@5, MRR,
//! p50 latency and indexing speed against a committed baseline. It measures the two halves of the
//! search (full-text, by meaning) as well as the fused search `brain search` runs.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path, time::Instant};

use crate::{
    embed::Embedder,
    index,
    search::{self, Mode, Opened, Query},
    sources::{self, Source},
    store::Store,
};

/// A quality metric may fall this far below the baseline before CI fails.
const TOLERANCE: f64 = 0.015;
/// Speed may be this many times worse than the baseline: CI machines are not the baseline's.
const SPEED_FACTOR: f64 = 4.0;

#[derive(Deserialize)]
struct Question {
    id: String,
    question: String,
    expect: Vec<String>,
    /// Files that must not be among the top 5 hits: an older version of the answer.
    #[serde(default)]
    absent: Vec<String>,
    /// Files that may come back, but only after the first of `expect`: an ADR that a later one
    /// superseded.
    #[serde(default)]
    after: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Score {
    pub recall_at_5: f64,
    pub mrr: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Results {
    pub questions: usize,
    /// Sources indexed: the project's docs and the outside corpus.
    #[serde(default)]
    pub sources: usize,
    pub files: usize,
    pub chunks: usize,
    /// The full-text half alone, the by-meaning half alone, and the two fused: what `brain
    /// search` runs.
    pub full_text: Score,
    pub meaning: Score,
    pub hybrid: Score,
    pub p50_ms: f64,
    pub chunks_per_s: f64,
}

fn load_questions(dir: &Path) -> Result<Vec<Question>> {
    let mut paths: Vec<_> = fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "yaml" || e == "yml"))
        .collect();
    paths.sort();
    let mut out: Vec<Question> = Vec::new();
    for p in paths {
        let qs: Vec<Question> = serde_yaml_ng::from_str(&fs::read_to_string(&p)?)
            .with_context(|| format!("parsing {}", p.display()))?;
        out.extend(qs);
    }
    let mut seen = std::collections::HashSet::new();
    for q in &out {
        if !seen.insert(q.id.as_str()) {
            bail!("question id {} appears twice", q.id);
        }
    }
    Ok(out)
}

pub fn p50(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v.get(v.len() / 2).copied().unwrap_or(0.0)
}

/// Whether a hit is the file an `expect` entry names: `project:<path>` is a file of the project's
/// docs, a bare path one of the outside corpus.
fn is_expected(hit: &search::Hit, expect: &str) -> bool {
    match expect.strip_prefix("project:") {
        Some(path) => hit.source == sources::PROJECT && hit.path == path,
        None => hit.source != sources::PROJECT && hit.path == expect,
    }
}

/// Indexes the project's docs and the corpus as two sources, and searches across both.
pub fn measure(
    corpus: &Path,
    project: &Path,
    questions: &Path,
    embedder: &mut dyn Embedder,
    details: bool,
) -> Result<Results> {
    let questions = load_questions(questions)?;
    // Relative to where `eval` runs, not to the project the sources are resolved in.
    let corpus = corpus
        .canonicalize()
        .with_context(|| format!("the corpus {} does not exist", corpus.display()))?;
    let resolved = sources::resolve(
        project,
        &sources::Flags {
            repo: Some(project.to_path_buf()),
            sources: vec![corpus],
        },
    )?;
    let live = resolved.live();
    let walks: Vec<(&Source, Vec<String>)> = live
        .iter()
        .map(|s| {
            Ok((
                *s,
                index::walk(s)?.files.into_iter().map(|f| f.rel).collect(),
            ))
        })
        .collect::<Result<_>>()?;
    for q in &questions {
        for e in q.expect.iter().chain(&q.absent).chain(&q.after) {
            let (want_project, path) = match e.strip_prefix("project:") {
                Some(p) => (true, p),
                None => (false, e.as_str()),
            };
            if !walks
                .iter()
                .any(|(s, rels)| s.is_project() == want_project && rels.iter().any(|r| r == path))
            {
                bail!("question {} expects {e}, which is not in the corpus", q.id);
            }
        }
    }
    let tmp = tempdir()?;
    let started = Instant::now();
    let (mut files, mut chunks, mut embedded) = (0, 0, 0);
    let mut opened = Vec::new();
    for (n, source) in live.iter().enumerate() {
        let path = tmp.join(format!("{n}/index.sqlite"));
        let mut store = Store::create(&path, &source.root)?;
        let report = index::index(source, &mut store, embedder, false)?;
        files += report.files;
        chunks += report.chunks;
        embedded += report.embedded_chunks;
        opened.push(Opened::new(source, store));
    }
    let secs = started.elapsed().as_secs_f64();
    let report = (files, chunks, embedded);

    const MODES: [(Mode, &str); 3] = [
        (Mode::FullText, "full-text"),
        (Mode::Meaning, "meaning"),
        (Mode::Both, "hybrid"),
    ];
    let (mut sums, mut latencies) = ([(0.0, 0.0); 3], Vec::new());
    let mut returned: Vec<String> = Vec::new();
    for q in &questions {
        let mut ranks = Vec::new();
        for (i, (mode, name)) in MODES.iter().enumerate() {
            let query = Query {
                question: &q.question,
                k: 10,
                mode: *mode,
            };
            let t = Instant::now();
            let hits = search::search(&opened, embedder, &query, search::EXACT_LIMIT)?;
            if *mode == Mode::Both {
                latencies.push(t.elapsed().as_secs_f64() * 1000.0);
            }
            let rank_of = |p: &str| hits.iter().position(|h| is_expected(h, p));
            for a in &q.absent {
                if rank_of(a).is_some_and(|r| r < 5) {
                    returned.push(format!(
                        "question {}: {a} is among the top 5 hits of the {name} search, and should have been left out",
                        q.id
                    ));
                }
            }
            let first = q.expect.iter().filter_map(|e| rank_of(e)).min();
            for a in &q.after {
                if let (Some(r), Some(f)) = (rank_of(a), first) {
                    if r < f {
                        returned.push(format!(
                            "question {}: {a} ranks above {} in the {name} search, and should come after it",
                            q.id, q.expect[0]
                        ));
                    }
                }
            }
            let found = q
                .expect
                .iter()
                .filter(|e| rank_of(e).is_some_and(|r| r < 5))
                .count();
            sums[i].0 += found as f64 / q.expect.len() as f64;
            sums[i].1 += first.map_or(0.0, |r| 1.0 / (r as f64 + 1.0));
            ranks.push(first.map_or("-".to_string(), |r| (r + 1).to_string()));
        }
        if details {
            eprintln!(
                "{:<22} full-text {:>2}  meaning {:>2}  hybrid {:>2}",
                q.id, ranks[0], ranks[1], ranks[2]
            );
        }
    }
    if !returned.is_empty() {
        bail!("{}", returned.join("\n"));
    }
    let n = questions.len().max(1) as f64;
    let score = |i: usize| Score {
        recall_at_5: round(sums[i].0 / n),
        mrr: round(sums[i].1 / n),
    };
    let _ = fs::remove_dir_all(&tmp);
    Ok(Results {
        questions: questions.len(),
        sources: live.len(),
        files: report.0,
        chunks: report.1,
        full_text: score(0),
        meaning: score(1),
        hybrid: score(2),
        p50_ms: round(p50(latencies)),
        chunks_per_s: round(report.2 as f64 / secs.max(1e-9)),
    })
}

fn round(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

fn tempdir() -> Result<std::path::PathBuf> {
    let dir = std::env::temp_dir().join(format!("brain-eval-{}", std::process::id()));
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn render(r: &Results) -> String {
    let row = |name: &str, m: &Score| format!("{name:<10} {:>9.3} {:>7.3}\n", m.recall_at_5, m.mrr);
    format!(
        "{} questions over {} files, {} chunks, {} sources\n{:<10} {:>9} {:>7}\n{}{}{}search p50 {:.0} ms, indexing {:.1} chunks/s\n",
        r.questions, r.files, r.chunks, r.sources, "", "recall@5", "MRR",
        row("full-text", &r.full_text), row("meaning", &r.meaning), row("hybrid", &r.hybrid),
        r.p50_ms, r.chunks_per_s
    )
}

/// What got worse than the baseline allows, and whether the fused search still finds at least as
/// many answers as the better of its two halves.
pub fn regressions(now: &Results, base: &Results) -> Vec<String> {
    let mut out = Vec::new();
    let mut quality = |name: &str, now: &Score, base: &Score| {
        for (metric, a, b) in [
            ("recall@5", now.recall_at_5, base.recall_at_5),
            ("MRR", now.mrr, base.mrr),
        ] {
            if a < b - TOLERANCE {
                out.push(format!(
                    "{name} {metric} {a:.3} regressed from the baseline {b:.3}"
                ));
            }
        }
    };
    quality("full-text", &now.full_text, &base.full_text);
    quality("meaning", &now.meaning, &base.meaning);
    quality("hybrid", &now.hybrid, &base.hybrid);
    if now.p50_ms > base.p50_ms * SPEED_FACTOR && now.p50_ms > 50.0 {
        out.push(format!(
            "search p50 {:.0} ms regressed from the baseline {:.0} ms",
            now.p50_ms, base.p50_ms
        ));
    }
    if now.chunks_per_s < base.chunks_per_s / SPEED_FACTOR {
        out.push(format!(
            "indexing {:.1} chunks/s regressed from the baseline {:.1}",
            now.chunks_per_s, base.chunks_per_s
        ));
    }
    let best = now.full_text.recall_at_5.max(now.meaning.recall_at_5);
    if now.hybrid.recall_at_5 < best {
        out.push(format!(
            "hybrid recall@5 {:.3} is below the better of its halves, {best:.3}",
            now.hybrid.recall_at_5
        ));
    }
    out
}

pub struct Opts<'a> {
    pub corpus: &'a Path,
    pub project: &'a Path,
    pub questions: &'a Path,
    pub baseline: &'a Path,
    pub update_baseline: bool,
    pub json: bool,
    pub details: bool,
}

/// Prints the results; with `update_baseline` writes them as the new baseline, otherwise fails
/// on any regression.
pub fn run(o: &Opts, embedder: &mut dyn Embedder) -> Result<()> {
    let now = measure(o.corpus, o.project, o.questions, embedder, o.details)?;
    if o.json {
        println!("{}", serde_json::to_string_pretty(&now)?);
    } else {
        print!("{}", render(&now));
    }
    if o.update_baseline {
        fs::write(o.baseline, serde_json::to_string_pretty(&now)? + "\n")?;
        eprintln!("baseline written to {}", o.baseline.display());
        return Ok(());
    }
    let base: Results = serde_json::from_str(
        &fs::read_to_string(o.baseline)
            .with_context(|| format!("reading baseline {}", o.baseline.display()))?,
    )?;
    let bad = regressions(&now, &base);
    if bad.is_empty() {
        eprintln!("no regression against the baseline");
        return Ok(());
    }
    bail!("{}", bad.join("\n"));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn results(recall: f64) -> Results {
        let m = Score {
            recall_at_5: recall,
            mrr: recall,
        };
        Results {
            questions: 10,
            full_text: Score {
                recall_at_5: 0.3,
                mrr: 0.3,
            },
            meaning: m.clone(),
            hybrid: m,
            p50_ms: 20.0,
            chunks_per_s: 12.0,
            ..Default::default()
        }
    }

    #[test]
    fn a_drop_beyond_the_tolerance_is_a_regression_and_a_small_one_is_not() {
        assert!(regressions(&results(0.9), &results(0.9)).is_empty());
        assert!(regressions(&results(0.89), &results(0.9)).is_empty());
        let bad = regressions(&results(0.8), &results(0.9));
        assert!(bad.iter().any(|b| b.contains("hybrid recall@5")), "{bad:?}");
    }

    #[test]
    fn hybrid_must_find_as_many_answers_as_the_better_half() {
        let base = results(0.9);
        let mut r = results(0.9);
        r.full_text.recall_at_5 = 0.95;
        assert!(regressions(&r, &base)
            .iter()
            .any(|b| b.contains("below the better of its halves")));
        r.full_text.recall_at_5 = 0.9;
        assert!(regressions(&r, &base).is_empty());
    }

    #[test]
    fn speed_may_vary_with_the_machine_but_not_by_much() {
        let base = results(0.9);
        let mut slow = base.clone();
        slow.chunks_per_s = 4.0;
        assert!(regressions(&slow, &base).is_empty());
        slow.chunks_per_s = 2.0;
        assert!(regressions(&slow, &base)
            .iter()
            .any(|b| b.contains("chunks/s")));
        let mut laggy = base.clone();
        laggy.p50_ms = 400.0;
        assert!(regressions(&laggy, &base).iter().any(|b| b.contains("p50")));
    }

    #[test]
    fn p50_is_the_middle() {
        assert_eq!(p50(vec![5.0, 1.0, 3.0]), 3.0);
        assert_eq!(p50(vec![]), 0.0);
    }
}
