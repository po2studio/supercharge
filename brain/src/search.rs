//! Hybrid search: BM25 over FTS5 and cosine over the vectors, fused by rank (RRF), across the
//! index of each source a project reads.

use anyhow::{bail, Context, Result};
use rusqlite::{types::Value, OptionalExtension};
use serde::Serialize;
use std::{collections::HashMap, path::PathBuf};

use crate::{doc, embed::Embedder, notes, sources, sources::Source, store, store::Store, vecs};

/// The reciprocal-rank-fusion constant.
const RRF_K: f64 = 60.0;
/// Up to this many chunks the vector search scores every vector exactly; beyond it the 1-bit
/// codes pick the candidates to rescore.
pub const EXACT_LIMIT: usize = 50_000;
const SNIPPET_CHARS: usize = 600;

#[derive(Debug, Serialize)]
pub struct Hit {
    /// Unique across sources: the source's index, then the chunk.
    pub id: String,
    /// `project` for the project's own docs, else the source's path as it was listed.
    pub source: String,
    /// Relative to the source's folder (the project's root, for the project).
    pub path: String,
    /// Absolute, to read the whole file.
    pub file: String,
    pub title: String,
    pub heading: String,
    pub date: String,
    /// `business`, `product`, `process` or `decision` for a decision, `adr` for a project's ADR,
    /// `note`, else `raw`.
    pub kind: String,
    /// The file's sha256: what a note records of each file it is made from.
    pub hash: String,
    /// `proposed`, `accepted` or `superseded` for a decision or ADR, else none.
    pub status: Option<String>,
    /// The decision's project, when its frontmatter names one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// For a superseded decision or ADR: what replaced it (a path in the same source, else as written).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<String>,
    /// For an amended ADR: the ADRs that amend it, which come with it in the results.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub amended_by: Vec<String>,
    /// For a note: a file it was made from has changed or is gone.
    pub stale: bool,
    pub score: f64,
    pub versions: Vec<String>,
    pub snippet: String,
}

pub struct Query<'a> {
    pub terms: Option<&'a str>,
    pub about: Option<&'a str>,
    pub k: usize,
}

/// What a search leaves out. Superseded decisions and ADRs are left out unless `all`, or unless
/// `status` asks for them.
#[derive(Debug, Default)]
pub struct Filter<'a> {
    /// One of `KINDS`; `decision` is every kind of decision, ADRs included.
    pub kind: Option<&'a str>,
    /// A decision's `project`; a project's own docs count as that project's.
    pub project: Option<&'a str>,
    /// `YYYY-MM-DD`: only entries dated then or later.
    pub since: Option<&'a str>,
    /// `proposed`, `accepted` or `superseded`.
    pub status: Option<&'a str>,
    pub all: bool,
}

pub const KINDS: [&str; 7] = [
    "decision", "business", "product", "process", "adr", "note", "raw",
];
const DECISION_KINDS: [&str; 5] = ["business", "product", "process", "decision", "adr"];
const STATUSES: [&str; 3] = ["proposed", "accepted", "superseded"];

impl Filter<'_> {
    fn check(&self) -> Result<()> {
        if let Some(k) = self.kind.filter(|k| !KINDS.contains(k)) {
            bail!("--kind {k:?} is none of decision, business, product, process, adr, note, raw");
        }
        if let Some(s) = self.status.filter(|s| !STATUSES.contains(s)) {
            bail!("--status {s:?} is none of proposed, accepted, superseded");
        }
        if let Some(d) = self
            .since
            .filter(|d| doc::valid_date(d).is_none_or(|v| v != *d))
        {
            bail!("--since {d:?} is not a date like 2025-03-01");
        }
        Ok(())
    }

    fn hides_superseded(&self) -> bool {
        !self.all && self.status != Some("superseded")
    }

    /// Which of a source's files may be hit, or none when every file may. `project_default` is
    /// the project a file of the project's own docs counts as.
    fn scope(&self, project_default: &str, is_project: bool) -> Option<Scope> {
        let mut conds: Vec<String> = Vec::new();
        let mut values: Vec<Value> = Vec::new();
        let text = |s: &str| Value::Text(s.to_string());
        if let Some(kind) = self.kind {
            let kinds: Vec<&str> = if kind == "decision" {
                DECISION_KINDS.to_vec()
            } else {
                vec![kind]
            };
            conds.push(format!("kind IN ({})", vec!["?"; kinds.len()].join(", ")));
            values.extend(kinds.into_iter().map(text));
        }
        if let Some(project) = self.project {
            if is_project {
                conds.push("COALESCE(NULLIF(project, ''), ?) = ?".into());
                values.push(text(project_default));
            } else {
                conds.push("project = ?".into());
            }
            values.push(text(project));
        }
        if let Some(since) = self.since {
            conds.push("date >= ?".into());
            values.push(text(since));
        }
        if let Some(status) = self.status {
            conds.push("status = ?".into());
            values.push(text(status));
        }
        if self.hides_superseded() {
            if conds.is_empty() {
                return Some(Scope {
                    except: true,
                    cond: "status = 'superseded'".into(),
                    values,
                });
            }
            conds.push("status != 'superseded'".into());
        }
        (!conds.is_empty()).then(|| Scope {
            except: false,
            cond: conds.join(" AND "),
            values,
        })
    }
}

