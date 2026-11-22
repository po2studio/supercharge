//! What decisions and ADRs say about each other. A file declares only its own side (`superseded-by`,
//! `supersedes`, an ADR's `amended by`); after every index run this settles each file's status and
//! links across the source, so a search only reads columns.

use anyhow::Result;
use rusqlite::{params, OptionalExtension, Transaction};

use crate::{doc::Meta, store::Store};

const EXTENSIONS: [&str; 4] = ["md", "markdown", "txt", "text"];

/// Recomputes `status`, `superseded_by` and `amended_by` of every file from what the files
/// declare. A decision is superseded when it says so, or when a decision that was accepted lists it in
/// `supersedes`, so a replacement written without updating the old one still hides it.
pub fn resolve(store: &mut Store) -> Result<()> {
    let tx = store.conn.transaction()?;
    tx.execute(
        "UPDATE files SET status = '', superseded_by = '', amended_by = '' WHERE status != '' OR superseded_by != '' OR amended_by != ''",
        [],
    )?;
    let declared: Vec<(String, Meta)> = {
        let mut stmt = tx.prepare("SELECT path, meta FROM files WHERE meta != ''")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (path, meta) = row?;
            out.push((path, serde_json::from_str(&meta)?));
        }
        out
    };
    for (path, meta) in &declared {
        let name = |r: &String| lookup(&tx, path, r).unwrap_or_else(|| r.clone());
        tx.execute(
            "UPDATE files SET status = ?1, superseded_by = ?2, amended_by = ?3 WHERE path = ?4",
            params![
                meta.status.as_deref().unwrap_or_default(),
                meta.superseded_by.as_ref().map(name).unwrap_or_default(),
                meta.amended_by
                    .iter()
                    .map(name)
                    .collect::<Vec<_>>()
                    .join("\n"),
                path
            ],
        )?;
    }
    for (path, meta) in &declared {
        // A decision that was accepted still counts for what it replaced, even once it is
        // superseded itself: replacing A with B and B with C leaves A replaced.
        if !matches!(meta.status.as_deref(), Some("accepted" | "superseded")) {
            continue;
        }
        for old in meta.supersedes.iter().filter_map(|r| lookup(&tx, path, r)) {
            if old != *path {
                tx.execute(
                    "UPDATE files SET status = 'superseded', superseded_by = CASE WHEN superseded_by = '' THEN ?1 ELSE superseded_by END WHERE path = ?2",
                    params![path, old],
                )?;
            }
        }
    }
    tx.commit()?;
    Ok(())
}

