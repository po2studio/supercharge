//! ADRs: what a project's ADR says about itself in its status line (`Status: accepted`,
//! `superseded by 0005`, `amended by 0004`), and, after every index run, which file each name
//! points to, so a search only reads columns.

use anyhow::Result;
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

use crate::store::Store;

const EXTENSIONS: [&str; 4] = ["md", "markdown", "txt", "text"];

/// What an ADR's status line declares about it. Empty for most files.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Status {
    /// `proposed`, `accepted` or `superseded`.
    pub status: Option<String>,
    /// The ADR that replaced this one, as written.
    pub superseded_by: Option<String>,
    /// The ADRs that amend this one, as written.
    pub amended_by: Vec<String>,
}

impl Status {
    pub fn is_empty(&self) -> bool {
        *self == Status::default()
    }
}

/// What an ADR's status line says, in the first lines of its body: `Status: accepted`,
/// `Status: superseded by 0005`, `Status: accepted; amended by [0004](0004-x.md), 0007`.
/// `amended by` alone means accepted.
pub fn status_of(body: &str) -> Status {
    let mut meta = Status::default();
    let Some(line) = body.lines().take(30).find_map(|l| {
        let l = l.trim().trim_start_matches(['*', '_']);
        let (key, rest) = l.split_at_checked(6)?;
        let rest = rest.trim_start_matches(['*', '_']);
        (key.eq_ignore_ascii_case("status") && rest.starts_with(':'))
            .then(|| rest.trim_start_matches(['*', '_', ':', ' ']).trim())
    }) else {
        return meta;
    };
    for part in line.split(';') {
        let part = part.trim();
        let lower = part.to_lowercase();
        if lower.starts_with("superseded by") {
            meta.status = Some("superseded".into());
            meta.superseded_by = refs(part.get("superseded by".len()..).unwrap_or(""))
                .into_iter()
                .next();
        } else if lower.starts_with("amended by") {
            meta.status.get_or_insert_with(|| "accepted".into());
            meta.amended_by = refs(part.get("amended by".len()..).unwrap_or(""));
        } else {
            let word = lower
                .split(|c: char| !c.is_alphabetic())
                .next()
                .unwrap_or("");
            if matches!(word, "proposed" | "accepted" | "superseded") {
                meta.status = Some(word.to_string());
            }
        }
    }
    meta
}

/// What an `amended by` or `superseded by` names: a comma- or `and`-separated list whose items
/// are Markdown links (their targets) or plain names.
fn refs(text: &str) -> Vec<String> {
    text.replace(" and ", ",")
        .replace('&', ",")
        .split(',')
        .filter_map(|item| {
            let name = match item.find("](") {
                Some(i) => item[i + 2..].split(')').next().unwrap_or("").trim(),
                None => item
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_matches(|c: char| !c.is_alphanumeric()),
            };
            (!name.is_empty()).then(|| name.to_string())
        })
        .collect()
}

/// Whether `path` (relative to the project) is an ADR: a file in `adr/` under the docs folder.
pub fn is_adr(path: &str, docs: &str) -> bool {
    let adr = if docs.is_empty() {
        "adr/".to_string()
    } else {
        format!("{docs}/adr/")
    };
    path.starts_with(&adr)
}

