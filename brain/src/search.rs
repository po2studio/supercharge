//! Search: full-text (BM25 over FTS5) and by-meaning (cosine over the vectors) on the question as
//! given, fused by rank (RRF), across the index of each source a project reads.

use anyhow::{bail, Context, Result};
use rusqlite::OptionalExtension;
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

use crate::{embed::Embedder, sources::Source, store::Store, vecs};

/// The reciprocal-rank-fusion constant.
const RRF_K: f64 = 60.0;
/// What each ranked list counts for in the fusion: the question's words, its exact tokens (an ID,
/// a code, a name, a quoted phrase) on top of them, and its meaning.
const WEIGHT_WORDS: f64 = 1.0;
const WEIGHT_EXACT: f64 = 1.0;
const WEIGHT_MEANING: f64 = 1.0;
/// TEMPORARY tuning knob.
fn tuned() -> [f64; 3] {
    match std::env::var("BRAIN_FUSE") {
        Ok(s) => {
            let v: Vec<f64> = s.split(',').filter_map(|x| x.parse().ok()).collect();
            [v[0], v[1], v[2]]
        }
        Err(_) => [WEIGHT_WORDS, WEIGHT_EXACT, WEIGHT_MEANING],
    }
}
/// Up to this many chunks the vector search scores every vector exactly; beyond it the 1-bit
/// codes pick the candidates to rescore.
pub const EXACT_LIMIT: usize = 50_000;
const SNIPPET_CHARS: usize = 600;
/// The most distinct words of a question that go into the full-text query.
const MAX_WORDS: usize = 40;

#[derive(Debug, Serialize)]
pub struct Hit {
    /// `project` for the project's own docs, else the source's path as it was listed.
    pub source: String,
    /// Relative to the source's folder (the project's root, for the project).
    pub path: String,
    /// Absolute, to read the whole file.
    pub file: String,
    pub title: String,
    pub heading: String,
    pub date: String,
    /// `adr` for a project's ADR, else `raw`.
    pub kind: String,
    /// `proposed`, `accepted` or `superseded` for an ADR, else none.
    pub status: Option<String>,
    /// For a superseded ADR: what replaced it (a path in the same source, else as written).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<String>,
    /// For an amended ADR: the ADRs that amend it, which come with it in the results.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub amended_by: Vec<String>,
    pub score: f64,
    pub versions: Vec<String>,
    pub snippet: String,
}

/// Which halves of the search run. The command line always runs both; `brain eval` and `brain
/// bench` measure each half on its own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    Both,
    FullText,
    Meaning,
}

pub struct Query<'a> {
    pub question: &'a str,
    pub k: usize,
    pub mode: Mode,
}

/// What a question becomes for full-text search: FTS5 phrases, each of which is only used where it
/// is specific (see `specific`).
#[derive(Debug, PartialEq)]
struct Words {
    /// Each of the question's words.
    any: Vec<String>,
    /// Its IDs and codes (a digit, or `-` `_` `/` `@` inside), acronyms and quoted phrases.
    codes: Vec<String>,
    /// Its names: capitalised words after the first, a run of them as one.
    names: Vec<String>,
}

impl Words {
    fn exact(&self) -> Vec<String> {
        self.codes.iter().chain(&self.names).cloned().collect()
    }
}

/// An FTS5 phrase for `text`, or none when it has no letter or digit to match.
fn phrase(text: &str) -> Option<String> {
    text.chars()
        .any(char::is_alphanumeric)
        .then(|| format!("\"{}\"", text.replace('"', " ")))
}

fn full_text(question: &str) -> Words {
    // Quoted phrases are exact as they stand; what is outside the quotes is split into words.
    let mut codes: Vec<String> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    let mut plain = String::new();
    for (n, part) in question.split(['"', '“', '”']).enumerate() {
        if n % 2 == 1 {
            codes.extend(phrase(part.trim()));
        } else {
            plain.push_str(part);
            plain.push(' ');
        }
    }
    let mut name: Vec<&str> = Vec::new();
    let mut starts_sentence = true;
    for raw in plain.split_whitespace() {
        let word = raw.trim_matches(|c: char| !c.is_alphanumeric());
        let ends_sentence = raw.ends_with(['.', '?', '!', ':']);
        let letters = word.chars().filter(|c| c.is_alphabetic());
        let code = word.chars().any(|c| c.is_ascii_digit())
            || word.contains(['-', '_', '/', '@'])
            || (letters.clone().count() >= 2 && letters.clone().all(char::is_uppercase));
        let capitalised = !starts_sentence
            && word.chars().count() >= 2
            && word.chars().next().is_some_and(char::is_uppercase);
        if capitalised && !code {
            name.push(word);
        } else if !name.is_empty() {
            names.extend(phrase(&name.join(" ")));
            name.clear();
        }
        if code {
            codes.extend(phrase(word));
        }
        starts_sentence = ends_sentence;
    }
    names.extend(phrase(&name.join(" ")));

    let mut seen = HashSet::new();
    let any: Vec<String> = plain
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 2)
        .map(str::to_lowercase)
        .filter(|w| seen.insert(w.clone()))
        .take(MAX_WORDS)
        .map(|w| format!("\"{w}\""))
        .collect();
    let mut seen = HashSet::new();
    codes.retain(|p| seen.insert(p.clone()));
    names.retain(|p| seen.insert(p.clone()));
    Words { any, codes, names }
}

/// A source's index, open for reading.
pub struct Opened {
    pub label: String,
    pub root: PathBuf,
    is_project: bool,
    pub store: Store,
}

