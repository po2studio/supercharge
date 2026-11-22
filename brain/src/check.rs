//! `brain check`: the gate of the brain repo's CI and the PR check of `build-slice`. It reads the
//! files agents own in the `update: true` source (`decisions/`, `notes/`, `BRAIN.md`) and never
//! writes, loads no model and needs no index.
//!
//! - `check` fails on invalid decision or note frontmatter, a broken link, or an accepted decision
//!   whose body (or any field but `status` and `superseded-by`) differs from when it was accepted,
//!   as git history says. A stale note, and a decision or note whose `touches:` match no file of
//!   the project, only warn.
//! - `check --changed` lists the decisions and notes whose `touches:` match the changed paths.

use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_yaml_ng::{Mapping, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

use crate::{
    doc::valid_date,
    notes::{self, Checked, State},
};

#[derive(Debug, Serialize)]
pub struct Problem {
    pub file: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct Stale {
    pub path: String,
    pub sources: Vec<Checked>,
}

/// An entry whose `touches:` match no file of its project.
#[derive(Debug, Serialize)]
pub struct Dangling {
    pub path: String,
    pub kind: &'static str,
    pub touches: Vec<String>,
}

/// An entry whose `touches:` match some of the changed paths.
#[derive(Debug, Serialize)]
pub struct Match {
    pub path: String,
    pub kind: &'static str,
    pub title: String,
    pub status: Option<String>,
    pub touches: Vec<String>,
    /// The changed paths that matched.
    pub files: Vec<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub brain: String,
    pub project: Option<String>,
    pub failures: Vec<Problem>,
    pub stale: Vec<Stale>,
    pub dangling: Vec<Dangling>,
    /// Citations of sources this run can't see (the project's, in the brain repo's CI): not checked.
    pub unchecked: usize,
    /// With `--changed`: the paths asked about, and what they touch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed: Option<Vec<String>>,
    pub matches: Vec<Match>,
}

/// The project the entries' `touches:` are globs in.
pub struct Project {
    pub name: String,
    pub root: PathBuf,
}

pub struct Opts<'a> {
    pub brain: &'a Path,
    /// Sources a note may cite as `<label>:<path>`; no root when this machine lacks the folder.
    pub sources: Vec<(String, Option<PathBuf>)>,
    pub project: Option<Project>,
    /// Paths, or one commit range, from `--changed`.
    pub changed: Option<Vec<String>>,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Decision,
    Note,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Decision => "decision",
            Kind::Note => "note",
        }
    }
}

/// A decision or note, read as far as it is valid.
struct Entry {
    path: String,
    kind: Kind,
    front: Mapping,
    body: String,
}

impl Entry {
    fn str(&self, key: &str) -> Option<String> {
        scalar(self.front.get(key)?).filter(|s| !s.is_empty())
    }

    fn touches(&self) -> Vec<String> {
        strings(self.front.get("touches"))
    }

    fn superseded(&self) -> bool {
        self.str("status").as_deref() == Some("superseded")
    }
}

pub fn run(o: &Opts) -> Result<Report> {
    let mut report = Report {
        brain: o.brain.display().to_string(),
        project: o.project.as_ref().map(|p| p.name.clone()),
        changed: None,
        ..Default::default()
    };
    let (entries, mut failures) = read_entries(o.brain)?;
    if let Some(args) = &o.changed {
        let project = o.project.as_ref().context(
            "--changed needs the project's folder: run inside the project, or give --repo",
        )?;
        let changed = changed_paths(args, &project.root)?;
        for e in &entries {
            let touches = e.touches();
            if e.superseded()
                || touches.is_empty()
                || e.str("project").as_deref() != Some(&project.name)
            {
                continue;
            }
            let files: Vec<String> = changed
                .iter()
                .filter(|c| touches.iter().any(|t| glob(t, c)))
                .cloned()
                .collect();
            if !files.is_empty() {
                report.matches.push(Match {
                    path: e.path.clone(),
                    kind: e.kind.name(),
                    title: e.str("title").unwrap_or_default(),
                    status: e.str("status"),
                    touches,
                    files,
                });
            }
        }
        report.changed = Some(changed);
        dangling(&entries, project, &mut report);
        return Ok(report);
    }

    for e in &entries {
        if e.kind == Kind::Decision {
            if let Some(reason) = edited_since_accepted(o.brain, e) {
                failures.push(Problem {
                    file: e.path.clone(),
                    reason,
                });
            }
        }
        if e.kind == Kind::Note {
            stale(o, e, &mut report);
        }
    }
    failures.sort_by(|a, b| a.file.cmp(&b.file));
    report.failures = failures;
    if let Some(project) = &o.project {
        dangling(&entries, project, &mut report);
    }
    Ok(report)
}

