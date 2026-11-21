//! Hybrid search: BM25 over FTS5 and cosine over the vectors, fused by rank (RRF).

use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::collections::HashMap;

use crate::{embed::Embedder, store::Store, vecs};

/// The reciprocal-rank-fusion constant.
const RRF_K: f64 = 60.0;
/// Up to this many chunks the vector search scores every vector exactly; beyond it the 1-bit
/// codes pick the candidates to rescore.
pub const EXACT_LIMIT: usize = 50_000;
const SNIPPET_CHARS: usize = 600;

#[derive(Debug, Serialize)]
pub struct Hit {
    pub id: i64,
    pub path: String,
    pub title: String,
    pub heading: String,
    pub date: String,
    pub kind: String,
    pub status: Option<String>,
    pub stale: bool,
    pub score: f64,
    pub snippet: String,
    pub versions: Vec<String>,
}

pub struct Query<'a> {
    pub terms: Option<&'a str>,
    pub about: Option<&'a str>,
    pub k: usize,
}

pub fn search(
    store: &Store,
    embedder: &mut dyn Embedder,
    q: &Query,
    exact_limit: usize,
) -> Result<Vec<Hit>> {
    if q.terms.is_none() && q.about.is_none() {
        bail!("give --terms, --about, or both");
    }
    let pool = (q.k * 10).max(50);
    let by_terms = q.terms.map(|t| fts(store, t, pool)).transpose()?;
    let by_about = match q.about {
        Some(text) => Some(vector(
            store,
            &embedder.embed_query(text)?,
            pool,
            exact_limit,
        )?),
        None => None,
    };
    let ranked: Vec<(i64, f64)> = match (by_terms, by_about) {
        (Some(t), Some(a)) => fuse(&[t, a]),
        (Some(l), None) | (None, Some(l)) => l,
        (None, None) => unreachable!(),
    };
    let mut hits: Vec<Hit> = Vec::new();
    for (id, score) in ranked {
        let hit = chunk_hit(store, id, score)?;
        // One hit per file: its best chunk.
        if hits.iter().all(|h| h.path != hit.path) {
            hits.push(hit);
        }
        if hits.len() == q.k {
            break;
        }
    }
    Ok(hits)
}

fn fuse(lists: &[Vec<(i64, f64)>]) -> Vec<(i64, f64)> {
    let mut score: HashMap<i64, f64> = HashMap::new();
    for list in lists {
        for (rank, (id, _)) in list.iter().enumerate() {
            *score.entry(*id).or_insert(0.0) += 1.0 / (RRF_K + rank as f64 + 1.0);
        }
    }
    let mut out: Vec<(i64, f64)> = score.into_iter().collect();
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

fn chunk_hit(store: &Store, id: i64, score: f64) -> Result<Hit> {
    let hit = store.conn.query_row(
        "SELECT f.path, f.title, c.heading, f.date, f.kind, c.text FROM chunks c JOIN files f ON f.id = c.file_id WHERE c.id = ?1",
        [id],
        |r| {
            Ok(Hit {
                id,
                path: r.get(0)?,
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

    fn brain(files: &[(&str, &str)]) -> (TempDir, Store) {
        let dir = TempDir::new().unwrap();
        for (p, t) in files {
            let full = dir.path().join("brain").join(p);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, t).unwrap();
        }
        fs::create_dir_all(dir.path().join("brain")).unwrap();
        let mut store = Store::create(
            &dir.path().join("idx/index.sqlite"),
            &dir.path().join("brain"),
        )
        .unwrap();
        index::index(&dir.path().join("brain"), &mut store, &mut Fake, false).unwrap();
        (dir, store)
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
        let (_d, store) = brain(DOCS);
        let q = |t| Query {
            terms: Some(t),
            about: None,
            k: 5,
        };
        assert_eq!(
            paths(&search(&store, &mut Fake, &q("holiday"), EXACT_LIMIT).unwrap()),
            ["leave.md"]
        );
        assert_eq!(
            paths(&search(&store, &mut Fake, &q("INV-2024-0042"), EXACT_LIMIT).unwrap()),
            ["ids.md"]
        );
        assert_eq!(
            paths(&search(&store, &mut Fake, &q("dogs OR holiday"), EXACT_LIMIT).unwrap()).len(),
            2
        );
        assert!(
            search(&store, &mut Fake, &q("nonexistentword"), EXACT_LIMIT)
                .unwrap()
                .is_empty()
        );
        assert!(search(&store, &mut Fake, &q("  "), EXACT_LIMIT).is_err());
    }

    #[test]
    fn about_ranks_by_vector_similarity() {
        let (_d, store) = brain(DOCS);
        let q = Query {
            terms: None,
            about: Some("paid holiday days"),
            k: 2,
        };
        let hits = search(&store, &mut Fake, &q, EXACT_LIMIT).unwrap();
        assert_eq!(hits[0].path, "leave.md");
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn the_one_bit_first_pass_agrees_with_exact_scoring_on_the_top_hit() {
        let (_d, store) = brain(DOCS);
        let q = Query {
            terms: None,
            about: Some("paid holiday days"),
            k: 1,
        };
        let exact = search(&store, &mut Fake, &q, EXACT_LIMIT).unwrap();
        let coded = search(&store, &mut Fake, &q, 0).unwrap();
        assert_eq!(paths(&exact), paths(&coded));
    }

    #[test]
    fn hybrid_fuses_by_rank_and_returns_one_hit_per_file() {
        let (_d, store) = brain(&[
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
        let hits = search(&store, &mut Fake, &q, EXACT_LIMIT).unwrap();
        assert_eq!(paths(&hits).iter().filter(|p| **p == "long.md").count(), 1);
        assert!(hits[0].score > 0.0);
    }

    #[test]
    fn hits_have_the_settled_fields() {
        let (_d, store) = brain(DOCS);
        let q = Query {
            terms: Some("holiday"),
            about: None,
            k: 1,
        };
        let hit = &search(&store, &mut Fake, &q, EXACT_LIMIT).unwrap()[0];
        assert_eq!((hit.title.as_str(), hit.kind.as_str()), ("Leave", "raw"));
        assert!(
            hit.snippet.contains("holiday")
                && !hit.stale
                && hit.status.is_none()
                && hit.versions.is_empty()
        );
        let json = serde_json::to_value(hit).unwrap();
        for k in [
            "id", "path", "title", "heading", "date", "kind", "status", "stale", "score",
            "snippet", "versions",
        ] {
            assert!(json.get(k).is_some(), "{k}");
        }
    }

    #[test]
    fn long_snippets_are_cut_at_a_word() {
        let s = snippet(&"word ".repeat(400));
        assert!(s.ends_with("word…") && s.chars().count() <= SNIPPET_CHARS + 1);
    }

    #[test]
    fn some_query_is_required() {
        let (_d, store) = brain(DOCS);
        assert!(search(
            &store,
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
