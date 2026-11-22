//! `brain check` through the CLI: the gate of the brain repo's CI and the PR check of
//! `build-slice`. Nothing here loads the model; every run is a plain process on temp folders.

use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn sha256(text: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

/// A brain that is a git repo, a project that is another, and a cache nothing should reach.
struct Fx {
    brain: TempDir,
    project: TempDir,
    cache: TempDir,
}

const POLICY: &str = "# Travel\n\nA trip needs approval ten days ahead.\n";

fn decision(status: &str, body: &str, extra: &str) -> String {
    format!(
        "---\ntitle: Second phone field on signup\ndate: 2024-05-14\nkind: business\nproject: courier-app\nstatus: {status}\ndecided-by: owner\ntouches: [src/signup/**]\nsource: https://github.com/acme/courier-app/issues/42\n{extra}---\n\n{body}\n"
    )
}

fn note(summary: &str, hash: &str, extra: &str) -> String {
    format!(
        "---\ntitle: Business trips\nsummary: {summary}\naliases: [trip rules]\nquestions:\n  - How early must a trip be approved?\nsources:\n  - path: policies/travel.md\n    hash: {hash}\nverified: 2025-02-10\n{extra}---\n\nApproval ten days ahead.\n"
    )
}

impl Fx {
    fn new() -> Fx {
        let fx = Fx {
            brain: TempDir::new().unwrap(),
            project: TempDir::new().unwrap(),
            cache: TempDir::new().unwrap(),
        };
        git(fx.brain.path(), &["init", "-q"]);
        git(fx.project.path(), &["init", "-q"]);
        fx.put("BRAIN.md", "# Brain\n\nFormat version 1.\n");
        fx.put("policies/travel.md", POLICY);
        fx.put("decisions/.gitkeep", "");
        fx.put("notes/.gitkeep", "");
        fx.put("projects/.gitkeep", "");
        fx.put(
            "decisions/2024-05-14-second-phone.md",
            &decision("accepted", "Context, decision, consequences.", ""),
        );
        fx.put(
            "notes/business-trips.md",
            &note("Trips need approval", &sha256(POLICY), ""),
        );
        fx.commit("brain");
        fs::create_dir_all(fx.project.path().join("src/signup")).unwrap();
        fs::write(fx.project.path().join("src/signup/form.ts"), "form").unwrap();
        fs::write(fx.project.path().join("src/main.ts"), "main").unwrap();
        git(fx.project.path(), &["add", "-A"]);
        git(fx.project.path(), &["commit", "-qm", "first"]);
        fx
    }

    fn put(&self, rel: &str, text: &str) {
        let p = self.brain.path().join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }

    fn commit(&self, msg: &str) {
        git(self.brain.path(), &["add", "-A"]);
        git(self.brain.path(), &["commit", "-qm", msg]);
    }

    fn run(&self, cwd: &Path, flags: &[&str], args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_brain"))
            .current_dir(cwd)
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("SUPERCHARGE_CACHE_DIR", self.cache.path())
            .args(flags)
            .args(args)
            .output()
            .unwrap()
    }

    /// Inside the brain repo, as its CI runs it.
    fn check(&self, args: &[&str]) -> Output {
        let mut all = vec!["check"];
        all.extend_from_slice(args);
        self.run(self.brain.path(), &[], &all)
    }

    /// Inside the project, with the brain as a source.
    fn check_in_project(&self, args: &[&str]) -> Output {
        let mut all = vec!["check"];
        all.extend_from_slice(args);
        self.run(
            self.project.path(),
            &["--source", self.brain.path().to_str().unwrap()],
            &all,
        )
    }
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn json(o: &Output) -> Value {
    assert!(o.status.success(), "{}", text(o));
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("not JSON ({e}): {}", text(o)))
}

fn paths(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|e| e["path"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn a_valid_brain_passes() {
    let fx = Fx::new();
    let out = fx.check(&[]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("0 failures"), "{}", text(&out));
}

/// Each way an entry can be invalid makes `check` fail, naming the file and the reason.
#[test]
fn each_failure_case_names_the_file_and_the_reason() {
    let ok = decision("accepted", "Context.", "");
    let cases: Vec<(&str, String, &str)> = vec![
        (
            "decisions/2024-06-01-no-kind.md",
            ok.replace("kind: business\n", ""),
            "kind",
        ),
        (
            "decisions/2024-06-01-bad-kind.md",
            ok.replace("kind: business", "kind: legal"),
            "kind",
        ),
        (
            "decisions/2024-06-01-bad-status.md",
            ok.replace("status: accepted", "status: maybe"),
            "status",
        ),
        (
            "decisions/2024-06-01-no-title.md",
            ok.replace("title: Second phone field on signup\n", ""),
            "title",
        ),
        (
            "decisions/2024-06-01-no-decider.md",
            ok.replace("decided-by: owner\n", ""),
            "decided-by",
        ),
        (
            "decisions/2024-06-01-no-source.md",
            ok.replace(
                "source: https://github.com/acme/courier-app/issues/42\n",
                "",
            ),
            "source",
        ),
        (
            "decisions/2024-06-01-bad-date.md",
            ok.replace("date: 2024-05-14", "date: May 14"),
            "date",
        ),
        (
            "decisions/2024-06-01-touches-no-project.md",
            ok.replace("project: courier-app\n", ""),
            "touches needs project",
        ),
        (
            "decisions/2024-06-01-superseded-by-nothing.md",
            ok.replace("status: accepted", "status: superseded"),
            "superseded-by",
        ),
        (
            "decisions/2024-06-01-supersedes-nothing.md",
            decision(
                "accepted",
                "Context.",
                "supersedes: [2020-01-01-never-written]\n",
            ),
            "2020-01-01-never-written",
        ),
        (
            "decisions/2024-06-01-no-frontmatter.md",
            "Just text, no frontmatter.\n".to_string(),
            "frontmatter",
        ),
        (
            "decisions/2024-06-01-broken-yaml.md",
            "---\ntitle: [unclosed\nkind: business\n---\n\nBody.\n".to_string(),
            "frontmatter",
        ),
        (
            "decisions/no-date-in-name.md",
            ok.clone(),
            "named YYYY-MM-DD-slug.md",
        ),
        (
            "decisions/2024-06-01-broken-link.md",
            decision("accepted", "See [the form](../src/gone.md).", ""),
            "broken link ../src/gone.md",
        ),
        (
            "notes/no-sources.md",
            note("s", &sha256(POLICY), "").replace(
                "sources:\n  - path: policies/travel.md\n    hash: ",
                "oldsources: ",
            ),
            "sources",
        ),
        ("notes/bad-hash.md", note("s", "not-a-hash", ""), "hash"),
        (
            "notes/no-summary.md",
            note("s", &sha256(POLICY), "").replace("summary: s\n", ""),
            "summary",
        ),
        (
            "notes/no-questions.md",
            note("s", &sha256(POLICY), "")
                .replace("questions:\n  - How early must a trip be approved?\n", ""),
            "questions",
        ),
        (
            "notes/bad-verified.md",
            note("s", &sha256(POLICY), "").replace("verified: 2025-02-10", "verified: soon"),
            "verified",
        ),
        (
            "notes/touches-no-project.md",
            note("s", &sha256(POLICY), "touches: [src/**]\n"),
            "touches needs project",
        ),
        (
            "notes/broken-link.md",
            note("s", &sha256(POLICY), "").replace("Approval", "See [it](missing.md). Approval"),
            "broken link missing.md",
        ),
    ];
    for (path, content, reason) in cases {
        let fx = Fx::new();
        fx.put(path, &content);
        let out = fx.check(&[]);
        let t = text(&out);
        assert!(!out.status.success(), "{path} should fail: {t}");
        assert!(t.contains(path), "{path} not named: {t}");
        assert!(t.contains(reason), "{path}: reason {reason:?} not in: {t}");
        // Nothing else is blamed.
        assert_eq!(
            t.lines().filter(|l| l.starts_with("FAIL ")).count(),
            1,
            "{path}: {t}"
        );
    }
}

#[test]
fn links_in_code_and_outside_the_brain_are_not_broken_links() {
    let fx = Fx::new();
    fx.put(
        "decisions/2024-06-01-links.md",
        &decision(
            "accepted",
            "See [the policy](../policies/travel.md#top), [the issue](https://example.com/x), [mail](mailto:a@b.c), [here](#context) and `[code](nowhere.md)`.\n\n```\n[fenced](nowhere.md)\n```",
            "",
        ),
    );
    let out = fx.check(&[]);
    assert!(out.status.success(), "{}", text(&out));
}

#[test]
fn a_stale_note_only_warns() {
    let fx = Fx::new();
    fx.put(
        "policies/travel.md",
        "# Travel\n\nApproval is now five days.\n",
    );
    let out = fx.check(&[]);
    assert!(out.status.success(), "{}", text(&out));
    let t = text(&out);
    assert!(
        t.contains("warning: notes/business-trips.md: stale")
            && t.contains("policies/travel.md changed"),
        "{t}"
    );
    let v = json(&fx.check(&["--json"]));
    assert_eq!(v["failures"].as_array().unwrap().len(), 0);
    assert_eq!(paths(&v["stale"]), ["notes/business-trips.md"]);
    // Gone is stale too.
    fs::remove_file(fx.brain.path().join("policies/travel.md")).unwrap();
    let v = json(&fx.check(&["--json"]));
    assert_eq!(v["stale"][0]["sources"][0]["state"], "gone");
}

#[test]
fn a_note_that_cites_a_source_this_check_cannot_see_is_not_called_stale() {
    let fx = Fx::new();
    fx.put(
        "notes/from-the-project.md",
        &note("s", &sha256("whatever"), "")
            .replace("path: policies/travel.md", "path: project:docs/PRODUCT.md"),
    );
    let v = json(&fx.check(&["--json"]));
    assert_eq!(v["failures"].as_array().unwrap().len(), 0);
    assert_eq!(v["stale"].as_array().unwrap().len(), 0);
}

#[test]
fn editing_an_accepted_decisions_body_fails_but_status_and_superseded_by_may_change() {
    let fx = Fx::new();
    let name = "decisions/2024-05-14-second-phone.md";
    let edited = decision(
        "accepted",
        "Context, decision, consequences. Edited later.",
        "",
    );
    // Not committed yet: still caught, against what history says it was.
    fx.put(name, &edited);
    let out = fx.check(&[]);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains(&format!("FAIL {name}: accepted decision edited: the body")),
        "{}",
        text(&out)
    );
    // Committed, still caught.
    fx.commit("edit");
    assert!(!fx.check(&[]).status.success());
    // An edit of another field counts too, apart from the two that may change.
    fx.put(
        name,
        &decision("accepted", "Context, decision, consequences.", "").replace("owner", "someone"),
    );
    let out = fx.check(&[]);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("decided-by"), "{}", text(&out));
    // Put back, it passes again: a revert fixes it.
    fx.put(
        name,
        &decision("accepted", "Context, decision, consequences.", ""),
    );
    assert!(fx.check(&[]).status.success());
    // Superseding changes only status and superseded-by, and the new decision lists what it replaces.
    fx.put(
        name,
        &decision(
            "superseded",
            "Context, decision, consequences.",
            "superseded-by: 2026-01-10-sms-fallback\n",
        ),
    );
    fx.put(
        "decisions/2026-01-10-sms-fallback.md",
        &decision(
            "accepted",
            "Fall back to SMS.",
            "supersedes: [2024-05-14-second-phone]\n",
        )
        .replace("date: 2024-05-14", "date: 2026-01-10"),
    );
    let out = fx.check(&[]);
    assert!(out.status.success(), "{}", text(&out));
    fx.commit("supersede");
    assert!(fx.check(&[]).status.success());
}

#[test]
fn a_proposed_decision_may_still_change_until_it_is_accepted() {
    let fx = Fx::new();
    let name = "decisions/2024-07-01-draft.md";
    fx.put(name, &decision("proposed", "First draft.", ""));
    fx.commit("draft");
    fx.put(name, &decision("proposed", "Second draft.", ""));
    fx.commit("draft 2");
    fx.put(name, &decision("accepted", "Second draft.", ""));
    fx.commit("accepted");
    assert!(fx.check(&[]).status.success());
    fx.put(name, &decision("accepted", "Third draft.", ""));
    assert!(!fx.check(&[]).status.success());
}

#[test]
fn a_brain_that_is_not_a_git_repo_skips_the_history_check() {
    let fx = Fx::new();
    fs::remove_dir_all(fx.brain.path().join(".git")).unwrap();
    fx.put(
        "decisions/2024-05-14-second-phone.md",
        &decision("accepted", "Anything at all.", ""),
    );
    let out = fx.run(
        fx.brain.path(),
        &["--repo", fx.brain.path().to_str().unwrap()],
        &["check"],
    );
    assert!(out.status.success(), "{}", text(&out));
}

#[test]
fn changed_paths_list_the_entries_they_touch() {
    let fx = Fx::new();
    fx.put(
        "notes/signup-flow.md",
        &note(
            "s",
            &sha256(POLICY),
            "project: courier-app\ntouches: [src/main.ts]\n",
        ),
    );
    fx.put(
        "decisions/2024-06-01-elsewhere.md",
        &decision("accepted", "Context.", "").replace("src/signup/**", "src/billing/**"),
    );
    fx.put(
        "decisions/2024-06-02-other-project.md",
        &decision("accepted", "Context.", "").replace("project: courier-app", "project: other"),
    );
    fx.put(
        "decisions/2024-06-03-old.md",
        &decision(
            "superseded",
            "Context.",
            "superseded-by: 2024-06-01-elsewhere\n",
        ),
    );
    fx.commit("more");
    let v = json(&fx.check_in_project(&[
        "--changed",
        "src/signup/form.ts",
        "README.md",
        "--project",
        "courier-app",
        "--json",
    ]));
    assert_eq!(
        paths(&v["matches"]),
        ["decisions/2024-05-14-second-phone.md"]
    );
    assert_eq!(v["matches"][0]["files"][0], "src/signup/form.ts");
    assert_eq!(v["matches"][0]["kind"], "decision");
    let v = json(&fx.check_in_project(&[
        "--changed",
        "src/main.ts",
        "--project",
        "courier-app",
        "--json",
    ]));
    assert_eq!(paths(&v["matches"]), ["notes/signup-flow.md"]);
    assert_eq!(v["matches"][0]["kind"], "note");
    // Human output says what matched and why.
    let out = fx.check_in_project(&[
        "--changed",
        "src/signup/form.ts",
        "--project",
        "courier-app",
    ]);
    let t = text(&out);
    assert!(out.status.success(), "{t}");
    assert!(
        t.contains("decisions/2024-05-14-second-phone.md") && t.contains("src/signup/form.ts"),
        "{t}"
    );
    let out = fx.check_in_project(&["--changed", "docs/x.md", "--project", "courier-app"]);
    assert!(text(&out).contains("no decision or note"), "{}", text(&out));
}

#[test]
fn a_commit_range_lists_everything_every_commit_in_it_changed() {
    let fx = Fx::new();
    let p = fx.project.path();
    let first = git(p, &["rev-parse", "HEAD"]);
    fs::write(p.join("src/signup/form.ts"), "form 2").unwrap();
    git(p, &["commit", "-qam", "outside the skills"]);
    fs::write(p.join("src/main.ts"), "main 2").unwrap();
    git(p, &["commit", "-qam", "another"]);
    let v = json(&fx.check_in_project(&[
        "--changed",
        &format!("{first}..HEAD"),
        "--project",
        "courier-app",
        "--json",
    ]));
    assert_eq!(
        paths(&v["matches"]),
        ["decisions/2024-05-14-second-phone.md"]
    );
    let changed: Vec<&str> = v["changed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap())
        .collect();
    assert_eq!(changed, ["src/main.ts", "src/signup/form.ts"]);
    // A commit that is not there is an error, not an empty answer.
    let out = fx.check_in_project(&["--changed", "nothere..HEAD", "--project", "courier-app"]);
    assert!(!out.status.success());
}

#[test]
fn an_entry_whose_touches_match_no_file_is_flagged_as_dangling() {
    let fx = Fx::new();
    fx.put(
        "decisions/2024-06-01-gone.md",
        &decision("accepted", "Context.", "").replace("src/signup/**", "src/legacy/**"),
    );
    fx.commit("dangling");
    // With --project alone.
    let out = fx.check_in_project(&["--project", "courier-app"]);
    let t = text(&out);
    assert!(out.status.success(), "dangling is a warning: {t}");
    assert!(
        t.contains("warning: decisions/2024-06-01-gone.md: touches match no file in courier-app: src/legacy/**"),
        "{t}"
    );
    assert!(!t.contains("decisions/2024-05-14-second-phone.md"), "{t}");
    // And with --changed.
    let v = json(&fx.check_in_project(&[
        "--changed",
        "src/main.ts",
        "--project",
        "courier-app",
        "--json",
    ]));
    assert_eq!(paths(&v["dangling"]), ["decisions/2024-06-01-gone.md"]);
    // A superseded decision is history: never dangling.
    fx.put(
        "decisions/2024-06-01-gone.md",
        &decision(
            "superseded",
            "Context.",
            "superseded-by: 2024-05-14-second-phone\n",
        )
        .replace("src/signup/**", "src/legacy/**"),
    );
    let v = json(&fx.check_in_project(&["--project", "courier-app", "--json"]));
    assert_eq!(v["dangling"].as_array().unwrap().len(), 0);
}

#[test]
fn the_brain_is_the_update_source_the_project_lists() {
    let fx = Fx::new();
    fx.put(
        "decisions/2024-06-01-bad.md",
        "Just text, no frontmatter.\n",
    );
    fs::write(
        fx.project.path().join("AGENTS.md"),
        format!(
            "# P\n\n## Supercharge\n\n```yaml\nbrain:\n  - path: {}\n    update: true\n```\n",
            fx.brain.path().display()
        ),
    )
    .unwrap();
    let out = fx.run(fx.project.path(), &[], &["check"]);
    assert!(!out.status.success());
    assert!(
        text(&out).contains("decisions/2024-06-01-bad.md"),
        "{}",
        text(&out)
    );
}

#[test]
fn with_no_brain_to_check_it_says_so() {
    let fx = Fx::new();
    let out = fx.run(fx.project.path(), &[], &["check"]);
    assert!(!out.status.success());
    assert!(text(&out).contains("update: true"), "{}", text(&out));
}

#[test]
fn nothing_is_written_by_check() {
    let fx = Fx::new();
    fx.put("notes/bad.md", "no frontmatter\n");
    let before = git(fx.brain.path(), &["status", "--porcelain"]);
    fx.check(&[]);
    fx.check_in_project(&["--changed", "src/main.ts", "--project", "courier-app"]);
    assert_eq!(before, git(fx.brain.path(), &["status", "--porcelain"]));
    assert_eq!(fs::read_dir(fx.cache.path()).unwrap().count(), 0);
}