/// Every decision and note that parsed, and a problem for each that didn't or isn't valid, with
/// `BRAIN.md`'s and every entry's links checked.
fn read_entries(brain: &Path) -> Result<(Vec<Entry>, Vec<Problem>)> {
    let mut entries = Vec::new();
    let mut problems = Vec::new();
    let mut fail = |file: &str, reason: String| {
        problems.push(Problem {
            file: file.to_string(),
            reason,
        })
    };
    let decisions: Vec<String> = markdown_in(brain, "decisions");
    let mut names: Vec<String> = decisions
        .iter()
        .filter_map(|p| {
            Path::new(p)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
        })
        .collect();
    names.sort();
    let brain_md = brain.join("BRAIN.md");
    if brain_md.is_file() {
        let text = fs::read_to_string(&brain_md).context("reading BRAIN.md")?;
        for reason in broken_links("BRAIN.md", &text, brain) {
            fail("BRAIN.md", reason);
        }
    }
    for (kind, paths) in [
        (Kind::Decision, decisions),
        (Kind::Note, markdown_in(brain, "notes")),
    ] {
        for path in paths {
            let text =
                fs::read_to_string(brain.join(&path)).with_context(|| format!("reading {path}"))?;
            for reason in broken_links(&path, &text, brain) {
                fail(&path, reason);
            }
            let (front, body) = match split_front(&text) {
                Front::Missing => {
                    fail(
                        &path,
                        "frontmatter is missing: the file must start with ---".into(),
                    );
                    continue;
                }
                Front::Unclosed => {
                    fail(&path, "frontmatter is not closed by a --- line".into());
                    continue;
                }
                Front::Found(front, body) => (front, body),
            };
            let Some(map) = front_map(front) else {
                fail(
                    &path,
                    "frontmatter is not valid YAML (quote a value that holds a colon)".into(),
                );
                continue;
            };
            let entry = Entry {
                path: path.clone(),
                kind,
                front: map,
                body: body.to_string(),
            };
            let reasons = match kind {
                Kind::Decision => decision_problems(&entry, &names),
                Kind::Note => note_problems(&entry),
            };
            for reason in reasons {
                fail(&path, reason);
            }
            entries.push(entry);
        }
    }
    Ok((entries, problems))
}

/// The `.md` files under `folder` of the brain, as `/`-separated paths from its root, sorted.
fn markdown_in(brain: &Path, folder: &str) -> Vec<String> {
    fn walk(dir: &Path, rel: &str, out: &mut Vec<String>) {
        let Ok(read) = fs::read_dir(dir) else { return };
        for e in read.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let rel = format!("{rel}/{name}");
            let Ok(t) = e.file_type() else { continue };
            if t.is_dir() {
                if name != ".git" {
                    walk(&e.path(), &rel, out);
                }
            } else if name.ends_with(".md") || name.ends_with(".markdown") {
                out.push(rel);
            }
        }
    }
    let mut out = Vec::new();
    walk(&brain.join(folder), folder, &mut out);
    out.sort();
    out
}

enum Front<'a> {
    Missing,
    Unclosed,
    Found(&'a str, &'a str),
}

/// The leading `---` block and what follows it.
fn split_front(text: &str) -> Front<'_> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return Front::Missing;
    };
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return Front::Found(&rest[..offset], &rest[offset + line.len()..]);
        }
        offset += line.len();
    }
    Front::Unclosed
}

