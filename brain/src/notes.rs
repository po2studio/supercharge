//! Notes: what an agent wrote from several raw files, and whether it is still true. A note names
//! its sources with the hash each had when it was written; it is stale when one differs or is
//! gone. Staleness is read, never stored: it depends on the other sources a project lists, and a
//! source's index is shared between projects.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    cell::OnceCell,
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path},
};

use crate::{dedup, search::Opened, sources};

/// A file a note was made from: where it is, and what it said when the note was written.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cited {
    /// `<source>:<path>` with the source as a hit shows it (`project`, or the path as listed in
    /// `brain:`); a bare path is a file of the note's own source.
    pub path: String,
    /// The file's sha256, as a hit gives it.
    pub hash: String,
}

/// What a note's frontmatter says, besides `title` and `project`, which every file has.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub summary: String,
    pub aliases: Vec<String>,
    /// The questions the note answers.
    pub questions: Vec<String>,
    pub sources: Vec<Cited>,
    /// When the note was last checked against its sources, `YYYY-MM-DD`.
    pub verified: Option<String>,
}

const KEYS: [&str; 5] = [
    "sources:",
    "summary:",
    "questions:",
    "aliases:",
    "verified:",
];

/// A note's fields from its frontmatter, or `None` when it has none of them. Frontmatter that is
/// not valid YAML gives a note with no sources, so it is flagged, never taken as fresh.
pub fn from_front(front: &str) -> Option<Note> {
    if !front.lines().any(|l| KEYS.iter().any(|k| l.starts_with(k))) {
        return None;
    }
    let Some(serde_yaml_ng::Value::Mapping(map)) = yaml(front).or_else(|| yaml(&requoted(front)))
    else {
        return Some(Note::default());
    };
    let get = |key: &str| map.get(key);
    let scalar = |v: &serde_yaml_ng::Value| match v {
        serde_yaml_ng::Value::String(s) => Some(s.trim().to_string()),
        serde_yaml_ng::Value::Number(n) => Some(n.to_string()),
        serde_yaml_ng::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    };
    let list = |key: &str| -> Vec<String> {
        match get(key) {
            Some(serde_yaml_ng::Value::Sequence(items)) => {
                items.iter().filter_map(scalar).collect()
            }
            Some(v) => scalar(v).into_iter().collect(),
            None => Vec::new(),
        }
    };
    let sources = match get("sources") {
        Some(serde_yaml_ng::Value::Sequence(items)) => items
            .iter()
            .filter_map(|item| {
                Some(Cited {
                    path: scalar(item.get("path")?)?,
                    hash: scalar(item.get("hash")?)?,
                })
            })
            .collect(),
        _ => Vec::new(),
    };
    Some(Note {
        summary: get("summary").and_then(scalar).unwrap_or_default(),
        aliases: list("aliases"),
        questions: list("questions"),
        sources,
        verified: get("verified")
            .and_then(scalar)
            .and_then(|d| crate::doc::valid_date(&d)),
    })
}

fn yaml(text: &str) -> Option<serde_yaml_ng::Value> {
    serde_yaml_ng::from_str(text).ok()
}

/// The frontmatter with every plain value that holds `: ` quoted, which agents write often
/// (`title: Business trips: approval and meals`) and YAML reads as a mistake.
pub(crate) fn requoted(front: &str) -> String {
    let quote = |v: &str| -> String {
        let v = v.trim();
        if v.contains(": ") && !v.starts_with(['"', '\'', '[', '{', '|', '>']) {
            format!("'{}'", v.replace('\'', "''"))
        } else {
            v.to_string()
        }
    };
    let mut key = "";
    front
        .lines()
        .map(|line| {
            if !line.starts_with([' ', '\t', '-']) {
                let Some((k, v)) = line.split_once(':') else {
                    return line.to_string();
                };
                key = k.trim();
                return format!("{k}: {}", quote(v));
            }
            // An item of `questions` or `aliases`; a source's `path` and `hash` are not plain text.
            let item = line.trim_start().strip_prefix("- ");
            match item {
                Some(text) if matches!(key, "questions" | "aliases") => {
                    format!(
                        "{}- {}",
                        &line[..line.len() - line.trim_start().len()],
                        quote(text)
                    )
                }
                _ => line.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// What a note is indexed as: its summary, the questions it answers and its aliases (so that
/// asking one of them finds it), then its body.
pub fn indexed_text(note: &Note, body: &str) -> String {
    let mut out = String::new();
    if !note.summary.is_empty() {
        out += &note.summary;
        out += "\n\n";
    }
    if !note.questions.is_empty() {
        out += "Answers:\n";
        for q in &note.questions {
            out += &format!("- {q}\n");
        }
        out += "\n";
    }
    if !note.aliases.is_empty() {
        out += &format!("Also called: {}\n\n", note.aliases.join(", "));
    }
    out += body.trim_start();
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Fresh,
    /// The file is there, with other content.
    Changed,
    Gone,
}

/// One source of a note, as it is now.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Checked {
    pub path: String,
    /// What the note recorded.
    pub hash: String,
    pub state: State,
    /// The file's hash now; none when it is gone.
    pub current: Option<String>,
}

/// Reads notes against the sources a search or a stats run has open.
pub struct Ctx<'a> {
    sources: &'a [Opened],
    project: OnceCell<String>,
}

/// The sha256 of a regular file, read in blocks; a file that isn't one (or can't be read) has none.
pub(crate) fn sha_of_file(path: &Path) -> Option<String> {
    // Looked at before it is opened: opening a pipe would wait for a writer.
    if !fs::metadata(path).ok()?.is_file() {
        return None;
    }
    let file = fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut std::io::BufReader::new(file), &mut hasher).ok()?;
    Some(format!("{:x}", hasher.finalize()))
}

impl std::fmt::Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(match self {
            State::Fresh => "fresh",
            State::Changed => "changed",
            State::Gone => "gone",
        })
    }
}

