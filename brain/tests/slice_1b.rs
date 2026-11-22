//! Acceptance tests of slice 1b (feature 0002): the brain works on the project with no setup, and
//! outside sources opt in. Through the `brain` CLI, as a person runs it: from inside a project,
//! with no flags. Deleted once they pass; `e2e.rs` keeps the happy path.

use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Output,
};
use tempfile::TempDir;

/// A machine: a home, a cache, and projects and sources beside them.
struct World {
    dir: TempDir,
}

fn bin() -> String {
    std::env::var("BRAIN_BIN").unwrap_or_else(|_| env!("CARGO_BIN_EXE_brain").to_string())
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

impl World {
    fn new() -> World {
        let w = World {
            dir: TempDir::new().unwrap(),
        };
        fs::create_dir_all(w.home()).unwrap();
        w
    }

    fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }

    fn cache(&self) -> PathBuf {
        self.dir.path().join("cache")
    }

    fn at(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    /// A project with the files given, and an AGENTS.md whose config block holds `config`.
    fn project(&self, name: &str, config: &str, files: &[(&str, &str)]) -> PathBuf {
        let root = self.at(name);
        write(
            &root.join("AGENTS.md"),
            &format!("# AGENTS.md\n\nThe project.\n\n## Supercharge\n\n```yaml\n{config}\n```\n"),
        );
        for (p, t) in files {
            write(&root.join(p), t);
        }
        root
    }

    fn source(&self, rel: &str, files: &[(&str, &str)]) -> PathBuf {
        let root = self.at(rel);
        fs::create_dir_all(&root).unwrap();
        for (p, t) in files {
            write(&root.join(p), t);
        }
        root
    }

    /// Runs `brain` in `cwd` with an empty environment but this machine's home and cache.
    fn run(&self, cwd: &Path, args: &[&str]) -> Output {
        std::process::Command::new(bin())
            .current_dir(cwd)
            .env_clear()
            .env("HOME", self.home())
            .env("SUPERCHARGE_CACHE_DIR", self.cache())
            .args(args)
            .output()
            .unwrap()
    }

    fn ok(&self, cwd: &Path, args: &[&str]) -> String {
        let out = self.run(cwd, args);
        assert!(
            out.status.success(),
            "{args:?} in {} failed: {}",
            cwd.display(),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn hits(&self, cwd: &Path, args: &[&str]) -> Vec<Value> {
        let mut full = vec!["search", "--json"];
        full.extend_from_slice(args);
        serde_json::from_str::<Value>(&self.ok(cwd, &full))
            .unwrap()
            .as_array()
            .unwrap()
            .clone()
    }

    fn found(&self, cwd: &Path, args: &[&str]) -> Vec<(String, String)> {
        self.hits(cwd, args)
            .iter()
            .map(|h| {
                (
                    h["source"].as_str().unwrap().to_string(),
                    h["path"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }

    fn index_dirs(&self) -> usize {
        fs::read_dir(self.cache().join("index")).unwrap().count()
    }
}

/// Every path under `root` with its size, so a change anywhere shows.
fn snapshot(root: &Path) -> Vec<(PathBuf, u64)> {
    fn walk(dir: &Path, out: &mut Vec<(PathBuf, u64)>) {
        for e in fs::read_dir(dir).unwrap() {
            let e = e.unwrap();
            let m = e.metadata().unwrap();
            out.push((e.path(), m.len()));
            if m.is_dir() {
                walk(&e.path(), out);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out.sort();
    out
}

const PLAIN: &str = "risk: medium\ndocs: docs";

fn animals() -> (&'static str, &'static str) {
    (
        "docs/policies/animals.md",
        "# Animals\n\nAnimals are not permitted on the premises except certified assistance canines.\n",
    )
}

/// AC 1 (the binary's part): in a project with nothing configured, `brain index` and `brain search`
/// with no flags read the project's docs folder, README.md and AGENTS.md, and write only the cache.
#[test]
fn ac1_a_project_with_nothing_configured_is_indexed_and_searched_with_no_flags() {
    let w = World::new();
    let p = w.project(
        "repo",
        PLAIN,
        &[
            (
                "README.md",
                "# Repo\n\nA service that chases late invoices.\n",
            ),
            animals(),
            (
                "docs/holidays.md",
                "# Holidays\n\nStaff get 25 days of paid leave per year.\n",
            ),
            ("src/main.rs", "fn main() {}"),
            (
                "notes.txt",
                "A file at the root that is not README.md or AGENTS.md.",
            ),
        ],
    );
    let before = (snapshot(&p), snapshot(&w.home()));
    let out = w.ok(&p, &["index", "--json"]);
    let index: Value = serde_json::from_str(&out).unwrap();
    // README.md, AGENTS.md and the two docs; not notes.txt, not src/.
    assert_eq!(index["files"], 4, "{out}");
    let hits = w.hits(&p, &["--about", "Can I bring my dog to work?"]);
    assert_eq!(hits[0]["path"], "docs/policies/animals.md");
    assert_eq!(hits[0]["source"], "project");
    assert!(hits[0]["file"]
        .as_str()
        .unwrap()
        .ends_with("repo/docs/policies/animals.md"));
    assert_eq!(
        w.found(&p, &["--terms", "invoices"]),
        [("project".to_string(), "README.md".to_string())]
    );
    assert_eq!(w.found(&p, &["--terms", "notes"]), []);
    // Nothing written into the project, or anywhere but the cache.
    assert_eq!((snapshot(&p), snapshot(&w.home())), before);
    assert_eq!(w.index_dirs(), 1);
}

/// AC 2: a `docs` key that points elsewhere indexes that folder.
#[test]
fn ac2_the_docs_key_moves_the_docs_folder() {
    let w = World::new();
    let p = w.project(
        "repo",
        "docs: handbook",
        &[
            animals(),
            (
                "handbook/leave.md",
                "# Leave\n\nStaff get 25 days of paid leave per year.\n",
            ),
        ],
    );
    w.ok(&p, &["index"]);
    let found = w.found(&p, &["--terms", "leave OR animals", "-k", "10"]);
    assert!(
        found.contains(&("project".into(), "handbook/leave.md".into())),
        "{found:?}"
    );
    assert!(
        !found.iter().any(|(_, path)| path.starts_with("docs/")),
        "the default docs folder is not read: {found:?}"
    );
}

/// AC 3: a listed source without `update` answers beside the project, and is never written.
#[test]
fn ac3_a_listed_source_is_cited_next_to_the_project_and_not_written() {
    let w = World::new();
    let shared = w.source(
        "shared-policies",
        &[(
            "travel.md",
            "# Travel\n\nHotels are capped at 22,000 krona per night.\n",
        )],
    );
    w.source(
        "home/brains/client-a",
        &[(
            "tone.md",
            "# Tone\n\nClient A wants a formal tone in every email.\n",
        )],
    );
    let p = w.project(
        "repo",
        "brain:\n  - path: ../shared-policies\n  - path: ~/brains/client-a",
        &[(
            "docs/PRODUCT.md",
            "# Product\n\nThe reminder service sends a hotel booking reminder.\n",
        )],
    );
    let before = (snapshot(&shared), snapshot(&w.at("home/brains/client-a")));
    w.ok(&p, &["index"]);
    let found = w.found(&p, &["--terms", "hotel OR hotels OR formal", "-k", "10"]);
    let sources: Vec<&str> = found.iter().map(|(s, _)| s.as_str()).collect();
    assert!(sources.contains(&"project"), "{found:?}");
    assert!(sources.contains(&"../shared-policies"), "{found:?}");
    assert!(sources.contains(&"~/brains/client-a"), "{found:?}");
    assert_eq!(
        (snapshot(&shared), snapshot(&w.at("home/brains/client-a"))),
        before,
        "nothing is written into a source"
    );
}

/// AC 4: `update: true` makes the layout creatable, by `brain init` only; two of them stop.
#[test]
fn ac4_init_creates_the_layout_in_the_update_source_and_two_update_sources_stop() {
    let w = World::new();
    let readonly = w.source("outside", &[("raw.md", "# Raw\n\nA raw file.\n")]);
    let brain = w.source("brain", &[("policy.md", "# Policy\n\nA raw policy.\n")]);
    let p = w.project(
        "repo",
        "brain:\n  - path: ../outside\n  - path: ../brain\n    update: true",
        &[animals()],
    );
    // Reading never creates anything, with or without the update source.
    let before = (snapshot(&readonly), snapshot(&brain));
    w.ok(&p, &["index"]);
    w.ok(&p, &["search", "--terms", "policy"]);
    assert_eq!((snapshot(&readonly), snapshot(&brain)), before);
    // `init` creates the four, and nothing else, and only in the update source.
    w.ok(&p, &["init"]);
    let mut names: Vec<String> = fs::read_dir(&brain)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        ["BRAIN.md", "decisions", "notes", "policy.md", "projects"]
    );
    assert_eq!(snapshot(&readonly), before.0);
    assert_eq!(
        fs::read_to_string(brain.join("policy.md")).unwrap(),
        "# Policy\n\nA raw policy.\n"
    );
    // Again: nothing changes.
    let after = snapshot(&brain);
    w.ok(&p, &["init"]);
    assert_eq!(snapshot(&brain), after);

    // Two update sources stop every command with a message naming both.
    let two = w.project(
        "repo2",
        "brain:\n  - path: ../outside\n    update: true\n  - path: ../brain\n    update: true",
        &[animals()],
    );
    for args in [
        &["index"][..],
        &["search", "--terms", "x"],
        &["init"],
        &["stats"],
    ] {
        let out = w.run(&two, args);
        assert!(!out.status.success(), "{args:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("../outside") && err.contains("../brain"),
            "{args:?}: {err}"
        );
    }
}

/// AC 5: sources stay apart per project, and a source two projects list is indexed once.
#[test]
fn ac5_projects_never_see_each_others_sources_and_share_an_index_for_a_shared_one() {
    let w = World::new();
    w.source(
        "only-a",
        &[("a.md", "# A\n\nThe zeppelin schedule for client A.\n")],
    );
    w.source(
        "only-b",
        &[("b.md", "# B\n\nThe zeppelin schedule for client B.\n")],
    );
    w.source(
        "shared",
        &[("s.md", "# S\n\nThe shared zeppelin handbook.\n")],
    );
    let a = w.project(
        "proj-a",
        "brain:\n  - path: ../only-a\n  - path: ../shared",
        &[animals()],
    );
    let b = w.project(
        "proj-b",
        "brain:\n  - path: ../only-b\n  - path: ../shared",
        &[animals()],
    );
    w.ok(&a, &["index"]);
    w.ok(&b, &["index"]);
    let in_a = w.found(&a, &["--terms", "zeppelin", "-k", "10"]);
    let in_b = w.found(&b, &["--terms", "zeppelin", "-k", "10"]);
    assert!(
        in_a.iter().any(|(_, p)| p == "a.md") && in_a.iter().any(|(_, p)| p == "s.md"),
        "{in_a:?}"
    );
    assert!(
        in_b.iter().any(|(_, p)| p == "b.md") && in_b.iter().any(|(_, p)| p == "s.md"),
        "{in_b:?}"
    );
    assert!(!in_a.iter().any(|(_, p)| p == "b.md"), "{in_a:?}");
    assert!(!in_b.iter().any(|(_, p)| p == "a.md"), "{in_b:?}");
    // Two projects, three sources of their own (their docs, `only-a` or `only-b`), and one shared:
    // 2 project indexes + only-a + only-b + shared.
    assert_eq!(w.index_dirs(), 5);
    // Index the shared source from the second project once more: nothing is embedded again.
    let again: Value = serde_json::from_str(&w.ok(&b, &["index", "--json"])).unwrap();
    assert_eq!(again["embedded_chunks"], 0);
}

/// AC 6: a flat folder, no subfolders and no headings, works as a source.
#[test]
fn ac6_a_flat_folder_with_no_headings_is_a_source() {
    let w = World::new();
    w.source(
        "dropbox-policies",
        &[
            (
                "vacation.txt",
                "Staff may take twenty five days of leave a year, agreed with the team lead.",
            ),
            (
                "parking.txt",
                "The car park is closed on Sundays for cleaning.",
            ),
        ],
    );
    let p = w.project(
        "repo",
        "brain:\n  - path: ../dropbox-policies",
        &[animals()],
    );
    w.ok(&p, &["index"]);
    let found = w.found(&p, &["--terms", "parking"]);
    assert_eq!(
        found,
        [("../dropbox-policies".to_string(), "parking.txt".to_string())]
    );
    let about = w.found(&p, &["--about", "how many vacation days do employees get"]);
    assert!(
        about.contains(&("../dropbox-policies".into(), "vacation.txt".into())),
        "{about:?}"
    );
}

/// The extra requirement: from anywhere inside the project, with no flags, `brain index` and
/// `brain search` resolve the project's sources; flags may override.
#[test]
fn direct_use_from_a_subfolder_and_with_flags_that_override() {
    let w = World::new();
    w.source(
        "extra",
        &[("extra.md", "# Extra\n\nThe quokka rota is on Tuesdays.\n")],
    );
    w.source(
        "listed",
        &[("listed.md", "# Listed\n\nThe quokka budget is small.\n")],
    );
    let p = w.project("repo", "brain:\n  - path: ../listed", &[animals()]);
    fs::create_dir(p.join(".git")).unwrap();
    let deep = p.join("docs/policies");
    // A subfolder of a project finds the project.
    w.ok(&deep, &["index"]);
    let found = w.found(&deep, &["--terms", "quokka OR animals", "-k", "10"]);
    assert!(
        found.contains(&("../listed".into(), "listed.md".into())),
        "{found:?}"
    );
    assert!(
        found.contains(&("project".into(), "docs/policies/animals.md".into())),
        "{found:?}"
    );
    // `--source` replaces the `brain:` list.
    let extra = w.at("extra");
    w.ok(&deep, &["index", "--source", extra.to_str().unwrap()]);
    let found = w.found(
        &deep,
        &["--terms", "quokka", "--source", extra.to_str().unwrap()],
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].1 == "extra.md", "{found:?}");
    // `--repo` names the project from anywhere.
    let elsewhere = w.source("elsewhere", &[]);
    let found = w.found(
        &elsewhere,
        &["--terms", "animals", "--repo", p.to_str().unwrap()],
    );
    assert_eq!(found[0].1, "docs/policies/animals.md");
    // Outside any project, with no source, it says what to do.
    let out = w.run(&elsewhere, &["index"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no project"));
}

/// A listed source this machine lacks is warned about, and the rest still works.
#[test]
fn a_missing_listed_source_warns_and_the_project_still_works() {
    let w = World::new();
    let p = w.project("repo", "brain:\n  - path: ~/not/here", &[animals()]);
    let out = w.run(&p, &["index"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("~/not/here"));
    assert!(!w.found(&p, &["--terms", "animals"]).is_empty());
}