/// The frontmatter as a YAML mapping; a plain value holding `: ` is read as the quoted text it
/// was meant to be, as the indexer does.
fn front_map(front: &str) -> Option<Mapping> {
    let parse = |t: &str| match serde_yaml_ng::from_str::<Value>(t) {
        Ok(Value::Mapping(m)) => Some(m),
        _ => None,
    };
    parse(front).or_else(|| parse(&notes::requoted(front)))
}

fn scalar(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// A list of strings; a lone string is a list of one.
fn strings(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::Sequence(items)) => items
            .iter()
            .filter_map(scalar)
            .filter(|s| !s.is_empty())
            .collect(),
        Some(v) => scalar(v).into_iter().filter(|s| !s.is_empty()).collect(),
        None => Vec::new(),
    }
}

/// A required, non-empty string.
fn require(e: &Entry, key: &str, out: &mut Vec<String>) -> Option<String> {
    let v = e.str(key);
    if v.is_none() {
        out.push(format!("{key} is missing"));
    }
    v
}

fn require_date(e: &Entry, key: &str, out: &mut Vec<String>) {
    if let Some(d) = require(e, key, out) {
        if valid_date(&d).as_deref() != Some(d.as_str()) || d.len() != 10 {
            out.push(format!("{key} must be YYYY-MM-DD, not {d}"));
        }
    }
}

fn require_one_of(e: &Entry, key: &str, allowed: &[&str], out: &mut Vec<String>) -> Option<String> {
    let v = require(e, key, out)?;
    if !allowed.contains(&v.as_str()) {
        out.push(format!("{key} must be {}, not {v}", join_or(allowed)));
        return None;
    }
    Some(v)
}

fn join_or(items: &[&str]) -> String {
    match items {
        [rest @ .., last] if !rest.is_empty() => format!("{} or {last}", rest.join(", ")),
        _ => items.join(""),
    }
}

/// `touches` are globs in a project, so they need one.
fn touches_problems(e: &Entry, out: &mut Vec<String>) {
    let has = e.front.get("touches").is_some();
    if let Some(v) = e.front.get("touches") {
        let ok = match v {
            Value::Sequence(items) => items
                .iter()
                .all(|i| matches!(i, Value::String(s) if !s.trim().is_empty())),
            Value::String(s) => !s.trim().is_empty(),
            _ => false,
        };
        if !ok {
            out.push("touches must be a list of globs".into());
        }
    }
    if has && !e.touches().is_empty() && e.str("project").is_none() {
        out.push("touches needs project: they are globs in that project".into());
    }
}