impl<'a> Ctx<'a> {
    pub fn new(sources: &'a [Opened]) -> Ctx<'a> {
        Ctx {
            sources,
            project: OnceCell::new(),
        }
    }

    fn project_name(&self) -> &str {
        self.project.get_or_init(|| {
            self.sources
                .iter()
                .find(|s| s.is_project())
                .map(|s| sources::project_name(&s.root))
                .unwrap_or_default()
        })
    }

    /// The current hash of `rel` in source `n`: from its index, else read from the folder.
    fn hash_now(&self, n: usize, rel: &str) -> Result<Option<String>> {
        let source = &self.sources[n];
        if let Some(hash) = source.store.hash_of(rel)? {
            return Ok(Some(hash));
        }
        let mut path = source.root.clone();
        for part in Path::new(rel).components() {
            match part {
                Component::Normal(p) => path.push(p),
                // A path that leaves the source, or isn't relative, names nothing in it.
                _ => return Ok(None),
            }
        }
        Ok(sha_of_file(&path))
    }

    /// Each source of the note stored in source `own`, as it is now. A file of a project's docs
    /// cited by a note of another project can't be told from here, and is left out.
    pub fn check(&self, own: usize, note: &Note, project: Option<&str>) -> Result<Vec<Checked>> {
        let mut out = Vec::new();
        for cited in &note.sources {
            // The longest source label that prefixes it wins; else it is a file of the note's own source.
            let labelled = self
                .sources
                .iter()
                .enumerate()
                .filter_map(|(i, s)| {
                    cited
                        .path
                        .strip_prefix(s.label.as_str())
                        .and_then(|rest| rest.strip_prefix(':'))
                        .map(|rest| (i, rest))
                })
                .max_by_key(|(i, _)| self.sources[*i].label.len());
            let (n, rel) = labelled.unwrap_or((own, cited.path.as_str()));
            if self.sources[n].is_project() && project.is_some_and(|p| p != self.project_name()) {
                continue;
            }
            let current = self.hash_now(n, rel)?;
            let recorded = cited.hash.trim().to_lowercase();
            let recorded = recorded.strip_prefix("sha256:").unwrap_or(&recorded);
            let state = match &current {
                None => State::Gone,
                Some(now) if now == recorded => State::Fresh,
                Some(_) => State::Changed,
            };
            out.push(Checked {
                path: cited.path.clone(),
                hash: cited.hash.clone(),
                state,
                current,
            });
        }
        Ok(out)
    }

    /// Whether the note in source `own` at `path` is stale: a source changed or is gone, or it
    /// names none to be true to.
    pub fn is_stale(&self, own: usize, path: &str) -> Result<bool> {
        let Some(meta) = self.sources[own].store.note(path)? else {
            return Ok(false);
        };
        let note = meta.note.clone().unwrap_or_default();
        Ok(note.sources.is_empty()
            || self
                .check(own, &note, meta.project.as_deref())?
                .iter()
                .any(|c| c.state != State::Fresh))
    }
}

/// A stale note and what became of each of its sources.
#[derive(Debug, Serialize)]
pub struct StaleNote {
    pub path: String,
    pub sources: Vec<Checked>,
}

/// A note that covers what a newer one does.
#[derive(Debug, Serialize)]
pub struct Duplicate {
    pub path: String,
    pub of: String,
}

/// The notes of one source.
#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub total: usize,
    pub stale: Vec<StaleNote>,
    /// Notes none of whose sources exist any more, or that name none.
    pub orphans: Vec<String>,
    /// Notes made from the same files, or saying nearly the same, as a newer one.
    pub duplicates: Vec<Duplicate>,
}

/// The notes of every source, in numbers.
#[derive(Debug, Default, Serialize)]
pub struct Summary {
    pub total: usize,
    pub stale: usize,
    pub orphans: usize,
    pub duplicates: usize,
}

impl Summary {
    pub fn add(&mut self, r: &Report) {
        self.total += r.total;
        self.stale += r.stale.len();
        self.orphans += r.orphans.len();
        self.duplicates += r.duplicates.len();
    }
}