impl Opened {
    pub fn is_project(&self) -> bool {
        self.is_project
    }

    pub fn new(source: &Source, store: Store) -> Opened {
        Opened {
            label: source.label.clone(),
            root: source.root.clone(),
            is_project: source.is_project(),
            store,
        }
    }
}

/// A chunk in one of the opened sources: its position in the list, and its row.
type Key = (usize, i64);

pub fn search(
    sources: &[Opened],
    embedder: &mut dyn Embedder,
    q: &Query,
    exact_limit: usize,
) -> Result<Vec<Hit>> {
    run(sources, embedder, q, exact_limit, specific_percent())
}

fn run(
    sources: &[Opened],
    embedder: &mut dyn Embedder,
    q: &Query,
    exact_limit: usize,
    percent: usize,
) -> Result<Vec<Hit>> {
    if q.question.trim().is_empty() {
        bail!("ask a question: brain search \"<question>\"");
    }
    let words = full_text(q.question);
    let query_vector = (q.mode != Mode::FullText)
        .then(|| embedder.embed_query(q.question))
        .transpose()?;
    // One hit per file, its best chunk. A file with many matching chunks can fill the candidate
    // pool on its own, so the pool grows until it yields enough files or holds every match.
    let mut pool = (q.k * 10).max(50);
    // Per source, the question's words and exact tokens that say something there.
    let narrowed: Vec<[String; 2]> = if q.mode == Mode::Meaning {
        Vec::new()
    } else {
        sources
            .iter()
            .map(|s| {
                Ok([
                    specific(&s.store, &words.any, percent)?,
                    specific(&s.store, &words.exact(), percent)?,
                ])
            })
            .collect::<Result<_>>()?
    };
    loop {
        let mut exhausted = true;
        let mut lists: Vec<(f64, Vec<(Key, f64)>)> = Vec::new();
        let w = tuned();
        for (i, weight) in [w[0], w[1]].into_iter().enumerate() {
            if narrowed.iter().all(|n| n[i].is_empty()) {
                continue;
            }
            let list = merge(sources, pool, &mut exhausted, |n, s| match narrowed[n][i].as_str() {
                "" => Ok(Vec::new()),
                text => fts(&s.store, text, pool),
            })?;
            lists.push((weight, list));
        }
        if let Some(v) = &query_vector {
            let list = merge(sources, pool, &mut exhausted, |_, s| {
                vector(&s.store, v, pool, exact_limit)
            })?;
            lists.push((w[2], list));
        }
        let mut hits: Vec<(usize, Hit)> = Vec::new();
        // Superseded hits go last, so they don't count towards the `k` that fill the results.
        let mut current = 0;
        let mut seen: HashSet<(usize, String)> = HashSet::new();
        for ((n, id), score) in fuse(&lists) {
            // A file that has a newer copy never ranks: the copy does, and lists it.
            let Some(hit) = chunk_hit(&sources[n], id, score)? else {
                continue;
            };
            // One hit per file of a source; the label is for people, so two sources are told
            // apart by position, not by name.
            if seen.insert((n, hit.path.clone())) {
                current += usize::from(hit.status.as_deref() != Some("superseded"));
                hits.push((n, hit));
            }
            if current == q.k {
                break;
            }
        }
        if current == q.k || exhausted {
            return finish(sources, hits, q.k);
        }
        pool *= 4;
    }
}

/// TEMPORARY: the raw ranked lists (file-level, best chunk per file) for offline fusion study.
pub fn dump(
    sources: &[Opened],
    embedder: &mut dyn Embedder,
    question: &str,
) -> Result<serde_json::Value> {
    let words = full_text(question);
    let pool = 100;
    let file_list = |lists: Vec<(Key, f64)>| -> Result<Vec<(String, f64)>> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for ((n, id), score) in lists {
            if let Some(hit) = chunk_hit(&sources[n], id, score)? {
                if seen.insert((n, hit.path.clone())) {
                    let name = if sources[n].is_project { format!("project:{}", hit.path) } else { hit.path };
                    out.push((name, score));
                }
            }
        }
        Ok(out)
    };
    let mut ex = false;
    let v = embedder.embed_query(question)?;
    let meaning = merge(sources, pool, &mut ex, |_, s| vector(&s.store, &v, pool, EXACT_LIMIT))?;
    let mut result = serde_json::Map::new();
    result.insert("meaning".into(), serde_json::to_value(file_list(meaning)?)?);
    for (name, terms) in [("any", &words.any), ("codes", &words.codes), ("names", &words.names)] {
        for (suffix, filtered) in [("", true), ("_all", false)] {
            let list = merge(sources, pool, &mut ex, |_, s| {
                let text = if filtered { specific(&s.store, terms, 1)? } else { terms.join(" OR ") };
                if text.is_empty() { Ok(Vec::new()) } else { fts(&s.store, &text, pool) }
            })?;
            result.insert(format!("{name}{suffix}"), serde_json::to_value(file_list(list)?)?);
        }
    }
    Ok(serde_json::Value::Object(result))
}