fn decision_problems(e: &Entry, names: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let stem = Path::new(&e.path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    if valid_date(&stem).is_none() || stem.len() < 12 || stem.as_bytes()[10] != b'-' {
        out.push("a decision is named YYYY-MM-DD-slug.md".into());
    }
    require(e, "title", &mut out);
    require_date(e, "date", &mut out);
    require_one_of(e, "kind", &["business", "product", "process"], &mut out);
    let status = require_one_of(
        e,
        "status",
        &["proposed", "accepted", "superseded"],
        &mut out,
    );
    require(e, "decided-by", &mut out);
    require(e, "source", &mut out);
    touches_problems(e, &mut out);
    let known = |n: &str| {
        let n = n.strip_suffix(".md").unwrap_or(n);
        names.iter().any(|k| k == n)
    };
    for n in strings(e.front.get("supersedes")) {
        if !known(&n) {
            out.push(format!(
                "supersedes names {n}, which is not a decision here"
            ));
        }
    }
    match (e.str("superseded-by"), status.as_deref()) {
        (None, Some("superseded")) => {
            out.push("superseded-by is missing: status is superseded".into())
        }
        (Some(n), _) if !known(&n) => out.push(format!(
            "superseded-by names {n}, which is not a decision here"
        )),
        _ => {}
    }
    out
}

fn note_problems(e: &Entry) -> Vec<String> {
    let mut out = Vec::new();
    require(e, "title", &mut out);
    if let Some(s) = require(e, "summary", &mut out) {
        if s.contains('\n') {
            out.push("summary must be one line".into());
        }
    }
    if strings(e.front.get("questions")).is_empty() {
        out.push("questions is missing: list the questions the note answers".into());
    }
    if let Some(v) = e.front.get("aliases") {
        if !matches!(v, Value::Sequence(_)) {
            out.push("aliases must be a list".into());
        }
    }
    require_date(e, "verified", &mut out);
    match e.front.get("sources") {
        Some(Value::Sequence(items)) => {
            for (i, item) in items.iter().enumerate() {
                let path = item.get("path").and_then(scalar).filter(|p| !p.is_empty());
                if path.is_none() {
                    out.push(format!("sources[{i}] has no path"));
                }
                match item.get("hash").and_then(scalar) {
                    Some(h) if is_sha256(&h) => {}
                    Some(h) => out.push(format!("sources[{i}] hash {h} is not a sha256")),
                    None => out.push(format!("sources[{i}] has no hash")),
                }
            }
        }
        _ => out.push("sources is missing: a list of path and hash".into()),
    }
    touches_problems(e, &mut out);
    out
}

fn is_sha256(h: &str) -> bool {
    let h = h.trim();
    let h = h.strip_prefix("sha256:").unwrap_or(h);
    h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The relative links of `text` whose target is not a file or folder of the brain. Links inside
/// code, and ones that go to a URL or within the page, are not looked at.
fn broken_links(file: &str, text: &str, brain: &Path) -> Vec<String> {
    let dir = Path::new(file).parent().unwrap_or(Path::new(""));
    let mut broken = Vec::new();
    let mut fenced = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        // Inline code is every second piece between backticks.
        let prose: String = line.split('`').step_by(2).collect::<Vec<_>>().join(" ");
        let mut rest = prose.as_str();
        while let Some(at) = rest.find("](") {
            rest = &rest[at + 2..];
            let (target, used) = destination(rest);
            rest = &rest[used.min(rest.len())..];
            let path = target.split(['#', '?']).next().unwrap_or("");
            let is_url = path.split_once(':').is_some_and(|(s, _)| {
                !s.is_empty()
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'.' || b == b'-')
            });
            if path.is_empty() || is_url {
                continue;
            }
            let path = percent_decode(path);
            if !target_exists(brain, dir, &path) {
                broken.push(format!("broken link {path}"));
            }
        }
    }
    broken.dedup();
    broken
}