/// Recomputes `status`, `superseded_by` and `amended_by` of every file from what the files
/// declare, naming each replacement and amendment by the path of the file it is.
pub fn resolve(store: &mut Store) -> Result<()> {
    let tx = store.conn.transaction()?;
    tx.execute(
        "UPDATE files SET status = '', superseded_by = '', amended_by = '' WHERE status != '' OR superseded_by != '' OR amended_by != ''",
        [],
    )?;
    let declared: Vec<(String, Status)> = {
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
    tx.commit()?;
    Ok(())
}

/// The path of the file a reference names, next to the file that holds it: a link target
/// (`0004-x.md`, `../adr/0004-x.md`), a file name without its extension, or an ADR's number
/// (`0004`). `None` when nothing in the index is it.
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

    fn file(path: &str, meta: Status) -> NewFile {
        NewFile {
            path: path.into(),
            hash: path.into(),
            title: "T".into(),
            date: "2024-01-01".into(),
            kind: "adr".into(),
            own_date: None,
            simhash: None,
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

    fn meta(status: &str) -> Status {
        Status {
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
    fn adrs_are_named_by_link_or_number_and_resolving_again_gives_the_same() {
        let amended = Status {
            amended_by: vec!["0004-b.md".into(), "0005".into(), "9999".into()],
            ..meta("accepted")
        };
        let (_d, mut s) = store_of(vec![
            file("docs/adr/0002-a.md", amended),
            file("docs/adr/0004-b.md", meta("accepted")),
            file("docs/adr/0005-c.md", meta("accepted")),
            file(
                "docs/adr/0001-old.md",
                Status {
                    superseded_by: Some("../adr/0002-a.md".into()),
                    ..meta("superseded")
                },
            ),
        ]);
        let expected = (
            "accepted".to_string(),
            "".to_string(),
            "docs/adr/0004-b.md\ndocs/adr/0005-c.md\n9999".to_string(),
        );
        assert_eq!(
            state(&s, "docs/adr/0002-a.md"),
            expected,
            "an unresolved name stays as written"
        );
        assert_eq!(state(&s, "docs/adr/0001-old.md").1, "docs/adr/0002-a.md");
        resolve(&mut s).unwrap();
        assert_eq!(state(&s, "docs/adr/0002-a.md"), expected);
        assert_eq!(state(&s, "docs/adr/0001-old.md").1, "docs/adr/0002-a.md");

        // The amendment goes away: its name stays as written, nothing resolves to it.
        s.remove_file("docs/adr/0004-b.md").unwrap();
        resolve(&mut s).unwrap();
        assert_eq!(
            state(&s, "docs/adr/0002-a.md").2,
            "0004-b.md\ndocs/adr/0005-c.md\n9999"
        );
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

    #[test]
    fn adrs_are_the_files_in_adr_under_the_docs_folder() {
        assert!(is_adr("docs/adr/0001-x.md", "docs"));
        assert!(is_adr("adr/0001-x.md", ""));
        assert!(is_adr("handbook/adr/0001-x.md", "handbook"));
        assert!(!is_adr("docs/PRODUCT.md", "docs"));
        assert!(!is_adr("src/adr/0001-x.md", "docs"));
    }

    #[test]
    fn an_adrs_status_line_says_accepted_superseded_or_amended() {
        let s = |t: &str| status_of(&format!("# 0001: T\n\n{t}\n\n## Context\n"));
        assert_eq!(s("Status: accepted").status.as_deref(), Some("accepted"));
        assert_eq!(
            s("**Status:** Proposed").status.as_deref(),
            Some("proposed")
        );
        let sup = s("Status: superseded by [0005](0005-x.md)");
        assert_eq!(sup.status.as_deref(), Some("superseded"));
        assert_eq!(sup.superseded_by.as_deref(), Some("0005-x.md"));
        let plain = s("Status: superseded by 0005");
        assert_eq!(plain.superseded_by.as_deref(), Some("0005"));
        let am = s("Status: accepted; amended by [0004](0004-a.md), [0007](0007-b.md)");
        assert_eq!(am.status.as_deref(), Some("accepted"));
        assert_eq!(am.amended_by, ["0004-a.md", "0007-b.md"]);
        let alone = s("Status: amended by 0004 and 0007");
        assert_eq!(alone.status.as_deref(), Some("accepted"));
        assert_eq!(alone.amended_by, ["0004", "0007"]);
        let mixed = s("Status: accepted; amended by [0004](0004-a.md), 0007 and [0009](0009-c.md)");
        assert_eq!(mixed.amended_by, ["0004-a.md", "0007", "0009-c.md"]);
        assert!(status_of("# No status here\n\nStatuses vary.\n").is_empty());
        assert!(status_of("Status: wat").is_empty());
        // Non-ASCII text must not trip the slicing.
        assert!(status_of("Status: İİİİİİ superseded by x").status.is_none());
    }
}