/// The best `k` of the hits (most relevant first), the current ones before any superseded ADR.
/// Each superseded ADR is preceded by what replaced it, and each amended one followed by the ADRs
/// that amend it, if they weren't hit already.
fn finish(sources: &[Opened], hits: Vec<(usize, Hit)>, k: usize) -> Result<Vec<Hit>> {
    let (old, mut ordered): (Vec<_>, Vec<_>) = hits
        .into_iter()
        .partition(|(_, h)| h.status.as_deref() == Some("superseded"));
    ordered.extend(old);
    ordered.truncate(k);
    let mut seen: HashSet<(usize, String)> =
        ordered.iter().map(|(n, h)| (*n, h.path.clone())).collect();
    let mut out = Vec::new();
    for (n, hit) in ordered {
        push_with_companions(sources, n, hit, &mut out, &mut seen)?;
    }
    Ok(out)
}

/// `hit` with the ADR that replaced it before, and those that amend it after, each with theirs in
/// turn; `seen` also stops a cycle.
fn push_with_companions(
    sources: &[Opened],
    n: usize,
    hit: Hit,
    out: &mut Vec<Hit>,
    seen: &mut HashSet<(usize, String)>,
) -> Result<()> {
    if let Some(by) = &hit.superseded_by {
        if seen.insert((n, by.clone())) {
            if let Some(next) = file_hit(&sources[n], by, hit.score)? {
                push_with_companions(sources, n, next, out, seen)?;
            }
        }
    }
    let (amended_by, score) = (hit.amended_by.clone(), hit.score);
    out.push(hit);
    for path in amended_by {
        if seen.insert((n, path.clone())) {
            if let Some(extra) = file_hit(&sources[n], &path, score)? {
                push_with_companions(sources, n, extra, out, seen)?;
            }
        }
    }
    Ok(())
}

/// Each source's best `pool` matches by `one`, as one list by score: cosine is the same measure
/// in every index, and BM25 close enough to it for a source's words to rank beside another's.
/// `exhausted` goes false when some source had more matches than it returned.
fn merge(
    sources: &[Opened],
    pool: usize,
    exhausted: &mut bool,
    one: impl Fn(usize, &Opened) -> Result<Vec<(i64, f64)>>,
) -> Result<Vec<(Key, f64)>> {
    let mut all: Vec<(Key, f64)> = Vec::new();
    for (n, source) in sources.iter().enumerate() {
        let list = one(n, source)?;
        if list.len() >= pool {
            *exhausted = false;
        }
        all.extend(list.into_iter().map(|(id, score)| ((n, id), score)));
    }
    all.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    // Several sources' lists can together hold more than the pool keeps.
    if all.len() > pool {
        *exhausted = false;
    }
    all.truncate(pool);
    Ok(all)
}

/// Weighted reciprocal-rank fusion of ranked lists.
fn fuse(lists: &[(f64, Vec<(Key, f64)>)]) -> Vec<(Key, f64)> {
    let mut score: HashMap<Key, f64> = HashMap::new();
    for (weight, list) in lists {
        for (rank, (key, _)) in list.iter().enumerate() {
            *score.entry(*key).or_insert(0.0) += weight / (RRF_K + rank as f64 + 1.0);
        }
    }
    let mut out: Vec<(Key, f64)> = score.into_iter().collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    out
}

/// What share of an index's chunks (in percent; at least 10 chunks) a term may match and still be
/// searched for: a word that most passages share says nothing the by-meaning half does not say
/// better, and only crowds the ranking with passages that happen to use it.
fn specific_percent() -> usize {
    std::env::var("BRAIN_DF").ok().and_then(|s| s.parse().ok()).unwrap_or(1)
}

/// The terms (FTS5 phrases) that match few enough chunks of this index to say something, as one
/// query; empty when none does. Counting stops one past the limit, so a common word costs no more
/// than a rare one however big the index is.
fn specific(store: &Store, terms: &[String], percent: usize) -> Result<String> {
    if terms.is_empty() {
        return Ok(String::new());
    }
    let limit = ((store.counts()?.1 as usize * percent) / 100).max(10);
    let mut stmt = store
        .conn
        .prepare("SELECT count(*) FROM (SELECT 1 FROM fts WHERE fts MATCH ?1 LIMIT ?2)")?;
    let mut keep: Vec<&str> = Vec::new();
    for term in terms {
        let matches: i64 = stmt.query_row(rusqlite::params![term, limit as i64 + 1], |r| r.get(0))?;
        if matches as usize <= limit {
            keep.push(term);
        }
    }
    Ok(keep.join(" OR "))
}

/// Full-text matches of an FTS5 query, best first, with `-bm25` as the score.
fn fts(store: &Store, query: &str, pool: usize) -> Result<Vec<(i64, f64)>> {
    let mut stmt = store.conn.prepare(
        "SELECT rowid, -bm25(fts) FROM fts WHERE fts MATCH ?1 ORDER BY bm25(fts) LIMIT ?2",
    )?;
    let rows = stmt.query_map(rusqlite::params![query, pool as i64], |r| {
        Ok((r.get(0)?, r.get(1)?))
    })?;
    rows.collect::<rusqlite::Result<_>>()
        .with_context(|| format!("cannot search for {query:?}"))
}

/// Threads that read the 1-bit codes at once: more than this and SQLite's page cache and the
/// disk, not the cores, are what a scan waits for.
const MAX_SCAN_THREADS: usize = 8;
/// Chunks from which the scan is split over threads: below it, opening a connection per thread
/// costs more than the scan (measured at 50 000 chunks: 27 ms in one thread, 40 ms in six).
const PARALLEL_SCAN_FROM: usize = 200_000;