/// The link destination at the start of `s`, which follows `](`, and how much of `s` it took:
/// `<…>` may hold spaces; otherwise it runs to the first space or the `)` that closes the link,
/// with balanced parentheses inside, and `\` before ASCII punctuation as an escape.
fn destination(s: &str) -> (String, usize) {
    let trimmed = s.trim_start();
    let skipped = s.len() - trimmed.len();
    if let Some(inner) = trimmed.strip_prefix('<') {
        let end = inner.find('>').unwrap_or(inner.len());
        return (inner[..end].to_string(), skipped + 1 + end);
    }
    let (mut out, mut depth) = (String::new(), 0);
    let mut chars = trimmed.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => match chars.next_if(|(_, n)| n.is_ascii_punctuation()) {
                Some((_, n)) => out.push(n),
                None => out.push(c),
            },
            '(' => {
                depth += 1;
                out.push(c);
            }
            ')' if depth == 0 => return (out, skipped + i),
            ')' => {
                depth -= 1;
                out.push(c);
            }
            c if c.is_whitespace() => return (out, skipped + i),
            c => out.push(c),
        }
    }
    (out, s.len())
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            if let Some(v) = s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
            {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether `target`, relative to `dir` (a folder of the brain), is something in the brain.
fn target_exists(brain: &Path, dir: &Path, target: &str) -> bool {
    let mut path = dir.to_path_buf();
    if let Some(abs) = target.strip_prefix('/') {
        path = PathBuf::from(abs);
    } else {
        path.push(target);
    }
    let mut parts: Vec<&std::ffi::OsStr> = Vec::new();
    for c in path.components() {
        match c {
            Component::Normal(p) => parts.push(p),
            Component::ParentDir if parts.pop().is_none() => return false,
            Component::ParentDir => {}
            _ => {}
        }
    }
    brain.join(parts.iter().collect::<PathBuf>()).exists()
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The versions of `rel` (a path in the brain) in git history, oldest first, as commit and the
/// file's path in the repo at that commit. A commit that renamed another file into it carries the
/// history on under the old name (git's own rename detection, between files of that one commit:
/// `log --follow` takes a new file for a copy of any similar one).
fn history(brain: &Path, rel: &str) -> Option<Vec<(String, String)>> {
    let top = git(brain, &["rev-parse", "--show-toplevel"])?;
    let top = Path::new(top.trim());
    let prefix = git(brain, &["rev-parse", "--show-prefix"])?;
    let mut path = format!("{}{rel}", prefix.trim());
    let mut rev = "HEAD".to_string();
    let mut versions: Vec<(String, String)> = Vec::new();
    // A decision is renamed once in a blue moon; the bound only stops a loop.
    for _ in 0..20 {
        let log = git(top, &["log", "--format=%H", &rev, "--", &path])?;
        let Some(oldest) = log.lines().last() else {
            break;
        };
        versions.extend(log.lines().map(|sha| (sha.to_string(), path.clone())));
        let diff = git(
            top,
            &[
                "-c",
                "core.quotepath=off",
                "diff-tree",
                "--root",
                "-r",
                "-M",
                "--name-status",
                "--no-commit-id",
                oldest,
            ],
        )?;
        let from = diff.lines().find_map(|line| {
            let mut f = line.split('\t');
            f.next()?.starts_with('R').then_some(())?;
            let old = f.next()?;
            (f.next()? == path).then(|| old.to_string())
        });
        let Some(from) = from else { break };
        rev = format!("{oldest}^");
        path = from;
    }
    versions.reverse();
    Some(versions)
}

/// What an accepted decision's file says now that it must not: its body, or a field other than
/// `status` and `superseded-by`, compared with the first version in git history that was
/// accepted. A brain that isn't a git repo, a decision with no history and one never accepted
/// have nothing to compare.
fn edited_since_accepted(brain: &Path, now: &Entry) -> Option<String> {
    let top = git(brain, &["rev-parse", "--show-toplevel"])?;
    let top = Path::new(top.trim());
    let accepted = history(brain, &now.path)?
        .into_iter()
        .find_map(|(sha, path)| {
            let text = git(top, &["show", &format!("{sha}:{path}")])?;
            let Front::Found(front, body) = split_front(&text) else {
                return None;
            };
            let map = front_map(front)?;
            let status = map.get("status").and_then(scalar)?;
            matches!(status.as_str(), "accepted" | "superseded")
                .then(|| (sha, map, body.trim().to_string()))
        });
    let (sha, then, body) = accepted?;
    let may_change = |m: &Mapping| -> BTreeMap<String, Value> {
        m.iter()
            .filter_map(|(k, v)| Some((k.as_str()?.to_string(), v.clone())))
            .filter(|(k, _)| k != "status" && k != "superseded-by")
            .collect()
    };
    let (a, b) = (may_change(&then), may_change(&now.front));
    let mut changed: Vec<String> = Vec::new();
    if body != now.body.trim() {
        changed.push("the body".into());
    }
    changed.extend(
        a.keys()
            .chain(b.keys())
            .filter(|k| a.get(*k) != b.get(*k))
            .cloned()
            .collect::<std::collections::BTreeSet<_>>(),
    );
    (!changed.is_empty()).then(|| {
        format!(
            "accepted decision edited: {} changed since {} (only status and superseded-by may change; to change a decision, write a new one)",
            changed.join(", "),
            &sha[..7.min(sha.len())]
        )
    })
}

/// A note is stale when a file it cites changed or is gone, or it cites none. A file of the
/// brain is cited by its path, another source's as `<label>:<path>`; a label this run doesn't
/// know belongs to a source it can't see, and isn't judged.
fn stale(o: &Opts, note: &Entry, report: &mut Report) {
    let cited = match note.front.get("sources") {
        Some(Value::Sequence(items)) => items.clone(),
        _ => return,
    };
    if cited.is_empty() {
        report.stale.push(Stale {
            path: note.path.clone(),
            sources: Vec::new(),
        });
        return;
    }
    let mut checked = Vec::new();
    for item in cited {
        let (Some(path), Some(hash)) = (
            item.get("path").and_then(scalar),
            item.get("hash").and_then(scalar),
        ) else {
            continue;
        };
        let labelled = o
            .sources
            .iter()
            .filter_map(|(label, root)| {
                path.strip_prefix(label.as_str())
                    .and_then(|r| r.strip_prefix(':'))
                    .map(|rel| (label.as_str(), root, rel))
            })
            .max_by_key(|(label, ..)| label.len());
        // Two projects may cite `project:docs/PRODUCT.md` and mean different files.
        let other_project = |label: &str| {
            label == crate::sources::PROJECT
                && note.str("project").is_some_and(|p| {
                    o.project.as_ref().map(|c| c.name.as_str()) != Some(p.as_str())
                })
        };
        let (root, rel) = match labelled {
            Some((label, ..)) if other_project(label) => {
                report.unchecked += 1;
                continue;
            }
            Some((_, Some(root), rel)) => (root.as_path(), rel),
            // A listed folder this machine lacks, or a label nobody here lists.
            Some((_, None, _)) => {
                report.unchecked += 1;
                continue;
            }
            None if path.contains(':') => {
                report.unchecked += 1;
                continue;
            }
            None => (o.brain, path.as_str()),
        };
        let current = file_hash(root, rel);
        let recorded = hash.trim().to_lowercase();
        let recorded = recorded.strip_prefix("sha256:").unwrap_or(&recorded);
        let state = match &current {
            None => State::Gone,
            Some(now) if now == recorded => State::Fresh,
            Some(_) => State::Changed,
        };
        checked.push(Checked {
            path,
            hash,
            state,
            current,
        });
    }
    if checked.iter().any(|c| c.state != State::Fresh) {
        report.stale.push(Stale {
            path: note.path.clone(),
            sources: checked,
        });
    }
}

/// The sha256 of `rel` under `root`; none for a path that leaves it or isn't a regular file.
fn file_hash(root: &Path, rel: &str) -> Option<String> {
    let mut path = root.to_path_buf();
    for part in Path::new(rel).components() {
        match part {
            Component::Normal(p) => path.push(p),
            _ => return None,
        }
    }
    notes::sha_of_file(&path)
}

/// The files of the project: what git tracks or would, else everything outside `.git`.
fn project_files(root: &Path) -> Vec<String> {
    if root.join(".git").exists() {
        if let Some(out) = git(
            root,
            &[
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
            ],
        ) {
            return out
                .split('\0')
                .filter(|f| !f.is_empty() && root.join(f).exists())
                .map(str::to_string)
                .collect();
        }
    }
    fn walk(dir: &Path, rel: &str, out: &mut Vec<String>) {
        let Ok(read) = fs::read_dir(dir) else { return };
        for e in read.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let rel = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            match e.file_type() {
                Ok(t) if t.is_dir() => {
                    if name != ".git" {
                        walk(&e.path(), &rel, out)
                    }
                }
                Ok(_) => out.push(rel),
                Err(_) => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(root, "", &mut out);
    out
}

/// Entries of the project that are not history and whose `touches:` match no file in it.
fn dangling(entries: &[Entry], project: &Project, report: &mut Report) {
    let mut files: Option<Vec<String>> = None;
    for e in entries {
        let touches = e.touches();
        if e.superseded()
            || touches.is_empty()
            || e.str("project").as_deref() != Some(&project.name)
        {
            continue;
        }
        let files = files.get_or_insert_with(|| project_files(&project.root));
        if !touches.iter().any(|t| files.iter().any(|f| glob(t, f))) {
            report.dangling.push(Dangling {
                path: e.path.clone(),
                kind: e.kind.name(),
                touches,
            });
        }
    }
}

/// The paths `--changed` names: the arguments themselves (relative to the project), or, for one
/// argument that is a commit range (`abc123..HEAD`), every path any commit in it touched.
fn changed_paths(args: &[String], project: &Path) -> Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    match args {
        [range] if range.contains("..") && !project.join(range).exists() => {
            // `--cc` adds what a merge changed on its own; `-z` leaves odd names unquoted.
            let log = Command::new("git")
                .arg("-C")
                .arg(project)
                .args([
                    "log",
                    "--cc",
                    "-z",
                    "--no-renames",
                    "--name-only",
                    "--format=",
                    range,
                    "--",
                ])
                .output()
                .context("running git")?;
            if !log.status.success() {
                bail!(
                    "{range} is not a commit range of the project: {}",
                    String::from_utf8_lossy(&log.stderr).trim()
                );
            }
            out = String::from_utf8_lossy(&log.stdout)
                .split('\0')
                .map(|p| p.trim_matches('\n'))
                .filter(|p| !p.is_empty())
                .map(str::to_string)
                .collect();
        }
        _ => {
            for a in args {
                let p = Path::new(a);
                let rel = p.strip_prefix(project).unwrap_or(p);
                let rel = rel.strip_prefix("./").unwrap_or(rel);
                out.push(rel.to_string_lossy().into_owned());
            }
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// Whether `path` (`/`-separated, relative to the project) matches the glob `pattern`: `*` and
/// `?` stay within a folder, `**` spans folders, and a plain path also covers what is under it.
pub fn glob(pattern: &str, path: &str) -> bool {
    let pattern = pattern
        .trim()
        .trim_start_matches("./")
        .trim_start_matches('/');
    let pattern = pattern.trim_end_matches('/');
    if !pattern.contains(['*', '?']) {
        return path == pattern
            || path
                .strip_prefix(pattern)
                .is_some_and(|rest| rest.starts_with('/'));
    }
    fn segments(pat: &[&str], path: &[&str]) -> bool {
        match pat.split_first() {
            None => path.is_empty(),
            Some((&"**", rest)) => {
                segments(rest, path) || (!path.is_empty() && segments(pat, &path[1..]))
            }
            Some((p, rest)) => {
                !path.is_empty()
                    && segment(p.as_bytes(), path[0].as_bytes())
                    && segments(rest, &path[1..])
            }
        }
    }
    fn segment(pat: &[u8], text: &[u8]) -> bool {
        match pat.split_first() {
            None => text.is_empty(),
            Some((b'*', rest)) => (0..=text.len()).any(|i| segment(rest, &text[i..])),
            Some((b'?', rest)) => !text.is_empty() && segment(rest, &text[1..]),
            Some((c, rest)) => text.first() == Some(c) && segment(rest, &text[1..]),
        }
    }
    let pat: Vec<&str> = pattern.split('/').collect();
    let path: Vec<&str> = path.split('/').collect();
    segments(&pat, &path)
}

impl Report {
    pub fn warnings(&self) -> usize {
        self.stale.len() + self.dangling.len()
    }

    /// The report as a person reads it.
    pub fn render(&self) -> String {
        let mut out = String::new();
        if let Some(changed) = &self.changed {
            for m in &self.matches {
                let status = m
                    .status
                    .as_deref()
                    .map(|s| format!(", {s}"))
                    .unwrap_or_default();
                let shown: Vec<&str> = m.files.iter().take(5).map(String::as_str).collect();
                let more = if m.files.len() > 5 {
                    format!(" and {} more", m.files.len() - 5)
                } else {
                    String::new()
                };
                out += &format!(
                    "{} ({}{status}): {}; touches {}; matched {}{more}\n",
                    m.path,
                    m.kind,
                    m.title,
                    m.touches.join(", "),
                    shown.join(", ")
                );
            }
            if self.matches.is_empty() {
                out += &format!(
                    "no decision or note touches the {} changed paths\n",
                    changed.len()
                );
            }
        } else {
            for f in &self.failures {
                out += &format!("FAIL {}: {}\n", f.file, f.reason);
            }
            for s in &self.stale {
                let what: Vec<String> = s
                    .sources
                    .iter()
                    .filter(|c| c.state != State::Fresh)
                    .map(|c| format!("{} {}", c.path, c.state))
                    .collect();
                let what = if what.is_empty() {
                    "it cites no source".to_string()
                } else {
                    what.join(", ")
                };
                out += &format!("warning: {}: stale: {what}\n", s.path);
            }
        }
        let project = self.project.as_deref().unwrap_or("the project");
        for d in &self.dangling {
            out += &format!(
                "warning: {}: touches match no file in {project}: {}\n",
                d.path,
                d.touches.join(", ")
            );
        }
        if self.changed.is_none() {
            out += &format!(
                "brain check: {} failures, {} warnings",
                self.failures.len(),
                self.warnings()
            );
            if self.unchecked > 0 {
                out += &format!(
                    "; {} cited files of sources this run can't see were not checked",
                    self.unchecked
                );
            }
            out += "\n";
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_match_like_paths_in_a_repo() {
        assert!(glob("src/signup/**", "src/signup/form.ts"));
        assert!(glob("src/signup/**", "src/signup/deep/er/form.ts"));
        assert!(!glob("src/signup/**", "src/signups/form.ts"));
        assert!(!glob("src/signup/**", "src/billing/form.ts"));
        assert!(glob("src/*.ts", "src/main.ts"));
        assert!(!glob("src/*.ts", "src/signup/form.ts"));
        assert!(glob("**/*.sql", "db/migrations/001.sql"));
        assert!(glob("**/*.sql", "001.sql"));
        assert!(glob("src/**/form.ts", "src/form.ts"));
        assert!(glob("src/**/form.ts", "src/a/b/form.ts"));
        assert!(glob("src/?.ts", "src/a.ts"));
        assert!(!glob("src/?.ts", "src/ab.ts"));
        // A plain path covers itself and what is under it.
        assert!(glob("src/signup", "src/signup/form.ts"));
        assert!(glob("src/signup/", "src/signup/form.ts"));
        assert!(glob("src/main.ts", "src/main.ts"));
        assert!(!glob("src/main.ts", "src/main.tsx"));
        assert!(glob("./src/main.ts", "src/main.ts"));
    }

    #[test]
    fn links_to_code_urls_and_anchors_are_left_alone() {
        let dir = tempfile::TempDir::new().unwrap();
        fs::write(dir.path().join("there.md"), "x").unwrap();
        let text = "[a](there.md#top) [b](https://x.y/z) [c](#here) `[d](nope.md)`\n\n```\n[e](nope.md)\n```\n[f](gone.md) [g](<there.md>) [h](there%2Emd)\n";
        assert_eq!(
            broken_links("n.md", text, dir.path()),
            vec!["broken link gone.md"]
        );
        // Relative to the file, not the brain.
        fs::create_dir(dir.path().join("notes")).unwrap();
        assert!(broken_links("notes/n.md", "[x](../there.md)", dir.path()).is_empty());
        assert_eq!(
            broken_links("notes/n.md", "[x](there.md)", dir.path()),
            vec!["broken link there.md"]
        );
        assert_eq!(
            broken_links("notes/n.md", "[x](../../../etc/passwd)", dir.path()),
            vec!["broken link ../../../etc/passwd"]
        );
    }

    #[test]
    fn a_backslash_escapes_only_punctuation_in_a_link_destination() {
        assert_eq!(destination(r"../a\b.md) rest").0, r"../a\b.md");
        assert_eq!(destination(r"../a\(b\).md) rest").0, "../a(b).md");
        assert_eq!(destination("a(b).md c").0, "a(b).md");
        assert_eq!(destination("<a b.md> c").0, "a b.md");
    }

    #[test]
    fn a_sha256_is_64_hex_digits_with_an_optional_prefix() {
        let h = "a".repeat(64);
        assert!(is_sha256(&h));
        assert!(is_sha256(&format!("sha256:{h}")));
        assert!(!is_sha256(&h[1..]));
        assert!(!is_sha256(&"g".repeat(64)));
    }
}
