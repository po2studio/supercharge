//! The "Ask the brain" flow through the `brain` CLI, with the real model and no network: index a
//! folder, search it, change a file, index again, and never write into the folder.

use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
use tempfile::TempDir;

struct Env {
    brain: TempDir,
    cache: TempDir,
}

impl Env {
    fn new(files: &[(&str, &str)]) -> Self {
        let env = Env {
            brain: TempDir::new().unwrap(),
            cache: TempDir::new().unwrap(),
        };
        for (path, text) in files {
            env.write(path, text);
        }
        env
    }

    fn write(&self, path: &str, text: &str) {
        let p = self.brain.path().join(path);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }

    fn cmd(&self, args: &[&str]) -> Output {
        run(
            env!("CARGO_BIN_EXE_brain"),
            self.brain.path(),
            self.cache.path(),
            args,
        )
    }

    fn json(&self, args: &[&str]) -> Value {
        let out = self.cmd(args);
        assert!(
            out.status.success(),
            "{args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
            panic!(
                "{args:?}: not JSON ({e}): {}",
                String::from_utf8_lossy(&out.stdout)
            )
        })
    }

    fn paths(&self, args: &[&str]) -> Vec<String> {
        self.json(args)
            .as_array()
            .unwrap()
            .iter()
            .map(|h| h["path"].as_str().unwrap().to_string())
            .collect()
    }
}

fn run(bin: &str, root: &Path, cache: &Path, args: &[&str]) -> Output {
    Command::new(bin)
        .env_clear()
        .env("SUPERCHARGE_CACHE_DIR", cache)
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .unwrap()
}

fn small_brain() -> Env {
    Env::new(&[
        ("policies/animals.md", "# Animals\n\nAnimals are not permitted on the premises except certified assistance canines.\n"),
        ("policies/holidays.md", "# Holidays\n\nStaff get 25 days of paid leave per year, taken in agreement with the team lead.\n"),
        ("finance/invoices.md", "# Invoices\n\nInvoice INV-2024-0042 was paid by Hafnarbakki ehf on 3 March. INV-2024-0043 is still open.\n"),
        ("it/laptops.md", "# Laptops\n\nNew laptops are ordered from the approved supplier and replaced every four years.\n"),
        ("icelandic/orlof.md", "# Orlof\n\nStarfsmenn eiga rétt á 25 orlofsdögum á ári. Orlof er tekið í samráði við yfirmann.\n"),
    ])
}

