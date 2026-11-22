//! Slice 8 acceptance tests (feature 0002: the brain is braindead to use). Deleted once green;
//! the ones worth keeping live on in `e2e.rs`.

use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;

fn run(cwd: &Path, cache: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_brain"))
        .current_dir(cwd)
        .env_clear()
        .env("SUPERCHARGE_CACHE_DIR", cache)
        .args(args)
        .output()
        .unwrap()
}

fn json(cwd: &Path, cache: &Path, args: &[&str]) -> Value {
    let out = run(cwd, cache, args);
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

fn paths(hits: &Value) -> Vec<String> {
    hits.as_array()
        .unwrap()
        .iter()
        .map(|h| h["path"].as_str().unwrap().to_string())
        .collect()
}

fn write(root: &Path, path: &str, text: &str) {
    let full = root.join(path);
    fs::create_dir_all(full.parent().unwrap()).unwrap();
    fs::write(full, text).unwrap();
}

/// A folder of documents read as the one source of a directory that is no project.
fn small_brain() -> (TempDir, TempDir, TempDir) {
    let (brain, cache, cwd) = (
        TempDir::new().unwrap(),
        TempDir::new().unwrap(),
        TempDir::new().unwrap(),
    );
    for (p, t) in [
        ("policies/animals.md", "# Animals\n\nAnimals are not permitted on the premises except certified assistance canines.\n"),
        ("policies/holidays.md", "# Holidays\n\nStaff get 25 days of paid leave per year, taken in agreement with the team lead.\n"),
        ("finance/invoices.md", "# Invoices\n\nInvoice INV-2024-0042 was paid by Hafnarbakki ehf on 3 March. INV-2024-0043 is still open.\n"),
        ("it/laptops.md", "# Laptops\n\nNew laptops are ordered from the approved supplier and replaced every four years.\n"),
        ("icelandic/orlof.md", "# Orlof\n\nStarfsmenn eiga rétt á 25 orlofsdögum á ári. Orlof er tekið í samráði við yfirmann.\n"),
    ] {
        write(brain.path(), p, t);
    }
    (brain, cache, cwd)
}

/// AC 1: `brain search "<question>"` with no flags and no `index` first.
#[test]
fn ac1_plain_questions_find_exact_terms_paraphrases_and_icelandic() {
    let (brain, cache, cwd) = small_brain();
    let ask = |q: &str| {
        json(
            cwd.path(),
            cache.path(),
            &[
                "--source",
                brain.path().to_str().unwrap(),
                "search",
                q,
                "--json",
            ],
        )
    };
    assert_eq!(
        paths(&ask("Who paid invoice INV-2024-0042?"))[0],
        "finance/invoices.md"
    );
    assert!(paths(&ask("Can I bring my dog to work?")).contains(&"policies/animals.md".to_string()));
    assert_eq!(
        paths(&ask("Hvað fæ ég marga frídaga á ári?"))[0],
        "icelandic/orlof.md"
    );
}

/// AC 1: on the eval, the fused search finds at least as many answers as either half alone.
#[test]
fn ac1_the_fused_search_is_at_least_as_good_as_either_half_on_the_eval() {
    let cache = TempDir::new().unwrap();
    let out = run(
        Path::new(env!("CARGO_MANIFEST_DIR")),
        cache.path(),
        &["eval", "--json", "--baseline", "/nonexistent"],
    );
    let r: Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "eval printed no JSON ({e}): {}",
            String::from_utf8_lossy(&out.stderr)
        )
    });
    let recall = |m: &str| r[m]["recall_at_5"].as_f64().unwrap();
    assert!(recall("hybrid") >= recall("about"), "{r}");
    assert!(recall("hybrid") >= recall("terms"), "{r}");
}