/// The files of one source a search may hit, as SQL over `files`. With nothing but the
/// superseded left out, `except` is set and `cond` names those few, not all the others.
struct Scope {
    except: bool,
    cond: String,
    values: Vec<Value>,
}

impl Scope {
    fn not(&self) -> &'static str {
        if self.except {
            "NOT "
        } else {
            ""
        }
    }

    /// For a query over `chunks`.
    fn on_chunks(&self) -> String {
        format!(
            "file_id {}IN (SELECT id FROM files WHERE {})",
            self.not(),
            self.cond
        )
    }

    /// For a query over `fts`, whose rowids are chunk ids.
    fn on_fts(&self) -> String {
        format!(
            "rowid {}IN (SELECT id FROM chunks WHERE file_id IN (SELECT id FROM files WHERE {}))",
            self.not(),
            self.cond
        )
    }
}

/// A source's index, open for reading.
pub struct Opened {
    pub label: String,
    pub root: PathBuf,
    is_project: bool,
    tag: String,
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
            tag: store::key_hash(&source.key())[..8].to_string(),
            store,
        }
    }
}

/// A chunk in one of the opened sources: its position in the list, and its row.
type Key = (usize, i64);

/// Hybrid search with nothing filtered out but the superseded.
pub fn search(
    sources: &[Opened],
    embedder: &mut dyn Embedder,
    q: &Query,
    exact_limit: usize,
) -> Result<Vec<Hit>> {
    search_filtered(sources, embedder, q, &Filter::default(), exact_limit)
}

/// An accepted decision ranks one place above where relevance puts it, and above a hit it ties
/// with. A fresh note takes the same lift, below a decision it ties with: the trust order is
/// decisions, fresh notes, raw files, stale notes, with the superseded, shown only on request,
/// after all of them. More than one place puts decisions that merely resemble a question above
/// the file that answers it: two cost 0.03 MRR on the eval.
const DECISION_LIFT: usize = 1;

pub fn search_filtered(
    sources: &[Opened],
    embedder: &mut dyn Embedder,
    q: &Query,
    filter: &Filter,
    exact_limit: usize,
) -> Result<Vec<Hit>> {
    if q.terms.is_none() && q.about.is_none() {
        bail!("give --terms, --about, or both");
    }
    filter.check()?;
    let project_default = match filter.project {
        Some(_) => sources
            .iter()
            .find(|s| s.is_project)
            .map(|s| sources::project_name(&s.root))
            .unwrap_or_default(),
        None => String::new(),
    };
    let scopes: Vec<Option<Scope>> = sources
        .iter()
        .map(|s| filter.scope(&project_default, s.is_project))
        .collect();
    let query_vector = q.about.map(|t| embedder.embed_query(t)).transpose()?;
    // One hit per file, its best chunk. A file with many matching chunks can fill the candidate
    // pool on its own, so the pool grows until it yields enough files or holds every match.
    // Candidates for the top `k`: those a lift can bring in.
    let window = q.k + DECISION_LIFT;
    let notes = notes::Ctx::new(sources);
    let mut pool = (q.k * 10).max(50);
    loop {
        let mut exhausted = true;
        let by_terms = match q.terms {
            Some(t) => Some(merge(
                sources,
                &scopes,
                pool,
                &mut exhausted,
                |s, scope| fts(&s.store, scope, t, pool),
            )?),
            None => None,
        };
        let by_about = match &query_vector {
            Some(v) => Some(merge(
                sources,
                &scopes,
                pool,
                &mut exhausted,
                |s, scope| vector(&s.store, scope, v, pool, exact_limit),
            )?),
            None => None,
        };
        let ranked: Vec<(Key, f64)> = match (by_terms, by_about) {
            (Some(t), Some(a)) => fuse(&[t, a]),
            (Some(l), None) | (None, Some(l)) => l,
            (None, None) => unreachable!(),
        };
        let mut hits: Vec<(usize, Hit)> = Vec::new();
        // Superseded hits (with `--all`) and stale notes go last, so they don't count towards the
        // window, unless the superseded are what was asked for.
        let mut current = 0;
        let mut seen: std::collections::HashSet<(usize, String)> = Default::default();
        for ((n, id), score) in ranked {
            // A file that has a newer copy never ranks: the copy does, and lists it.
            let Some(mut hit) = chunk_hit(&sources[n], id, score)? else {
                continue;
            };
            // One hit per file of a source; the label is for people, so two sources are told
            // apart by position, not by name.
            if seen.insert((n, hit.path.clone())) {
                if hit.kind == "note" {
                    hit.stale = notes.is_stale(n, &hit.path)?;
                }
                current += usize::from(
                    !hit.stale
                        && (hit.status.as_deref() != Some("superseded")
                            || filter.status == Some("superseded")),
                );
                hits.push((n, hit));
            }
            if current == window {
                break;
            }
        }
        if current == window || exhausted {
            return trusted(sources, hits, q.k, filter);
        }
        pool *= 4;
    }
}