/// Every file and directory under `root` with its size and modification time.
fn snapshot(root: &Path) -> Vec<(PathBuf, u64, std::time::SystemTime)> {
    fn walk(dir: &Path, out: &mut Vec<(PathBuf, u64, std::time::SystemTime)>) {
        for e in fs::read_dir(dir).unwrap() {
            let e = e.unwrap();
            let m = e.metadata().unwrap();
            out.push((e.path(), m.len(), m.modified().unwrap()));
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

#[test]
fn index_search_change_and_index_again() {
    let env = small_brain();
    let first = env.json(&["index", "--json"]);
    assert_eq!(first["embedded_chunks"], first["chunks"]);
    // Exact terms, including an ID that isn't valid full-text syntax as typed.
    assert_eq!(
        env.paths(&["search", "--terms", "INV-2024-0042", "--json"]),
        ["finance/invoices.md"]
    );
    // A question that shares no words with its answer, and an Icelandic one.
    let paths = env.paths(&[
        "search",
        "--about",
        "Can I bring my dog to work?",
        "--json",
        "-k",
        "5",
    ]);
    assert!(
        paths.contains(&"policies/animals.md".to_string()),
        "{paths:?}"
    );
    let paths = env.paths(&[
        "search",
        "--about",
        "Hvað fæ ég marga frídaga á ári?",
        "--json",
        "-k",
        "3",
    ]);
    assert_eq!(paths[0], "icelandic/orlof.md", "{paths:?}");
    // Hits carry the settled fields.
    let hits = env.json(&[
        "search",
        "--terms",
        "laptops",
        "--about",
        "how often are computers replaced",
        "--json",
    ]);
    for key in [
        "id", "path", "title", "heading", "date", "kind", "status", "stale", "score", "snippet",
        "versions",
    ] {
        assert!(hits[0].get(key).is_some(), "hit lacks {key}: {}", hits[0]);
    }
    // A flat text file with no headings or newlines is cut up and found.
    let flat = (0..400)
        .map(|i| format!("filler sentence {i} about nothing. "))
        .collect::<String>()
        + "The door code is zebrafish-7731. ";
    env.write("misc/chat-export.txt", &flat);
    let after = env.json(&["index", "--json"]);
    assert_eq!(
        (
            after["files_changed"].as_u64(),
            after["embedded_chunks"].as_u64() > Some(1)
        ),
        (Some(1), true)
    );
    assert_eq!(
        env.paths(&["search", "--terms", "zebrafish", "--json"]),
        ["misc/chat-export.txt"]
    );
    // Only the changed file is embedded again; `stats` says so.
    env.write(
        "it/laptops.md",
        "# Laptops\n\nNew laptops are replaced every three years now.\n",
    );
    let after = env.json(&["index", "--json"]);
    assert_eq!(
        (
            after["files_changed"].as_u64(),
            after["embedded_chunks"].as_u64()
        ),
        (Some(1), Some(1))
    );
    assert_eq!(
        env.json(&["stats", "--json"])["last_index"]["embedded_chunks"],
        1
    );
    assert_eq!(
        env.paths(&["search", "--terms", "three", "--json"]),
        ["it/laptops.md"]
    );
    // A deleted file leaves the index.
    fs::remove_file(env.brain.path().join("it/laptops.md")).unwrap();
    env.json(&["index", "--json"]);
    assert!(env
        .paths(&["search", "--terms", "three", "--json"])
        .is_empty());
}

#[test]
fn nothing_is_written_into_the_brain() {
    let env = small_brain();
    let before = snapshot(env.brain.path());
    env.json(&["index", "--json"]);
    env.json(&["search", "--terms", "laptops", "--json"]);
    env.json(&["stats", "--json"]);
    assert_eq!(snapshot(env.brain.path()), before);
    // The index lives under the cache directory, keyed by the root.
    let index_dirs: Vec<_> = fs::read_dir(env.cache.path().join("index"))
        .unwrap()
        .collect();
    assert_eq!(index_dirs.len(), 1);
    assert!(index_dirs[0]
        .as_ref()
        .unwrap()
        .path()
        .join("index.sqlite")
        .exists());
}

#[test]
fn files_it_cannot_read_yet_are_counted_not_failed() {
    let env = small_brain();
    env.write("scans/receipt.pdf", "%PDF-1.4 not really");
    env.write(".git/config", "[core]");
    let index = env.json(&["index", "--json"]);
    assert_eq!(index["files"], 5);
    let stats = env.json(&["stats", "--json"]);
    assert_eq!(stats["not_indexed"]["pdf"], 1);
}

#[test]
fn search_before_index_says_so() {
    let env = small_brain();
    let out = env.cmd(&["search", "--terms", "laptops", "--json"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("brain index"));
}

#[test]
fn eval_prints_the_metrics_and_fails_on_a_regression() {
    let corpus = TempDir::new().unwrap();
    let questions = TempDir::new().unwrap();
    let env = small_brain();
    for (path, _) in [
        ("policies/animals.md", 0),
        ("policies/holidays.md", 0),
        ("finance/invoices.md", 0),
        ("it/laptops.md", 0),
        ("icelandic/orlof.md", 0),
    ] {
        let dest = corpus.path().join(path);
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        fs::copy(env.brain.path().join(path), dest).unwrap();
    }
    fs::write(
        questions.path().join("q.yaml"),
        r#"
- id: dog
  question: Can I bring my dog to work?
  terms: dog OR dogs
  expect: [policies/animals.md]
- id: laptop-age
  question: How often do computers get swapped out?
  terms: computers OR swapped
  expect: [it/laptops.md]
- id: invoice
  question: Who paid INV-2024-0042?
  terms: '"INV-2024-0042"'
  expect: [finance/invoices.md]
- id: leave
  question: How many vacation days do employees get?
  terms: vacation OR employees
  expect: [policies/holidays.md]
"#,
    )
    .unwrap();
    let baseline = env.cache.path().join("baseline.json");
    let eval = |extra: &[&str]| {
        let mut args = vec![
            "eval",
            "--corpus",
            corpus.path().to_str().unwrap(),
            "--questions",
            questions.path().to_str().unwrap(),
            "--baseline",
            baseline.to_str().unwrap(),
        ];
        args.extend_from_slice(extra);
        env.cmd(&args)
    };
    let out = eval(&["--update-baseline"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    for needle in ["recall@5", "MRR", "p50", "chunks/s", "hybrid", "terms"] {
        assert!(text.contains(needle), "eval output lacks {needle}:\n{text}");
    }
    let json: Value = serde_json::from_str(&fs::read_to_string(&baseline).unwrap()).unwrap();
    assert!(
        json["hybrid"]["recall_at_5"].as_f64().unwrap()
            > json["terms"]["recall_at_5"].as_f64().unwrap()
    );
    // Against its own baseline it passes; against a better one it fails.
    assert!(eval(&[]).status.success());
    let mut better = json.clone();
    better["hybrid"]["recall_at_5"] = Value::from(1.5);
    fs::write(&baseline, better.to_string()).unwrap();
    let out = eval(&[]);
    assert!(
        !out.status.success(),
        "a regression against the baseline must fail"
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("regress"));
}