/// What is wrong with the notes of source `n`.
pub fn report(sources: &[Opened], n: usize) -> Result<Report> {
    let ctx = Ctx::new(sources);
    let mut report = Report::default();
    // Two projects may cite `project:docs/PRODUCT.md` and mean different files, so notes are
    // only compared within a project.
    let mut items: BTreeMap<Option<String>, Vec<dedup::Item>> = BTreeMap::new();
    for row in sources[n].store.notes()? {
        report.total += 1;
        let note = row.meta.note.clone().unwrap_or_default();
        let checked = ctx.check(n, &note, row.meta.project.as_deref())?;
        if note.sources.is_empty()
            || (!checked.is_empty() && checked.iter().all(|c| c.state == State::Gone))
        {
            report.orphans.push(row.path.clone());
        }
        if note.sources.is_empty() || checked.iter().any(|c| c.state != State::Fresh) {
            report.stale.push(StaleNote {
                path: row.path.clone(),
                sources: checked,
            });
        }
        // Notes made from the same files are copies of one another; one with no sources is its own.
        let cited: BTreeSet<&str> = note.sources.iter().map(|c| c.path.as_str()).collect();
        let key = if cited.is_empty() {
            row.path.clone()
        } else {
            cited.into_iter().collect::<Vec<_>>().join("\n")
        };
        items
            .entry(row.meta.project.clone())
            .or_default()
            .push(dedup::Item {
                path: row.path,
                hash: key,
                date: row.date,
                fingerprint: row.simhash,
            });
    }
    report.duplicates = items
        .values()
        .flat_map(|items| dedup::copies(items))
        .map(|(path, of)| Duplicate { path, of })
        .collect();
    report.duplicates.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notes_frontmatter_is_read() {
        let note = from_front(
            "title: T\nsummary: Trips need approval\naliases: [trip rules, travel]\nquestions:\n  - How early?\n  - What for meals?\nsources:\n  - path: policies/travel.md\n    hash: abc123\n  - path: \"project:docs/X.md\"\n    hash: 'DEF'\nverified: 2025-03-01\n",
        )
        .unwrap();
        assert_eq!(note.summary, "Trips need approval");
        assert_eq!(note.aliases, ["trip rules", "travel"]);
        assert_eq!(note.questions, ["How early?", "What for meals?"]);
        assert_eq!(
            note.sources,
            [
                Cited {
                    path: "policies/travel.md".into(),
                    hash: "abc123".into()
                },
                Cited {
                    path: "project:docs/X.md".into(),
                    hash: "DEF".into()
                }
            ]
        );
        assert_eq!(note.verified.as_deref(), Some("2025-03-01"));
    }

    #[test]
    fn a_decisions_frontmatter_is_no_note_and_a_broken_note_has_no_sources() {
        assert_eq!(
            from_front("title: T\ndate: 2024-01-01\nkind: product\nsource: https://x/1\n"),
            None
        );
        // Not YAML, and not mended by quoting.
        assert_eq!(
            from_front("summary: [unclosed\nsources:\n  - path: a\n"),
            Some(Note::default())
        );
        // A source with no hash is no source.
        let note = from_front("sources:\n  - path: a.md\n  - path: b.md\n    hash: 12\n").unwrap();
        assert_eq!(
            note.sources,
            [Cited {
                path: "b.md".into(),
                hash: "12".into()
            }]
        );
    }

    #[test]
    fn a_colon_in_a_plain_value_does_not_make_a_note_unreadable() {
        let note = from_front(
            "title: Business trips: approval and meals\nsummary: Ten days ahead: always\nquestions:\n  - How early: for a trip?\n  - Plain one?\nsources:\n  - path: a.md\n    hash: abc\n",
        )
        .unwrap();
        assert_eq!(note.summary, "Ten days ahead: always");
        assert_eq!(note.questions, ["How early: for a trip?", "Plain one?"]);
        assert_eq!(note.sources.len(), 1);
    }

    #[test]
    fn the_questions_and_aliases_are_indexed_with_the_body() {
        let note = Note {
            summary: "Trips need approval.".into(),
            aliases: vec!["trip rules".into()],
            questions: vec!["How early must a trip be approved?".into()],
            ..Note::default()
        };
        let text = indexed_text(&note, "\nTen days ahead.\n");
        assert!(text.starts_with("Trips need approval.\n\n"));
        assert!(text.contains("- How early must a trip be approved?"));
        assert!(text.contains("Also called: trip rules"));
        assert!(text.ends_with("Ten days ahead.\n"));
        assert_eq!(indexed_text(&Note::default(), "Body"), "Body");
    }

    #[test]
    fn a_file_is_hashed_and_a_folder_or_a_pipe_is_not() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("a.md");
        fs::write(&file, "hello").unwrap();
        assert_eq!(
            sha_of_file(&file).as_deref(),
            Some("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")
        );
        assert_eq!(sha_of_file(dir.path()), None);
        assert_eq!(sha_of_file(&dir.path().join("missing")), None);
        // Opening a pipe would wait for a writer for ever.
        let pipe = dir.path().join("pipe");
        let name = std::ffi::CString::new(pipe.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert_eq!(sha_of_file(&pipe), None);
    }

    // What a stale note is, and how notes in other sources are told apart, runs through the
    // binary in tests/e2e.rs, which has the sources to check against.
}