/// AC 2: a file that changes is found by the next search, with no `index` between.
#[test]
fn ac2_a_changed_file_is_found_without_index() {
    let (brain, cache, cwd) = small_brain();
    let ask = |q: &str| {
        paths(&json(
            cwd.path(),
            cache.path(),
            &[
                "--source",
                brain.path().to_str().unwrap(),
                "search",
                q,
                "--json",
            ],
        ))
    };
    assert!(!ask("zebrafish door code").contains(&"it/laptops.md".to_string()));
    write(
        brain.path(),
        "it/laptops.md",
        "# Laptops\n\nThe door code is zebrafish-7731.\n",
    );
    assert_eq!(ask("zebrafish door code")[0], "it/laptops.md");
    write(
        brain.path(),
        "misc/new.md",
        "# New\n\nThe walrus meeting is on Thursday.\n",
    );
    assert_eq!(ask("walrus meeting")[0], "misc/new.md");
    fs::remove_file(brain.path().join("misc/new.md")).unwrap();
    assert!(!ask("walrus meeting").contains(&"misc/new.md".to_string()));
}

/// AC 3: a superseded ADR comes after the current one that replaced it, marked; an amended one
/// comes with its amendments.
#[test]
fn ac3_a_superseded_adr_ranks_after_its_replacement_and_an_amended_one_brings_its_amendments() {
    let (dir, cache) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let repo = dir.path().join("repo");
    write(&repo, "AGENTS.md", "# AGENTS.md\n\nA service.\n");
    write(&repo, "docs/adr/0001-weekly.md", "# 0001: Invoices go out weekly\n\nStatus: superseded by [0003](0003-daily.md)\n\nInvoices are batched weekly.\n");
    write(&repo, "docs/adr/0002-currency.md", "# 0002: Currencies\n\nStatus: accepted; amended by [0004](0004-nok.md)\n\nAmounts are stored in ISK and EUR.\n");
    write(
        &repo,
        "docs/adr/0003-daily.md",
        "# 0003: Invoices go out daily\n\nStatus: accepted\n\nInvoices are batched daily.\n",
    );
    write(
        &repo,
        "docs/adr/0004-nok.md",
        "# 0004: NOK too\n\nStatus: accepted\n\nNorwegian krone amounts are stored as well.\n",
    );
    let hits = json(
        &repo,
        cache.path(),
        &["search", "How often are invoices batched?", "--json"],
    );
    let found = paths(&hits);
    let at = |p: &str| {
        found
            .iter()
            .position(|f| f == p)
            .unwrap_or_else(|| panic!("{p} not found: {found:?}"))
    };
    assert!(
        at("docs/adr/0003-daily.md") < at("docs/adr/0001-weekly.md"),
        "{found:?}"
    );
    let old = &hits[at("docs/adr/0001-weekly.md")];
    assert_eq!(old["status"], "superseded");
    assert_eq!(old["superseded_by"], "docs/adr/0003-daily.md");
    let text = String::from_utf8(
        run(
            &repo,
            cache.path(),
            &["search", "How often are invoices batched?"],
        )
        .stdout,
    )
    .unwrap();
    assert!(
        text.contains("superseded by docs/adr/0003-daily.md"),
        "{text}"
    );
    let amended = json(
        &repo,
        cache.path(),
        &[
            "search",
            "Amounts are stored in ISK and EUR",
            "-k",
            "1",
            "--json",
        ],
    );
    assert_eq!(
        paths(&amended),
        ["docs/adr/0002-currency.md", "docs/adr/0004-nok.md"]
    );
}

/// AC 4: `--help` lists no removed command or flag.
#[test]
fn ac4_help_lists_no_removed_command_or_flag() {
    let cache = TempDir::new().unwrap();
    let cwd = TempDir::new().unwrap();
    let help =
        |args: &[&str]| String::from_utf8(run(cwd.path(), cache.path(), args).stdout).unwrap();
    let top = help(&["--help"]);
    let commands: Vec<&str> = top
        .lines()
        .skip_while(|l| !l.starts_with("Commands:"))
        .skip(1)
        .take_while(|l| l.starts_with("  "))
        .filter_map(|l| l.split_whitespace().next())
        .collect();
    assert_eq!(
        commands,
        ["search", "index", "stats", "eval", "pack", "bench", "help"],
        "{top}"
    );
    let search = help(&["search", "--help"]);
    for gone in [
        "--terms",
        "--about",
        "--kind",
        "--project",
        "--since",
        "--status",
        "--all",
    ] {
        assert!(
            !search.contains(gone),
            "search --help still lists {gone}:\n{search}"
        );
    }
    for gone in ["BRAIN.md", "brain check", "decisions/", "notes/", "touches"] {
        assert!(!top.contains(gone), "--help still mentions {gone}:\n{top}");
    }
}
