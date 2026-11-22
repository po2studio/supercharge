//! Hybrid search: BM25 over FTS5 and cosine over the vectors, fused by rank (RRF), across the
//! index of each source a project reads.

use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::{collections::HashMap, path::PathBuf};

use crate::{embed::Embedder, sources::Source, store, store::Store, vecs};

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
    pub kind: String,
    pub status: Option<String>,
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

/// A source's index, open for reading.
pub struct Opened {
    pub label: String,
    pub root: PathBuf,
    tag: String,
    pub store: Store,
}

impl Opened {
    pub fn new(source: &Source, store: Store) -> Opened {
        Opened {
            label: source.label.clone(),
            root: source.root.clone(),
            tag: store::key_hash(&source.key())[..8].to_string(),
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
    if q.terms.is_none() && q.about.is_none() {
        bail!("give --terms, --about, or both");
    }
    let query_vector = q.about.map(|t| embedder.embed_query(t)).transpose()?;
    // One hit per file, its best chunk. A file with many matching chunks can fill the candidate
    // pool on its own, so the pool grows until it yields `k` files or holds every match.
    let mut pool = (q.k * 10).max(50);
    loop {
        let mut exhausted = true;
        let by_terms = match q.terms {
            Some(t) => Some(merge(sources, pool, &mut exhausted, |s| {
                fts(&s.store, t, pool)
            })?),
            None => None,
        };
        let by_about = match &query_vector {
            Some(v) => Some(merge(sources, pool, &mut exhausted, |s| {
                vector(&s.store, v, pool, exact_limit)
            })?),
            None => None,
        };
        let ranked: Vec<(Key, f64)> = match (by_terms, by_about) {
            (Some(t), Some(a)) => fuse(&[t, a]),
            (Some(l), None) | (None, Some(l)) => l,
            (None, None) => unreachable!(),
        };
        let mut hits: Vec<Hit> = Vec::new();
        let mut seen: std::collections::HashSet<(usize, String)> = Default::default();
        for ((n, id), score) in ranked {
            let hit = chunk_hit(&sources[n], id, score)?;
            // One hit per file of a source; the label is for people, so two sources are told
            // apart by position, not by name.
            if seen.insert((n, hit.path.clone())) {
                hits.push(hit);
            }
            if hits.len() == q.k {
                break;
            }
        }
        if hits.len() == q.k || exhausted {
            return Ok(hits);
        }
        pool *= 4;
    }
}

/// Each source's best `pool` matches by `one`, as one list by score: cosine is the same measure
/// in every index, and BM25 close enough to it for a source's words to rank beside another's.
/// `exhausted` goes false when some source had more matches than it returned.
fn merge(
    sources: &[Opened],
    pool: usize,
    exhausted: &mut bool,
    one: impl Fn(&Opened) -> Result<Vec<(i64, f64)>>,
) -> Result<Vec<(Key, f64)>> {
    let mut all: Vec<(Key, f64)> = Vec::new();
    for (n, source) in sources.iter().enumerate() {
        let list = one(source)?;
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
fn fts(store: &Store, terms: &str, pool: usize) -> Result<Vec<(i64, f64)>> {
    if terms.trim().is_empty() {
        bail!("--terms is empty");
    }
    match fts_query(store, terms, pool) {
        Ok(rows) => Ok(rows),
        Err(_) => fts_query(store, &quote_words(terms), pool)
            .with_context(|| format!("cannot search for {terms:?}")),
    }
}

fn fts_query(store: &Store, query: &str, pool: usize) -> rusqlite::Result<Vec<(i64, f64)>> {
    let mut stmt = store.conn.prepare(
        "SELECT rowid, -bm25(fts) FROM fts WHERE fts MATCH ?1 ORDER BY bm25(fts) LIMIT ?2",
    )?;
    let rows = stmt.query_map(rusqlite::params![query, pool as i64], |r| {
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
        let mut near: Vec<(u32, i64)> = Vec::new();
        let mut stmt = store.conn.prepare("SELECT id, code FROM chunks")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))?;
        for row in rows {
            let (id, code) = row?;
            near.push((vecs::hamming(&qcode, &code), id));
            if near.len() >= keep * 4 {
                near.select_nth_unstable(keep);
                near.truncate(keep);
            }
        }
        if near.len() > keep {
            near.select_nth_unstable(keep);
            near.truncate(keep);
        }
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

fn chunk_hit(source: &Opened, id: i64, score: f64) -> Result<Hit> {
    let hit = source.store.conn.query_row(
        "SELECT f.path, f.title, c.heading, f.date, f.kind, c.text FROM chunks c JOIN files f ON f.id = c.file_id WHERE c.id = ?1",
        [id],
        |r| {
            let path: String = r.get(0)?;
            Ok(Hit {
                id: format!("{}-{id}", source.tag),
                source: source.label.clone(),
                file: source.root.join(&path).to_string_lossy().into_owned(),
                path,
                title: r.get(1)?,
                heading: r.get(2)?,
                date: r.get(3)?,
                kind: r.get(4)?,
                status: None,
                stale: false,
                score,
                snippet: snippet(&r.get::<_, String>(5)?),
                versions: Vec::new(),
            })
        },
    )?;
    Ok(hit)
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
        let first = fts(&s[0].store, "banana", 50).unwrap();
        let in_big = |id: i64| chunk_hit(&s[0], id, 0.0).unwrap().path == "big.md";
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
            "id", "source", "path", "file", "title", "heading", "date", "kind", "status", "stale",
            "score", "snippet", "versions",
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
}