/// The `keep` chunks whose 1-bit codes are nearest `qcode`, as `(distance, id)`, nearest first
/// and by id on a tie. Reading every code of a big brain is the cost of a search, so the chunks
/// are split by file into `threads` ranges, each read on a connection of its own.
fn nearest_codes(
    store: &Store,
    qcode: &[u8],
    keep: usize,
    threads: usize,
) -> Result<Vec<(u32, i64)>> {
    let scan =
        |conn: &rusqlite::Connection, range: Option<(i64, i64)>| -> Result<Vec<(u32, i64)>> {
            let (clause, bound) = match range {
                None => ("", Vec::new()),
                Some((from, to)) => (" WHERE file_id BETWEEN ? AND ?", vec![from, to]),
            };
            let mut stmt = conn.prepare(&format!("SELECT id, code FROM chunks{clause}"))?;
            // The distance is taken on the bytes SQLite holds: no copy of each code.
            let rows = stmt.query_map(rusqlite::params_from_iter(bound), |r| {
                Ok((
                    vecs::hamming(qcode, r.get_ref(1)?.as_blob()?),
                    r.get::<_, i64>(0)?,
                ))
            })?;
            let mut near: Vec<(u32, i64)> = Vec::new();
            for row in rows {
                near.push(row?);
                if near.len() >= keep * 4 {
                    near.select_nth_unstable(keep);
                    near.truncate(keep);
                }
            }
            Ok(near)
        };
    let (low, high): (Option<i64>, Option<i64>) =
        store
            .conn
            .query_row("SELECT min(file_id), max(file_id) FROM chunks", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
    let mut near = match (low, high) {
        (Some(low), Some(high)) if threads > 1 && high > low => {
            let path = store.path()?;
            let width = (high - low) / threads as i64 + 1;
            let parts: Vec<Result<Vec<(u32, i64)>>> = std::thread::scope(|scope| {
                let workers: Vec<_> = (0..threads as i64)
                    .map(|n| {
                        let (path, scan) = (&path, &scan);
                        scope.spawn(move || {
                            let conn = rusqlite::Connection::open_with_flags(
                                path,
                                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                            )?;
                            scan(&conn, Some((low + n * width, low + (n + 1) * width - 1)))
                        })
                    })
                    .collect();
                workers
                    .into_iter()
                    .map(|w| {
                        w.join()
                            .unwrap_or_else(|_| Err(anyhow::anyhow!("a scan thread panicked")))
                    })
                    .collect()
            });
            let mut all = Vec::new();
            for part in parts {
                all.extend(part?);
            }
            all
        }
        _ => scan(&store.conn, None)?,
    };
    near.sort_unstable();
    near.truncate(keep);
    Ok(near)
}

/// Cosine matches, best first.
fn vector(
    store: &Store,
    query: &[f32],
    pool: usize,
    exact_limit: usize,
) -> Result<Vec<(i64, f64)>> {
    let total = store.counts()?.1 as usize;
    let mut scored: Vec<(i64, f64)> = Vec::new();
    if total <= exact_limit {
        let mut stmt = store.conn.prepare("SELECT id, scale, vec FROM chunks")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, f32>(1)?,
                r.get::<_, Vec<u8>>(2)?,
            ))
        })?;
        for row in rows {
            let (id, scale, bytes) = row?;
            scored.push((
                id,
                f64::from(vecs::dot(query, scale, &vecs::bytes_i8(&bytes))),
            ));
        }
    } else {
        let qcode = vecs::code(query);
        let keep = (pool * 20).max(1000);
        let threads = if total >= PARALLEL_SCAN_FROM {
            std::thread::available_parallelism().map_or(1, |n| n.get().min(MAX_SCAN_THREADS))
        } else {
            1
        };
        let near = nearest_codes(store, &qcode, keep, threads)?;
        let mut stmt = store
            .conn
            .prepare("SELECT scale, vec FROM chunks WHERE id = ?1")?;
        for (_, id) in near {
            let (scale, bytes): (f32, Vec<u8>) =
                stmt.query_row([id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            scored.push((
                id,
                f64::from(vecs::dot(query, scale, &vecs::bytes_i8(&bytes))),
            ));
        }
    }
    scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    scored.truncate(pool);
    Ok(scored)
}

/// The hit for a chunk, or `None` when its file has a newer copy. The newest copy of a file lists
/// the older ones in `versions`, newest first.
fn chunk_hit(source: &Opened, id: i64, score: f64) -> Result<Option<Hit>> {
    let (hidden, mut hit) = source.store.conn.query_row(
        "SELECT f.path, f.title, c.heading, f.date, f.kind, c.text, f.status, f.superseded_by, f.amended_by, \
         EXISTS(SELECT 1 FROM files n WHERE n.path = f.dup_of) \
         FROM chunks c JOIN files f ON f.id = c.file_id WHERE c.id = ?1",
        [id],
        |r| {
            let path: String = r.get(0)?;
            let some = |s: String| (!s.is_empty()).then_some(s);
            Ok((
                r.get::<_, bool>(9)?,
                Hit {
                    source: source.label.clone(),
                    file: source.root.join(&path).to_string_lossy().into_owned(),
                    path,
                    title: r.get(1)?,
                    heading: r.get(2)?,
                    date: r.get(3)?,
                    kind: r.get(4)?,
                    status: some(r.get(6)?),
                    superseded_by: some(r.get(7)?),
                    amended_by: r
                        .get::<_, String>(8)?
                        .split('\n')
                        .filter(|p| !p.is_empty())
                        .map(String::from)
                        .collect(),
                    score,
                    snippet: snippet(&r.get::<_, String>(5)?),
                    versions: Vec::new(),
                },
            ))
        },
    )?;
    if hidden {
        return Ok(None);
    }
    let mut stmt = source
        .store
        .conn
        .prepare("SELECT path FROM files WHERE dup_of = ?1 ORDER BY date DESC, path DESC")?;
    hit.versions = stmt
        .query_map([&hit.path], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(Some(hit))
}

/// A file's first chunk as a hit, for a file that comes along with another; none if it isn't
/// indexed.
fn file_hit(source: &Opened, path: &str, score: f64) -> Result<Option<Hit>> {
    let id: Option<i64> = source
        .store
        .conn
        .query_row(
            "SELECT c.id FROM chunks c JOIN files f ON f.id = c.file_id WHERE f.path = ?1 ORDER BY c.ord LIMIT 1",
            [path],
            |r| r.get(0),
        )
        .optional()?;
    match id {
        Some(id) => chunk_hit(source, id, score),
        None => Ok(None),
    }
}

/// Breaks `text` at spaces into lines of at most `width` characters (a longer word gets its own line).
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(line) if line.chars().count() + 1 + word.chars().count() <= width => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines
}

fn snippet(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= SNIPPET_CHARS {
        return flat;
    }
    let cut: String = flat.chars().take(SNIPPET_CHARS).collect();
    format!("{}…", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{embed::Fake, index};
    use std::fs;
    use tempfile::TempDir;

    /// An index of `files`, as one outside source called `label`, under `dir`.
    fn source(dir: &TempDir, label: &str, files: &[(&str, &str)]) -> Opened {
        let root = dir.path().join(label);
        fs::create_dir_all(&root).unwrap();
        for (p, t) in files {
            let full = root.join(p);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, t).unwrap();
        }
        let source = Source::outside(label, &root);
        let mut store =
            Store::create(&dir.path().join(format!("idx-{label}/index.sqlite")), &root).unwrap();
        index::index(&source, &mut store, &mut Fake, false).unwrap();
        Opened::new(&source, store)
    }

    fn brain(files: &[(&str, &str)]) -> (TempDir, Vec<Opened>) {
        let dir = TempDir::new().unwrap();
        let s = source(&dir, "brain", files);
        (dir, vec![s])
    }

    fn paths(hits: &[Hit]) -> Vec<&str> {
        hits.iter().map(|h| h.path.as_str()).collect()
    }

    fn ask(s: &[Opened], question: &str, k: usize, mode: Mode) -> Vec<Hit> {
        search(s, &mut Fake, &Query { question, k, mode }, EXACT_LIMIT).unwrap()
    }

    /// `ask`, with every word counting as specific however many chunks it matches.
    fn ask_every_word(s: &[Opened], question: &str, k: usize, mode: Mode) -> Vec<Hit> {
        let q = Query { question, k, mode };
        run(s, &mut Fake, &q, EXACT_LIMIT, 100).unwrap()
    }

    const DOCS: &[(&str, &str)] = &[
        ("pets.md", "# Pets\n\nDogs and cats are welcome on Fridays."),
        ("leave.md", "# Leave\n\nTwenty five days of paid holiday."),
        (
            "ids.md",
            "# IDs\n\nInvoice INV-2024-0042 was paid. Sibling INV-2024-0043 is open.",
        ),
        ("ids2.md", "# Other\n\nSomething about invoices in general."),
    ];

    #[test]
    fn a_question_becomes_its_words_and_its_exact_tokens() {
        let w = full_text("Who paid invoice INV-2024-0042 and how much?");
        assert_eq!(w.exact(), ["\"INV-2024-0042\""]);
        for word in ["\"who\"", "\"paid\"", "\"inv\"", "\"2024\"", "\"0042\"", "\"much\""] {
            assert!(w.any.contains(&word.to_string()), "{:?}", w.any);
        }
        // Names after the first word, a run of them as one phrase; a plain question has none.
        let w = full_text("Where did Guðmundur Ólafsson work before Hrafn?");
        assert_eq!(w.names, ["\"Guðmundur Ólafsson\"", "\"Hrafn\""]);
        assert!(full_text("Can I bring my dog to work?").exact().is_empty());
        // Quoted phrases and acronyms.
        let w = full_text("What is our \"back out\" rule for the ADR?");
        assert_eq!(w.exact(), ["\"back out\"", "\"ADR\""]);
        // Words are quoted, so full-text syntax in a question is only text.
        let w = full_text("cats AND (dogs OR NOT birds) *");
        assert_eq!(
            w.any,
            ["\"cats\"", "\"and\"", "\"dogs\"", "\"or\"", "\"not\"", "\"birds\""]
        );
        assert!(full_text("?!").any.is_empty());
    }

    #[test]
    fn a_word_most_passages_share_is_left_to_the_by_meaning_half() {
        // `pay` is in all 30 passages, `zebrafish` in one: with 30 chunks a term may match 10.
        let files: Vec<(String, String)> = (0..30)
            .map(|n| {
                let extra = if n == 7 { " zebrafish" } else { "" };
                (format!("d{n:02}.md"), format!("# Doc {n}\n\nPay rules number {n}{extra}."))
            })
            .collect();
        let refs: Vec<(&str, &str)> = files.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
        let (_d, s) = brain(&refs);
        let terms = |q: &str| specific(&s[0].store, &full_text(q).any, 1).unwrap();
        assert_eq!(terms("pay zebrafish"), "\"zebrafish\"");
        assert_eq!(terms("pay rules"), "");
        // So a question of only common words finds nothing by words; by meaning it still ranks.
        assert!(ask(&s, "pay rules", 5, Mode::FullText).is_empty());
        assert_eq!(ask(&s, "pay rules", 5, Mode::Both).len(), 5);
        // And one rare word among them is what the full-text half searches for.
        assert_eq!(paths(&ask(&s, "pay zebrafish", 5, Mode::FullText)), ["d07.md"]);
    }

    #[test]
    fn the_code_scan_gives_the_same_nearest_chunks_on_any_number_of_threads() {
        let names: Vec<String> = (0..60).map(|n| format!("doc-{n:02}.md")).collect();
        let texts: Vec<String> = (0..60)
            .map(|n| {
                format!(
                    "# Doc {n}\n\nWords about topic {} and subject {}.",
                    n % 7,
                    n % 11
                )
            })
            .collect();
        let files: Vec<(&str, &str)> = names
            .iter()
            .zip(&texts)
            .map(|(n, t)| (n.as_str(), t.as_str()))
            .collect();
        let (_dir, s) = brain(&files);
        let qcode = vecs::code(&Fake.embed_query("topic 3 and subject 4").unwrap());
        let one = nearest_codes(&s[0].store, &qcode, 20, 1).unwrap();
        assert_eq!(one.len(), 20);
        for threads in [2, 4, 7] {
            assert_eq!(
                nearest_codes(&s[0].store, &qcode, 20, threads).unwrap(),
                one
            );
        }
    }

    #[test]
    fn full_text_finds_words_and_odd_ids_in_a_question() {
        let (_d, s) = brain(DOCS);
        let found = |q: &str| {
            paths(&ask(&s, q, 5, Mode::FullText))
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>()
        };
        assert_eq!(found("How much holiday is there?")[0], "leave.md");
        assert_eq!(found("What happened to INV-2024-0042?")[0], "ids.md");
        assert!(ask(&s, "nonexistentword", 5, Mode::FullText).is_empty());
        assert!(search(
            &s,
            &mut Fake,
            &Query {
                question: "  ",
                k: 5,
                mode: Mode::Both
            },
            EXACT_LIMIT
        )
        .is_err());
    }

    #[test]
    fn meaning_ranks_by_vector_similarity() {
        let (_d, s) = brain(DOCS);
        let hits = ask(&s, "paid holiday days", 2, Mode::Meaning);
        assert_eq!(hits[0].path, "leave.md");
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn the_one_bit_first_pass_agrees_with_exact_scoring_on_the_top_hit() {
        let (_d, s) = brain(DOCS);
        let q = Query {
            question: "paid holiday days",
            k: 1,
            mode: Mode::Meaning,
        };
        let exact = search(&s, &mut Fake, &q, EXACT_LIMIT).unwrap();
        let coded = search(&s, &mut Fake, &q, 0).unwrap();
        assert_eq!(paths(&exact), paths(&coded));
    }

    #[test]
    fn both_halves_fuse_by_rank_and_return_one_hit_per_file() {
        let (_d, s) = brain(&[
            (
                "long.md",
                "# One\n\nalpha beta gamma\n\n# Two\n\nalpha beta delta\n",
            ),
            ("other.md", "# Other\n\nzeta eta theta"),
        ]);
        let hits = ask(&s, "alpha beta", 5, Mode::Both);
        assert_eq!(paths(&hits).iter().filter(|p| **p == "long.md").count(), 1);
        assert_eq!(hits[0].path, "long.md");
        assert!(hits[0].score > 0.0);
    }

    #[test]
    fn one_file_with_many_matching_chunks_does_not_hide_the_others() {
        let filler = |tag: &str| {
            (0..70)
                .map(|i| format!("{tag}{i}"))
                .collect::<Vec<_>>()
                .join(" ")
        };
        let mut big = String::new();
        for i in 0..80 {
            big.push_str(&format!(
                "# S{i}\n\nbanana banana banana {}\n\n",
                filler(&format!("f{i}_"))
            ));
        }
        let (_d, s) = brain(&[
            ("big.md", &big),
            ("b.md", &format!("# B\n\nbanana {}", filler("b_"))),
            ("c.md", &format!("# C\n\nbanana {}", filler("c_"))),
        ]);
        // The long file's 80 chunks all rank above the others', and fill the first pool of 50.
        let first = fts(&s[0].store, "\"banana\"", 50).unwrap();
        let in_big = |id: i64| chunk_hit(&s[0], id, 0.0).unwrap().unwrap().path == "big.md";
        assert!(first.iter().all(|(id, _)| in_big(*id)));
        let mut found = paths(&ask_every_word(&s, "banana", 3, Mode::FullText))
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
        found.sort();
        assert_eq!(found, ["b.md", "big.md", "c.md"]);
        // And the same through the vector half.
        let first = vector(
            &s[0].store,
            &Fake.embed_query("banana").unwrap(),
            50,
            EXACT_LIMIT,
        )
        .unwrap();
        assert!(first.iter().all(|(id, _)| in_big(*id)));
        assert_eq!(ask(&s, "banana", 3, Mode::Meaning).len(), 3);
    }

    #[test]
    fn hits_have_the_settled_fields_and_name_their_source() {
        let (_d, s) = brain(DOCS);
        let hit = &ask(&s, "holiday", 1, Mode::Both)[0];
        assert_eq!((hit.title.as_str(), hit.kind.as_str()), ("Leave", "raw"));
        assert_eq!(hit.source, "brain");
        assert!(hit.file.ends_with("brain/leave.md") && hit.file.starts_with('/'));
        assert!(hit.snippet.contains("holiday") && hit.status.is_none() && hit.versions.is_empty());
        let json = serde_json::to_value(hit).unwrap();
        for k in [
            "source", "path", "file", "title", "heading", "date", "kind", "status", "score",
            "snippet", "versions",
        ] {
            assert!(json.get(k).is_some(), "{k}");
        }
    }

    #[test]
    fn a_search_covers_every_source_and_the_same_path_in_two_is_two_hits() {
        let dir = TempDir::new().unwrap();
        let a = source(
            &dir,
            "a",
            &[
                ("README.md", "# Readme\n\nThe tangerine rota is on Mondays."),
                ("fruit.md", "# Fruit\n\nBananas are yellow."),
            ],
        );
        let b = source(
            &dir,
            "b",
            &[
                (
                    "README.md",
                    "# Readme\n\nThe tangerine rota is on Thursdays.",
                ),
                ("other.md", "# Other\n\nNothing relevant here."),
            ],
        );
        let both = [a, b];
        let hits = ask(&both, "tangerine", 5, Mode::FullText);
        let found: Vec<(&str, &str)> = hits
            .iter()
            .map(|h| (h.source.as_str(), h.path.as_str()))
            .collect();
        assert_eq!(found.len(), 2);
        assert!(found.contains(&("a", "README.md")) && found.contains(&("b", "README.md")));
        assert_ne!(hits[0].file, hits[1].file);
        // Alone, a source returns only its own files.
        let alone = ask(&both[1..], "tangerine", 5, Mode::FullText);
        assert_eq!(alone.len(), 1);
        assert_eq!(alone[0].source, "b");
    }

    #[test]
    fn matches_cut_by_the_merged_pool_are_looked_for_again() {
        // 40 chunks of the same word in each of two big files; the third file, in the first
        // source, matches once, and sits below the merged pool of 50.
        let section = |tag: &str| -> String {
            let filler = (0..70)
                .map(|i| format!("{tag}{i}"))
                .collect::<Vec<_>>()
                .join(" ");
            (0..40)
                .map(|i| format!("# S{i}\n\nbanana banana banana {filler}\n\n"))
                .collect()
        };
        let dir = TempDir::new().unwrap();
        let a = source(
            &dir,
            "a",
            &[
                ("big.md", &section("a")),
                (
                    "small.md",
                    &format!(
                        "# Small\n\nbanana {}",
                        (0..70)
                            .map(|i| format!("s{i}"))
                            .collect::<Vec<_>>()
                            .join(" ")
                    ),
                ),
            ],
        );
        let b = source(&dir, "b", &[("big.md", &section("b"))]);
        let hits = ask_every_word(&[a, b], "banana", 3, Mode::FullText);
        let mut found: Vec<(&str, &str)> = hits
            .iter()
            .map(|h| (h.source.as_str(), h.path.as_str()))
            .collect();
        found.sort();
        assert_eq!(found, [("a", "big.md"), ("a", "small.md"), ("b", "big.md")]);
    }

    #[test]
    fn two_sources_with_the_same_label_stay_two_sources() {
        let (da, db) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        let mut both = Vec::new();
        for d in [&da, &db] {
            both.push(source(
                d,
                "project",
                &[("README.md", "# R\n\nThe tangerine rota.")],
            ));
        }
        let hits = ask(&both, "tangerine", 5, Mode::FullText);
        assert_eq!(hits.len(), 2);
        assert_ne!(hits[0].file, hits[1].file);
    }

    #[test]
    fn a_source_with_nothing_relevant_does_not_push_down_the_one_that_has_it() {
        let dir = TempDir::new().unwrap();
        let a = source(
            &dir,
            "a",
            &[
                ("one.md", "# One\n\nHolidays are twenty five days."),
                ("two.md", "# Two\n\nSomething about holiday pay."),
                ("three.md", "# Three\n\nA holiday calendar exists."),
            ],
        );
        let b = source(&dir, "b", &[("x.md", "# X\n\nCats sit on mats.")]);
        let hits = ask(&[b, a], "how many holiday days", 3, Mode::Meaning);
        assert_eq!(hits[0].source, "a", "{:?}", paths(&hits));
    }

    #[test]
    fn wrap_breaks_at_spaces_and_keeps_every_word() {
        let lines = wrap("aaa bbb ccc ddd eee", 8);
        assert_eq!(lines, ["aaa bbb", "ccc ddd", "eee"]);
        assert_eq!(
            wrap("averyveryverylongword x", 5),
            ["averyveryverylongword", "x"]
        );
        assert!(wrap("   ", 10).is_empty());
    }

    #[test]
    fn long_snippets_are_cut_at_a_word() {
        let s = snippet(&"word ".repeat(400));
        assert!(s.ends_with("word…") && s.chars().count() <= SNIPPET_CHARS + 1);
    }

    /// A project whose docs hold ADRs, indexed as the project source.
    fn adrs(files: &[(&str, &str)]) -> (TempDir, Vec<Opened>) {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("courier-app");
        for (p, t) in files {
            let full = root.join(p);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, t).unwrap();
        }
        let source = Source {
            label: crate::sources::PROJECT.into(),
            kind: crate::sources::Kind::Project {
                docs: "docs".into(),
            },
            root: root.clone(),
            exists: true,
        };
        let mut store = Store::create(&dir.path().join("idx/index.sqlite"), &root).unwrap();
        index::index(&source, &mut store, &mut Fake, false).unwrap();
        let opened = vec![Opened::new(&source, store)];
        (dir, opened)
    }

    #[test]
    fn a_superseded_adr_ranks_after_what_replaced_it_and_says_so() {
        let (_d, s) = adrs(&[
            ("docs/adr/0001-a.md", "# 0001: Fax\n\nStatus: superseded by [0003](0003-c.md)\n\nThe contact channel is fax. Fax fax fax."),
            ("docs/adr/0002-b.md", "# 0002: Chat\n\nStatus: accepted\n\nThe contact channel could be chat."),
            ("docs/adr/0003-c.md", "# 0003: Email\n\nStatus: accepted\n\nThe contact channel is email."),
        ]);
        let hits = ask(&s, "fax contact channel", 5, Mode::FullText);
        let found = paths(&hits);
        let at = |p: &str| found.iter().position(|f| *f == p).unwrap();
        assert!(
            at("docs/adr/0003-c.md") < at("docs/adr/0001-a.md"),
            "{found:?}"
        );
        assert!(
            at("docs/adr/0002-b.md") < at("docs/adr/0001-a.md"),
            "{found:?}"
        );
        let old = &hits[at("docs/adr/0001-a.md")];
        assert_eq!(old.status.as_deref(), Some("superseded"));
        assert_eq!(old.superseded_by.as_deref(), Some("docs/adr/0003-c.md"));
        // With room for one hit only, the superseded ADR that matches best still brings its
        // replacement, ahead of it.
        let (_d, s) = adrs(&[
            (
                "docs/adr/0001-a.md",
                "# 0001: Fax\n\nStatus: superseded by 0003\n\nFax fax fax fax.",
            ),
            (
                "docs/adr/0003-c.md",
                "# 0003: Email\n\nStatus: accepted\n\nThe channel is email.",
            ),
        ]);
        let found = ask(&s, "fax", 1, Mode::FullText);
        assert_eq!(paths(&found), ["docs/adr/0003-c.md", "docs/adr/0001-a.md"]);
    }

    #[test]
    fn an_amended_adr_comes_with_its_amendments() {
        let (_d, s) = adrs(&[
            ("docs/adr/0002-a.md", "# 0002: Currencies\n\nStatus: accepted; amended by [0004](0004-b.md)\n\nStored in ISK and EUR."),
            ("docs/adr/0004-b.md", "# 0004: Add NOK\n\nStatus: accepted; amended by [0005](0005-c.md)\n\nNOK as well."),
            ("docs/adr/0005-c.md", "# 0005: NOK rounding\n\nStatus: accepted\n\nRounded at the edge."),
            ("docs/adr/0001-o.md", "# 0001: Old\n\nStatus: superseded by 0002\n\nStored in ISK only."),
            ("docs/PRODUCT.md", "# Product\n\nAmounts in ISK."),
        ]);
        let hits = ask(&s, "stored in ISK and EUR", 2, Mode::FullText);
        let paths: Vec<&str> = hits.iter().map(|h| h.path.as_str()).collect();
        let at = paths
            .iter()
            .position(|p| *p == "docs/adr/0002-a.md")
            .unwrap();
        assert_eq!(hits[at].kind, "adr");
        assert_eq!(hits[at].amended_by, ["docs/adr/0004-b.md"]);
        assert_eq!(paths[at + 1], "docs/adr/0004-b.md", "{paths:?}");
        // The amendment was amended too, and comes along.
        assert_eq!(paths[at + 2], "docs/adr/0005-c.md", "{paths:?}");
    }

    #[test]
    fn a_file_with_a_newer_copy_never_ranks_and_the_copy_lists_it() {
        let policy = "Staff are entitled to twenty five days of paid leave each calendar year in addition to public holidays. \
            Leave is agreed with the team lead at least two weeks ahead, and no more than ten working days may be taken in a row without \
            approval from the head of department. Unused days carry over until the end of May the following year, after which they expire. \
            Sick leave is recorded separately and does not reduce the annual entitlement.";
        let (_dir, s) = brain(&[
            (
                "leave/v1.md",
                &format!("---\ndate: 2023-01-10\n---\n{policy}"),
            ),
            (
                "leave/v2.md",
                &format!(
                    "---\ndate: 2023-06-01\n---\n{}",
                    policy.replace("twenty five", "thirty")
                ),
            ),
            (
                "leave/final.md",
                &format!(
                    "---\ndate: 2024-02-02\n---\n{}",
                    policy
                        .replace("twenty five", "thirty")
                        .replace("May", "June")
                ),
            ),
            (
                "other.md",
                "The office plants are watered on Fridays, and leave nothing to chance.",
            ),
        ]);
        for mode in [Mode::FullText, Mode::Both] {
            let hits = ask(&s, "how many days of leave", 5, mode);
            let paths: Vec<&str> = hits.iter().map(|h| h.path.as_str()).collect();
            assert_eq!(
                paths.iter().filter(|p| p.starts_with("leave/")).count(),
                1,
                "{paths:?}"
            );
            let newest = hits.iter().find(|h| h.path == "leave/final.md").unwrap();
            assert_eq!(newest.versions, ["leave/v2.md", "leave/v1.md"]);
            assert!(hits
                .iter()
                .filter(|h| h.path != "leave/final.md")
                .all(|h| h.versions.is_empty()));
        }
        // Asking for more files than there are fills the list from the files that remain.
        assert_eq!(ask(&s, "leave plants", 10, Mode::FullText).len(), 2);
    }
}