fn is_accepted_decision(h: &Hit) -> bool {
    h.status.as_deref() == Some("accepted") && DECISION_KINDS.contains(&h.kind.as_str())
}

/// The best `k` of the hits (most relevant first) by the trust order; each amended ADR is
/// followed by the ADRs that amend it, and theirs in turn, if they weren't hit already.
fn trusted(
    sources: &[Opened],
    hits: Vec<(usize, Hit)>,
    k: usize,
    filter: &Filter,
) -> Result<Vec<Hit>> {
    let mut ordered: Vec<_> = hits
        .into_iter()
        .enumerate()
        .map(|(rank, hit)| {
            let key = if hit.1.status.as_deref() == Some("superseded") {
                (usize::MAX, 0, rank)
            } else if hit.1.stale {
                (usize::MAX - 1, 0, rank)
            } else if is_accepted_decision(&hit.1) {
                (rank.saturating_sub(DECISION_LIFT), 0, rank)
            } else if hit.1.kind == "note" {
                (rank.saturating_sub(DECISION_LIFT), 1, rank)
            } else {
                (rank, 2, rank)
            };
            (key, hit)
        })
        .collect();
    ordered.sort_by_key(|(key, _)| *key);
    ordered.truncate(k);
    let hits: Vec<(usize, Hit)> = ordered.into_iter().map(|(_, h)| h).collect();
    let mut seen: std::collections::HashSet<(usize, String)> =
        hits.iter().map(|(n, h)| (*n, h.path.clone())).collect();
    let mut out = Vec::new();
    for (n, hit) in hits {
        let mut queue: std::collections::VecDeque<String> =
            hit.amended_by.iter().cloned().collect();
        let score = hit.score;
        out.push(hit);
        // An amendment can be amended in turn; `seen` also stops a cycle.
        while let Some(path) = queue.pop_front() {
            if !seen.insert((n, path.clone())) {
                continue;
            }
            if let Some(extra) = file_hit(&sources[n], &path, score)? {
                if extra.status.as_deref() == Some("superseded") && filter.hides_superseded() {
                    continue;
                }
                queue.extend(extra.amended_by.iter().cloned());
                out.push(extra);
            }
        }
    }
    Ok(out)
}