/// The path of the file a reference names, next to the file that holds it: a link target
/// (`0004-x.md`, `../adr/0004-x.md`), a decision's name (`2023-11-02-single-phone`), or an ADR's
/// number (`0004`). `None` when nothing in the index is it.
fn lookup(tx: &Transaction, from: &str, reference: &str) -> Option<String> {
    let dir = from.rsplit_once('/').map_or("", |(d, _)| d);
    let reference = reference.split('#').next()?.trim();
    if reference.is_empty() {
        return None;
    }
    let exists = |path: &str| -> bool {
        tx.query_row("SELECT 1 FROM files WHERE path = ?1", [path], |_| Ok(()))
            .optional()
            .ok()
            .flatten()
            .is_some()
    };
    let joined = normalize(dir, reference)?;
    if exists(&joined) {
        return Some(joined);
    }
    for ext in EXTENSIONS {
        let path = format!("{joined}.{ext}");
        if exists(&path) {
            return Some(path);
        }
    }
    // An ADR is named by its number alone.
    if reference.bytes().all(|b| b.is_ascii_digit()) {
        let like = format!("{joined}-%");
        return tx
            .query_row(
                "SELECT path FROM files WHERE path LIKE ?1 ORDER BY path LIMIT 1",
                [like],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten();
    }
    None
}

/// `dir` joined with a relative path, with `.` and `..` applied; `None` if it climbs out of the source.
fn normalize(dir: &str, relative: &str) -> Option<String> {
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for part in relative.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            p => parts.push(p),
        }
    }
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{embed, store::NewFile};
    use tempfile::TempDir;

    fn file(path: &str, meta: Meta) -> NewFile {
        NewFile {
            path: path.into(),
            hash: path.into(),
            title: "T".into(),
            date: "2024-01-01".into(),
            kind: "decision".into(),
            meta,
            chunks: vec![crate::store::NewChunk {
                heading: String::new(),
                text: "text".into(),
                header: "T".into(),
                vector: {
                    let mut v = vec![0f32; embed::DIM];
                    v[0] = 1.0;
                    v
                },
            }],
        }
    }

    fn state(store: &Store, path: &str) -> (String, String, String) {
        store
            .conn
            .query_row(
                "SELECT status, superseded_by, amended_by FROM files WHERE path = ?1",
                [path],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap()
    }

    fn meta(status: &str) -> Meta {
        Meta {
            status: Some(status.into()),
            ..Default::default()
        }
    }

    fn store_of(files: Vec<NewFile>) -> (TempDir, Store) {
        let dir = TempDir::new().unwrap();
        let mut s = Store::create(&dir.path().join("i/index.sqlite"), dir.path()).unwrap();
        s.put_files(&files).unwrap();
        resolve(&mut s).unwrap();
        (dir, s)
    }

    #[test]
    fn a_decision_names_what_replaced_it_by_its_file_name() {
        let (_d, s) = store_of(vec![
            file(
                "decisions/a.md",
                Meta {
                    superseded_by: Some("b".into()),
                    ..meta("superseded")
                },
            ),
            file("decisions/b.md", meta("accepted")),
        ]);
        assert_eq!(
            state(&s, "decisions/a.md"),
            ("superseded".into(), "decisions/b.md".into(), "".into())
        );
        assert_eq!(state(&s, "decisions/b.md").0, "accepted");
    }

    #[test]
    fn an_accepted_replacement_hides_what_it_supersedes_but_a_proposed_one_does_not() {
        let supersedes = |status: &str| Meta {
            supersedes: vec!["a".into()],
            ..meta(status)
        };
        let (_d, s) = store_of(vec![
            file("decisions/a.md", meta("accepted")),
            file("decisions/b.md", supersedes("accepted")),
        ]);
        assert_eq!(
            state(&s, "decisions/a.md"),
            ("superseded".into(), "decisions/b.md".into(), "".into())
        );
        let (_d, s) = store_of(vec![
            file("decisions/a.md", meta("accepted")),
            file("decisions/b.md", supersedes("proposed")),
        ]);
        assert_eq!(state(&s, "decisions/a.md").0, "accepted");
    }

    #[test]
    fn replacing_a_replacement_leaves_the_first_one_replaced() {
        let supersedes = |status: &str, old: &str| Meta {
            supersedes: vec![old.into()],
            ..meta(status)
        };
        // A is accepted; B replaced A and was replaced by C, neither of them marking A.
        let (_d, s) = store_of(vec![
            file("decisions/a.md", meta("accepted")),
            file("decisions/b.md", supersedes("accepted", "a")),
            file("decisions/c.md", supersedes("accepted", "b")),
        ]);
        assert_eq!(state(&s, "decisions/a.md").0, "superseded");
        assert_eq!(state(&s, "decisions/b.md").0, "superseded");
        assert_eq!(state(&s, "decisions/c.md").0, "accepted");
    }

    #[test]
    fn adrs_are_named_by_link_or_number_and_a_removed_replacement_restores_the_old_one() {
        let adr = |path: &str, m: Meta| NewFile {
            kind: "adr".into(),
            ..file(path, m)
        };
        let amended = Meta {
            amended_by: vec!["0004-b.md".into(), "0005".into(), "9999".into()],
            ..meta("accepted")
        };
        let (_d, mut s) = store_of(vec![
            adr("docs/adr/0002-a.md", amended),
            adr("docs/adr/0004-b.md", meta("accepted")),
            adr("docs/adr/0005-c.md", meta("accepted")),
            adr(
                "docs/adr/0001-old.md",
                Meta {
                    superseded_by: Some("../adr/0002-a.md".into()),
                    ..meta("superseded")
                },
            ),
        ]);
        assert_eq!(
            state(&s, "docs/adr/0002-a.md"),
            (
                "accepted".into(),
                "".into(),
                "docs/adr/0004-b.md\ndocs/adr/0005-c.md\n9999".into()
            ),
            "an unresolved name stays as written"
        );
        assert_eq!(state(&s, "docs/adr/0001-old.md").1, "docs/adr/0002-a.md");

        // The replacement goes away: running again leaves nothing marked by it.
        let b = Meta {
            supersedes: vec!["0002".into()],
            ..meta("accepted")
        };
        s.put_files(&[adr("docs/adr/0004-b.md", b)]).unwrap();
        resolve(&mut s).unwrap();
        assert_eq!(state(&s, "docs/adr/0002-a.md").0, "superseded");
        s.remove_file("docs/adr/0004-b.md").unwrap();
        resolve(&mut s).unwrap();
        assert_eq!(state(&s, "docs/adr/0002-a.md").0, "accepted");
    }

    #[test]
    fn names_cannot_climb_out_of_the_source() {
        assert_eq!(
            normalize("docs/adr", "../x.md").as_deref(),
            Some("docs/x.md")
        );
        assert_eq!(
            normalize("docs", "./a/../b.md").as_deref(),
            Some("docs/b.md")
        );
        assert_eq!(normalize("", "../x.md"), None);
    }
}
