//! Slice 1 acceptance tests, through the binary, with the real model and no network.
//! Criteria numbers are the slice's (docs/features/0002-second-brain.md).

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
fn ac3_flat_text_with_no_headings_or_newlines_is_indexed_and_found() {
    let mut flat = String::new();
    for i in 0..300 {
        flat.push_str(&format!(
            "filler sentence number {i} about nothing in particular. "
        ));
    }
    flat.push_str("The warehouse door code is zebrafish-7731. ");
    for i in 0..300 {
        flat.push_str(&format!("more filler {i} that goes on and on. "));
    }
    assert!(!flat.contains('\n'));
    let env = Env::new(&[
        ("misc/chat-export.txt", &flat),
        ("misc/other.txt", "Nothing to see here."),
    ]);
    env.json(&["index", "--json"]);
    let stats = env.json(&["stats", "--json"]);
    assert!(
        stats["chunks"].as_u64().unwrap() > 2,
        "a long flat file is cut into several chunks: {stats}"
    );
    assert_eq!(
        env.paths(&["search", "--terms", "zebrafish", "--json"]),
        ["misc/chat-export.txt"]
    );
    let about = env.paths(&[
        "search",
        "--about",
        "what is the code for the warehouse door",
        "--json",
        "-k",
        "5",
    ]);
    assert_eq!(about[0], "misc/chat-export.txt");
}

#[test]
fn ac4_a_question_with_no_shared_words_finds_its_answer_in_the_top_5() {
    let env = small_brain();
    env.json(&["index", "--json"]);
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
}

#[test]
fn ac4_an_exact_id_is_found_with_terms() {
    let env = small_brain();
    env.json(&["index", "--json"]);
    // A hyphenated ID is not valid full-text syntax as typed; it must still work.
    let paths = env.paths(&["search", "--terms", "INV-2024-0042", "--json"]);
    assert_eq!(paths, ["finance/invoices.md"]);
}

#[test]
fn ac4_an_icelandic_question_finds_its_icelandic_answer() {
    let env = small_brain();
    env.json(&["index", "--json"]);
    let paths = env.paths(&[
        "search",
        "--about",
        "Hvað fæ ég marga frídaga á ári?",
        "--json",
        "-k",
        "3",
    ]);
    assert_eq!(paths[0], "icelandic/orlof.md", "{paths:?}");
}

#[test]
fn ac4_hits_have_the_settled_shape() {
    let env = small_brain();
    env.json(&["index", "--json"]);
    let hits = env.json(&[
        "search",
        "--terms",
        "laptops",
        "--about",
        "how often are computers replaced",
        "--json",
    ]);
    let hit = &hits[0];
    for key in [
        "id", "path", "title", "heading", "date", "kind", "status", "stale", "score", "snippet",
        "versions",
    ] {
        assert!(hit.get(key).is_some(), "hit lacks {key}: {hit}");
    }
    assert_eq!(hit["path"], "it/laptops.md");
    assert_eq!(hit["title"], "Laptops");
    assert!(hit["snippet"].as_str().unwrap().contains("four years"));
    assert!(hit["date"].as_str().unwrap().len() == 10);
}

#[test]
fn ac5_after_one_file_changes_only_that_files_chunks_are_embedded_again() {
    let env = small_brain();
    let first = env.json(&["index", "--json"]);
    assert_eq!(first["embedded_chunks"], first["chunks"]);
    let again = env.json(&["index", "--json"]);
    assert_eq!(again["embedded_chunks"], 0, "nothing changed: {again}");
    env.write(
        "it/laptops.md",
        "# Laptops\n\nNew laptops are replaced every three years now.\n",
    );
    let after = env.json(&["index", "--json"]);
    assert_eq!(after["files_changed"], 1);
    assert_eq!(after["embedded_chunks"], 1);
    let stats = env.json(&["stats", "--json"]);
    assert_eq!(stats["last_index"]["embedded_chunks"], 1);
    assert_eq!(stats["files"], 5);
    let paths = env.paths(&["search", "--terms", "three", "--json"]);
    assert_eq!(paths, ["it/laptops.md"]);
    // A deleted file leaves the index.
    fs::remove_file(env.brain.path().join("it/laptops.md")).unwrap();
    env.json(&["index", "--json"]);
    assert!(env
        .paths(&["search", "--terms", "three", "--json"])
        .is_empty());
}

#[test]
fn ac6_eval_prints_the_metrics_and_hybrid_beats_terms_only() {
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

#[test]
fn ac7_a_lone_binary_indexes_and_searches_with_no_environment() {
    let dir = TempDir::new().unwrap();
    let lone = dir.path().join("brain");
    fs::copy(env!("CARGO_BIN_EXE_brain"), &lone).unwrap();
    let env = small_brain();
    let out = run(
        lone.to_str().unwrap(),
        env.brain.path(),
        env.cache.path(),
        &["index", "--json"],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = run(
        lone.to_str().unwrap(),
        env.brain.path(),
        env.cache.path(),
        &["search", "--terms", "laptops", "--json"],
    );
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("it/laptops.md"));
}

#[test]
fn ac8_nothing_is_written_into_the_brain() {
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