/// Each source's best `pool` matches by `one`, as one list by score: cosine is the same measure
/// in every index, and BM25 close enough to it for a source's words to rank beside another's.
/// `exhausted` goes false when some source had more matches than it returned.
fn merge(
    sources: &[Opened],
    scopes: &[Option<Scope>],
    pool: usize,
    exhausted: &mut bool,
    one: impl Fn(&Opened, &Option<Scope>) -> Result<Vec<(i64, f64)>>,
) -> Result<Vec<(Key, f64)>> {
    let mut all: Vec<(Key, f64)> = Vec::new();
    for (n, source) in sources.iter().enumerate() {
        let list = one(source, &scopes[n])?;
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

fn fuse(lists: &[Vec<(Key, f64)>]) -> Vec<(Key, f64)> {
    let mut score: HashMap<Key, f64> = HashMap::new();
    for list in lists {
        for (rank, (key, _)) in list.iter().enumerate() {
            *score.entry(*key).or_insert(0.0) += 1.0 / (RRF_K + rank as f64 + 1.0);
        }
    }
    let mut out: Vec<(Key, f64)> = score.into_iter().collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    out
}

/// Full-text matches, best first, with `-bm25` as the score. The query is FTS5 syntax; when it
/// isn't valid (an ID like `INV-2024-0042` isn't), every word is taken literally.
fn fts(store: &Store, scope: &Option<Scope>, terms: &str, pool: usize) -> Result<Vec<(i64, f64)>> {
    if terms.trim().is_empty() {
        bail!("--terms is empty");
    }
    match fts_query(store, scope, terms, pool) {
        Ok(rows) => Ok(rows),
        Err(_) => fts_query(store, scope, &quote_words(terms), pool)
            .with_context(|| format!("cannot search for {terms:?}")),
    }
}

fn fts_query(
    store: &Store,
    scope: &Option<Scope>,
    query: &str,
    pool: usize,
) -> rusqlite::Result<Vec<(i64, f64)>> {
    let (clause, values) = match scope {
        None => (String::new(), Vec::new()),
        Some(s) => (format!(" AND {}", s.on_fts()), s.values.clone()),
    };
    let mut stmt = store.conn.prepare(&format!(
        "SELECT rowid, -bm25(fts) FROM fts WHERE fts MATCH ?{clause} ORDER BY bm25(fts) LIMIT ?"
    ))?;
    let mut bound = vec![Value::Text(query.to_string())];
    bound.extend(values);
    bound.push(Value::Integer(pool as i64));
    let rows = stmt.query_map(rusqlite::params_from_iter(bound), |r| {
        Ok((r.get(0)?, r.get(1)?))
    })?;
    rows.collect()
}

/// Each word becomes a phrase; AND, OR and NOT stay operators.
fn quote_words(terms: &str) -> String {
    terms
        .split_whitespace()
        .map(|w| {
            if matches!(w, "AND" | "OR" | "NOT") {
                w.to_string()
            } else {
                format!("\"{}\"", w.replace('"', ""))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
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
    filter: &str,
    values: &[Value],
    qcode: &[u8],
    keep: usize,
    threads: usize,
) -> Result<Vec<(u32, i64)>> {
    let scan =
        |conn: &rusqlite::Connection, range: Option<(i64, i64)>| -> Result<Vec<(u32, i64)>> {
            let (clause, mut bound) = match (range, filter.is_empty()) {
                (None, _) => (filter.to_string(), values.to_vec()),
                (Some(_), true) => (" WHERE file_id BETWEEN ? AND ?".to_string(), Vec::new()),
                (Some(_), false) => (
                    format!("{filter} AND file_id BETWEEN ? AND ?"),
                    values.to_vec(),
                ),
            };
            if let Some((from, to)) = range {
                bound.push(Value::Integer(from));
                bound.push(Value::Integer(to));
            }
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
    scope: &Option<Scope>,
    query: &[f32],
    pool: usize,
    exact_limit: usize,
) -> Result<Vec<(i64, f64)>> {
    let total = store.counts()?.1 as usize;
    let mut scored: Vec<(i64, f64)> = Vec::new();
    let (filter, values) = match scope {
        None => (String::new(), Vec::new()),
        Some(s) => (format!(" WHERE {}", s.on_chunks()), s.values.clone()),
    };
    if total <= exact_limit {
        let mut stmt = store
            .conn
            .prepare(&format!("SELECT id, scale, vec FROM chunks{filter}"))?;
        let rows = stmt.query_map(rusqlite::params_from_iter(values), |r| {
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
        let near = nearest_codes(store, &filter, &values, &qcode, keep, threads)?;
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
        "SELECT f.path, f.title, c.heading, f.date, f.kind, c.text, f.project, f.status, f.superseded_by, f.amended_by, \
         EXISTS(SELECT 1 FROM files n WHERE n.path = f.dup_of), f.hash \
         FROM chunks c JOIN files f ON f.id = c.file_id WHERE c.id = ?1",
        [id],
        |r| {
            let path: String = r.get(0)?;
            let some = |s: String| (!s.is_empty()).then_some(s);
            Ok((r.get::<_, bool>(10)?, Hit {
                id: format!("{}-{id}", source.tag),
                source: source.label.clone(),
                file: source.root.join(&path).to_string_lossy().into_owned(),
                path,
                title: r.get(1)?,
                heading: r.get(2)?,
                date: r.get(3)?,
                kind: r.get(4)?,
                hash: r.get(11)?,
                status: some(r.get(7)?),
                project: some(r.get(6)?),
                superseded_by: some(r.get(8)?),
                amended_by: r
                    .get::<_, String>(9)?
                    .split('\n')
                    .filter(|p| !p.is_empty())
                    .map(String::from)
                    .collect(),
                stale: false,
                score,
                snippet: snippet(&r.get::<_, String>(5)?),
                versions: Vec::new(),
            }))
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
        let scope = Filter::default().scope("", false).unwrap();
        let (filter, values) = (
            format!(" WHERE {}", scope.on_chunks()),
            scope.values.clone(),
        );
        let one = nearest_codes(&s[0].store, "", &[], &qcode, 20, 1).unwrap();
        assert_eq!(one.len(), 20);
        for threads in [2, 4, 7] {
            assert_eq!(
                nearest_codes(&s[0].store, "", &[], &qcode, 20, threads).unwrap(),
                one
            );
        }
        // With a filter on the files too.
        let one = nearest_codes(&s[0].store, &filter, &values, &qcode, 20, 1).unwrap();
        assert_eq!(
            nearest_codes(&s[0].store, &filter, &values, &qcode, 20, 4).unwrap(),
            one
        );
    }

    #[test]
    fn terms_find_exact_words_and_odd_ids() {
        let (_d, s) = brain(DOCS);
        let q = |t| Query {
            terms: Some(t),
            about: None,
            k: 5,
        };
        assert_eq!(
            paths(&search(&s, &mut Fake, &q("holiday"), EXACT_LIMIT).unwrap()),
            ["leave.md"]
        );
        assert_eq!(
            paths(&search(&s, &mut Fake, &q("INV-2024-0042"), EXACT_LIMIT).unwrap()),
            ["ids.md"]
        );
        assert_eq!(
            paths(&search(&s, &mut Fake, &q("dogs OR holiday"), EXACT_LIMIT).unwrap()).len(),
            2
        );
        assert!(search(&s, &mut Fake, &q("nonexistentword"), EXACT_LIMIT)
            .unwrap()
            .is_empty());
        assert!(search(&s, &mut Fake, &q("  "), EXACT_LIMIT).is_err());
    }

    #[test]
    fn about_ranks_by_vector_similarity() {
        let (_d, s) = brain(DOCS);
        let q = Query {
            terms: None,
            about: Some("paid holiday days"),
            k: 2,
        };
        let hits = search(&s, &mut Fake, &q, EXACT_LIMIT).unwrap();
        assert_eq!(hits[0].path, "leave.md");
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn the_one_bit_first_pass_agrees_with_exact_scoring_on_the_top_hit() {
        let (_d, s) = brain(DOCS);
        let q = Query {
            terms: None,
            about: Some("paid holiday days"),
            k: 1,
        };
        let exact = search(&s, &mut Fake, &q, EXACT_LIMIT).unwrap();
        let coded = search(&s, &mut Fake, &q, 0).unwrap();
        assert_eq!(paths(&exact), paths(&coded));
    }

    #[test]
    fn hybrid_fuses_by_rank_and_returns_one_hit_per_file() {
        let (_d, s) = brain(&[
            (
                "long.md",
                "# One\n\nalpha beta gamma\n\n# Two\n\nalpha beta delta\n",
            ),
            ("other.md", "# Other\n\nzeta eta theta"),
        ]);
        let q = Query {
            terms: Some("alpha"),
            about: Some("alpha beta"),
            k: 5,
        };
        let hits = search(&s, &mut Fake, &q, EXACT_LIMIT).unwrap();
        assert_eq!(paths(&hits).iter().filter(|p| **p == "long.md").count(), 1);
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
        let q = Query {
            terms: Some("banana"),
            about: None,
            k: 3,
        };
        // The long file's 80 chunks all rank above the others', and fill the first pool of 50.
        let first = fts(&s[0].store, &None, "banana", 50).unwrap();
        let in_big = |id: i64| chunk_hit(&s[0], id, 0.0).unwrap().unwrap().path == "big.md";
        assert!(first.iter().all(|(id, _)| in_big(*id)));
        let hits = search(&s, &mut Fake, &q, EXACT_LIMIT).unwrap();
        let mut found = paths(&hits);
        found.sort();
        assert_eq!(found, ["b.md", "big.md", "c.md"]);
        // And the same through the vector half.
        let q = Query {
            terms: None,
            about: Some("banana"),
            k: 3,
        };
        let first = vector(
            &s[0].store,
            &None,
            &Fake.embed_query("banana").unwrap(),
            50,
            EXACT_LIMIT,
        )
        .unwrap();
        assert!(first.iter().all(|(id, _)| in_big(*id)));
        assert_eq!(search(&s, &mut Fake, &q, EXACT_LIMIT).unwrap().len(), 3);
    }

    #[test]
    fn hits_have_the_settled_fields_and_name_their_source() {
        let (_d, s) = brain(DOCS);
        let q = Query {
            terms: Some("holiday"),
            about: None,
            k: 1,
        };
        let hit = &search(&s, &mut Fake, &q, EXACT_LIMIT).unwrap()[0];
        assert_eq!((hit.title.as_str(), hit.kind.as_str()), ("Leave", "raw"));
        assert_eq!(hit.source, "brain");
        assert!(hit.file.ends_with("brain/leave.md") && hit.file.starts_with('/'));
        assert!(
            hit.snippet.contains("holiday")
                && !hit.stale
                && hit.status.is_none()
                && hit.versions.is_empty()
        );
        let json = serde_json::to_value(hit).unwrap();
        for k in [
            "id", "source", "path", "file", "title", "heading", "date", "kind", "hash", "status",
            "stale", "score", "snippet", "versions",
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
        let q = Query {
            terms: Some("tangerine"),
            about: None,
            k: 5,
        };
        let hits = search(&both, &mut Fake, &q, EXACT_LIMIT).unwrap();
        let found: Vec<(&str, &str)> = hits
            .iter()
            .map(|h| (h.source.as_str(), h.path.as_str()))
            .collect();
        assert_eq!(found.len(), 2);
        assert!(found.contains(&("a", "README.md")) && found.contains(&("b", "README.md")));
        assert_ne!(hits[0].id, hits[1].id);
        // Alone, a source returns only its own files.
        let alone = search(&both[1..], &mut Fake, &q, EXACT_LIMIT).unwrap();
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
        let q = Query {
            terms: Some("banana"),
            about: None,
            k: 3,
        };
        let hits = search(&[a, b], &mut Fake, &q, EXACT_LIMIT).unwrap();
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
        let q = Query {
            terms: Some("tangerine"),
            about: None,
            k: 5,
        };
        let hits = search(&both, &mut Fake, &q, EXACT_LIMIT).unwrap();
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
        let q = Query {
            terms: None,
            about: Some("how many holiday days"),
            k: 3,
        };
        let hits = search(&[b, a], &mut Fake, &q, EXACT_LIMIT).unwrap();
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

    #[test]
    fn some_query_is_required() {
        let (_d, s) = brain(DOCS);
        assert!(search(
            &s,
            &mut Fake,
            &Query {
                terms: None,
                about: None,
                k: 3
            },
            EXACT_LIMIT
        )
        .is_err());
    }

    fn decision(title: &str, date: &str, kind: &str, extra: &str, body: &str) -> String {
        format!("---\ntitle: {title}\ndate: {date}\nkind: {kind}\n{extra}---\n{body}\n")
    }

    fn found(sources: &[Opened], terms: &str, k: usize, filter: &Filter) -> Vec<Hit> {
        let q = Query {
            terms: Some(terms),
            about: None,
            k,
        };
        search_filtered(sources, &mut Fake, &q, filter, EXACT_LIMIT).unwrap()
    }

    /// An outside source with `a` replaced by `b`, an unrelated proposal and a raw file.
    fn rules() -> (TempDir, Vec<Opened>) {
        brain(&[
            (
                "decisions/2023-01-01-a.md",
                &decision(
                    "A",
                    "2023-01-01",
                    "product",
                    "status: superseded\nsuperseded-by: 2024-01-01-b\n",
                    "Fax is the contact channel.",
                ),
            ),
            (
                "decisions/2024-01-01-b.md",
                &decision(
                    "B",
                    "2024-01-01",
                    "product",
                    "status: accepted\nsupersedes: [2023-01-01-a]\nproject: shop\n",
                    "Email is the contact channel.",
                ),
            ),
            (
                "decisions/2025-01-01-c.md",
                &decision(
                    "C",
                    "2025-01-01",
                    "process",
                    "status: proposed\n",
                    "Chat could be the contact channel.",
                ),
            ),
            (
                "policies/contact.md",
                "# Contact\n\nThe contact channel is whatever works.",
            ),
        ])
    }

    #[test]
    fn superseded_decisions_are_hidden_unless_asked_for_and_say_what_replaced_them() {
        let (_d, s) = rules();
        let paths_of = |f: &Filter| -> Vec<String> {
            found(&s, "contact", 10, f)
                .into_iter()
                .map(|h| h.path)
                .collect()
        };
        let plain = paths_of(&Filter::default());
        assert!(
            !plain.contains(&"decisions/2023-01-01-a.md".to_string()),
            "{plain:?}"
        );
        let all = found(
            &s,
            "contact",
            10,
            &Filter {
                all: true,
                ..Default::default()
            },
        );
        let old = all
            .iter()
            .find(|h| h.path == "decisions/2023-01-01-a.md")
            .unwrap();
        assert_eq!(old.status.as_deref(), Some("superseded"));
        assert_eq!(
            old.superseded_by.as_deref(),
            Some("decisions/2024-01-01-b.md")
        );
        assert_eq!(all.last().unwrap().path, old.path, "last of all");
        let asked = paths_of(&Filter {
            status: Some("superseded"),
            ..Default::default()
        });
        assert_eq!(asked, ["decisions/2023-01-01-a.md"]);
    }

    #[test]
    fn with_all_the_current_decision_is_still_found_behind_many_superseded_ones() {
        let mut files: Vec<(String, String)> = (0..4)
            .map(|i| {
                (
                    format!("decisions/2023-0{}-01-old{i}.md", i + 1),
                    decision(
                        "Old",
                        &format!("2023-0{}-01", i + 1),
                        "product",
                        "status: superseded\nsuperseded-by: 2025-01-01-new\n",
                        &"gizmo ".repeat(10),
                    ),
                )
            })
            .collect();
        files.push((
            "decisions/2025-01-01-new.md".into(),
            decision(
                "New",
                "2025-01-01",
                "product",
                "status: accepted\n",
                "gizmo",
            ),
        ));
        let refs: Vec<(&str, &str)> = files
            .iter()
            .map(|(p, t)| (p.as_str(), t.as_str()))
            .collect();
        let (_d, s) = brain(&refs);
        let hits = found(
            &s,
            "gizmo",
            1,
            &Filter {
                all: true,
                ..Default::default()
            },
        );
        assert_eq!(hits[0].path, "decisions/2025-01-01-new.md");
    }

    #[test]
    fn asking_for_the_superseded_stops_at_the_window() {
        let files: Vec<(String, String)> = (0..6)
            .map(|i| {
                (
                    format!("decisions/2023-0{}-01-old{i}.md", i + 1),
                    decision(
                        "Old",
                        "2023-01-01",
                        "product",
                        "status: superseded\n",
                        &format!("gizmo {i}"),
                    ),
                )
            })
            .collect();
        let refs: Vec<(&str, &str)> = files
            .iter()
            .map(|(p, t)| (p.as_str(), t.as_str()))
            .collect();
        let (_d, s) = brain(&refs);
        let hits = found(
            &s,
            "gizmo",
            2,
            &Filter {
                status: Some("superseded"),
                ..Default::default()
            },
        );
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn filters_narrow_by_kind_project_since_and_status() {
        let (_d, s) = rules();
        let only = |f: Filter| -> Vec<String> {
            found(&s, "contact", 10, &f)
                .into_iter()
                .map(|h| h.path)
                .collect()
        };
        assert_eq!(
            only(Filter {
                kind: Some("process"),
                ..Default::default()
            }),
            ["decisions/2025-01-01-c.md"]
        );
        assert_eq!(
            only(Filter {
                kind: Some("raw"),
                ..Default::default()
            }),
            ["policies/contact.md"]
        );
        assert_eq!(
            only(Filter {
                project: Some("shop"),
                ..Default::default()
            }),
            ["decisions/2024-01-01-b.md"]
        );
        assert_eq!(
            only(Filter {
                since: Some("2024-06-01"),
                kind: Some("decision"),
                ..Default::default()
            }),
            ["decisions/2025-01-01-c.md"]
        );
        assert_eq!(
            only(Filter {
                status: Some("accepted"),
                ..Default::default()
            }),
            ["decisions/2024-01-01-b.md"]
        );
        let kinds = found(
            &s,
            "contact",
            10,
            &Filter {
                kind: Some("decision"),
                ..Default::default()
            },
        );
        assert_eq!(kinds.len(), 2);
        assert!(only(Filter {
            project: Some("nowhere"),
            ..Default::default()
        })
        .is_empty());
        for bad in [
            Filter {
                kind: Some("memo"),
                ..Default::default()
            },
            Filter {
                status: Some("draft"),
                ..Default::default()
            },
            Filter {
                since: Some("last week"),
                ..Default::default()
            },
            Filter {
                since: Some("2024-01-01T10:00"),
                ..Default::default()
            },
        ] {
            let q = Query {
                terms: Some("contact"),
                about: None,
                k: 3,
            };
            assert!(
                search_filtered(&s, &mut Fake, &q, &bad, EXACT_LIMIT).is_err(),
                "{bad:?}"
            );
        }
    }

    fn entry(path: &str, kind: &str, status: Option<&str>, stale: bool) -> (usize, Hit) {
        (
            0,
            Hit {
                id: path.into(),
                source: "s".into(),
                path: path.into(),
                file: path.into(),
                title: String::new(),
                heading: String::new(),
                date: String::new(),
                kind: kind.into(),
                hash: String::new(),
                status: status.map(String::from),
                project: None,
                superseded_by: None,
                amended_by: vec![],
                stale,
                score: 0.0,
                snippet: String::new(),
                versions: vec![],
            },
        )
    }

    fn ordered(hits: Vec<(usize, Hit)>, k: usize) -> Vec<String> {
        trusted(&[], hits, k, &Filter::default())
            .unwrap()
            .into_iter()
            .map(|h| h.path)
            .collect()
    }

    #[test]
    fn an_accepted_decision_moves_up_one_place_and_wins_the_tie() {
        let raw = |p: &str| entry(p, "raw", None, false);
        let hits = vec![
            raw("r0"),
            raw("r1"),
            raw("r2"),
            entry("dec", "product", Some("accepted"), false),
            raw("r4"),
        ];
        assert_eq!(ordered(hits, 5), ["r0", "r1", "dec", "r2", "r4"]);
        let hits = vec![raw("r0"), entry("adr", "adr", Some("accepted"), false)];
        assert_eq!(ordered(hits, 2), ["adr", "r0"], "an ADR is a decision");
        // A decision just outside the top k can come in, one further out cannot.
        let from = |n: usize| -> Vec<(usize, Hit)> {
            let mut hits: Vec<_> = (0..n).map(|i| raw(&format!("r{i}"))).collect();
            hits.push(entry("dec", "business", Some("accepted"), false));
            hits
        };
        assert_eq!(ordered(from(2), 2), ["r0", "dec"]);
        assert_eq!(ordered(from(3), 2), ["r0", "r1"]);
    }

    #[test]
    fn proposals_and_decisions_without_a_status_are_documents_and_superseded_ones_come_last() {
        let hits = vec![
            entry("sup", "product", Some("superseded"), false),
            entry("prop", "product", Some("proposed"), false),
            entry("raw", "raw", None, false),
            entry("nostatus", "decision", None, false),
        ];
        assert_eq!(ordered(hits, 4), ["prop", "raw", "nostatus", "sup"]);
    }

    #[test]
    fn fresh_notes_join_the_order_below_decisions_and_stale_ones_go_below_the_raw_files() {
        let raw = |p: &str| entry(p, "raw", None, false);
        let note = |p: &str| entry(p, "note", None, false);
        // A fresh note moves up one place, like a decision.
        let hits = vec![raw("r0"), raw("r1"), note("n"), raw("r3")];
        assert_eq!(ordered(hits, 4), ["r0", "n", "r1", "r3"]);
        // On a tie a decision comes first, then the note, then the raw file.
        let hits = vec![
            raw("r0"),
            note("n"),
            entry("dec", "product", Some("accepted"), false),
        ];
        assert_eq!(ordered(hits, 3), ["n", "r0", "dec"]);
        let hits = vec![
            raw("r0"),
            entry("dec", "product", Some("accepted"), false),
            raw("r2"),
            note("n"),
        ];
        assert_eq!(ordered(hits, 4), ["dec", "r0", "n", "r2"]);
        // A stale note goes below the raw files, and above a superseded decision.
        let hits = vec![
            entry("sup", "product", Some("superseded"), false),
            entry("stale", "note", None, true),
            raw("r2"),
            raw("r3"),
        ];
        assert_eq!(ordered(hits, 4), ["r2", "r3", "stale", "sup"]);
    }

    #[test]
    fn a_real_search_reports_each_entrys_kind_and_status() {
        let (_d, s) = brain(&[
            ("policies/raw.md", "# Raw\n\nQuokka."),
            (
                "decisions/2024-01-01-d.md",
                &decision(
                    "D",
                    "2024-01-01",
                    "business",
                    "status: accepted\n",
                    "Quokka.",
                ),
            ),
            (
                "decisions/2024-02-01-p.md",
                &decision(
                    "P",
                    "2024-02-01",
                    "business",
                    "status: proposed\n",
                    "Quokka.",
                ),
            ),
        ]);
        let hits = found(&s, "quokka", 3, &Filter::default());
        assert_eq!(hits.len(), 3);
        let status = |p: &str| {
            let h = hits.iter().find(|h| h.path == p).unwrap();
            (h.kind.clone(), h.status.clone())
        };
        assert_eq!(
            status("decisions/2024-01-01-d.md"),
            ("business".into(), Some("accepted".into()))
        );
        assert_eq!(
            status("decisions/2024-02-01-p.md"),
            ("business".into(), Some("proposed".into()))
        );
        assert_eq!(status("policies/raw.md"), ("raw".into(), None));
    }

    #[test]
    fn an_amended_adr_comes_with_its_amendments_and_a_projects_adrs_count_as_its_own() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("courier-app");
        for (p, t) in [
            ("docs/adr/0002-a.md", "# 0002: Currencies\n\nStatus: accepted; amended by [0004](0004-b.md)\n\nStored in ISK and EUR."),
            ("docs/adr/0004-b.md", "# 0004: Add NOK\n\nStatus: accepted; amended by [0005](0005-c.md)\n\nNOK as well."),
            ("docs/adr/0005-c.md", "# 0005: NOK rounding\n\nStatus: accepted\n\nRounded at the edge."),
            ("docs/adr/0001-o.md", "# 0001: Old\n\nStatus: superseded by 0002\n\nStored in ISK only."),
            ("docs/PRODUCT.md", "# Product\n\nAmounts in ISK."),
        ] {
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
            update: false,
        };
        let mut store = Store::create(&dir.path().join("idx/index.sqlite"), &root).unwrap();
        index::index(&source, &mut store, &mut Fake, false).unwrap();
        let s = vec![Opened::new(&source, store)];

        let hits = found(&s, "ISK", 2, &Filter::default());
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
        assert!(!paths.contains(&"docs/adr/0001-o.md"));

        // No `project:` in an ADR: it is the project's, named after its folder here (no remote).
        let own = found(
            &s,
            "ISK",
            10,
            &Filter {
                project: Some("courier-app"),
                kind: Some("adr"),
                ..Default::default()
            },
        );
        assert_eq!(own.len(), 3);
        assert!(found(
            &s,
            "ISK",
            10,
            &Filter {
                project: Some("other"),
                ..Default::default()
            }
        )
        .is_empty());
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
        for q in [
            Query {
                terms: Some("leave"),
                about: None,
                k: 5,
            },
            Query {
                terms: Some("leave"),
                about: Some("how many days of leave"),
                k: 5,
            },
        ] {
            let hits = search(&s, &mut Fake, &q, EXACT_LIMIT).unwrap();
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
        let hits = search(
            &s,
            &mut Fake,
            &Query {
                terms: Some("leave OR plants"),
                about: None,
                k: 10,
            },
            EXACT_LIMIT,
        )
        .unwrap();
        assert_eq!(hits.len(), 2);
    }
}
