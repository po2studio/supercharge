//! Slice 2, "Any file goes in", through the `brain` CLI with the real model: what the eval corpus
//! holds in each format is found, what cannot be converted is reported, three versions of a
//! document return one hit, and an index run over unchanged files converts nothing.

use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
use tempfile::TempDir;

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("eval/corpus")
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dest = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &dest);
        } else {
            fs::copy(e.path(), dest).unwrap();
        }
    }
}

/// A machine whose one source is a copy of some folders of the eval corpus.
struct Env {
    source: TempDir,
    cache: TempDir,
    cwd: TempDir,
}

impl Env {
    fn new(folders: &[&str]) -> Env {
        let env = Env {
            source: TempDir::new().unwrap(),
            cache: TempDir::new().unwrap(),
            cwd: TempDir::new().unwrap(),
        };
        for f in folders {
            copy_dir(&corpus().join(f), &env.source.path().join(f));
        }
        env
    }

    fn cmd(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_brain"))
            .current_dir(self.cwd.path())
            .env_clear()
            .env("SUPERCHARGE_CACHE_DIR", self.cache.path())
            .arg("--source")
            .arg(self.source.path())
            .args(args)
            .output()
            .unwrap()
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

    fn hits(&self, args: &[&str]) -> Vec<Value> {
        let mut full = vec!["search", "--json"];
        full.extend_from_slice(args);
        self.json(&full).as_array().unwrap().clone()
    }
}

fn paths(hits: &[Value]) -> Vec<&str> {
    hits.iter().map(|h| h["path"].as_str().unwrap()).collect()
}

/// AC 1: for each format, a question whose answer exists only in a file of that format finds it.
#[test]
fn every_format_is_found_by_what_only_it_says() {
    let env = Env::new(&["formats"]);
    env.json(&["index", "--json"]);
    for (term, file) in [
        ("Brimvörn", "vendor-security-review.html"),
        ("switchover", "regional-switchover-drill.pdf"),
        ("\"test booking\"", "operator-onboarding-checklist.docx"),
        ("Snæfellsnes", "regional-sales-2024.xlsx"),
        ("\"Mission Control\"", "all-hands-2025-q1.pptx"),
        ("Westfjords", "all-hands-2025-q1.pptx"),
        ("\"two people in finance\"", "refund-exceptions.odt"),
        ("\"Lindi Hosting\"", "vendor-contracts.ods"),
        ("\"voucher wallet\"", "product-roadmap-2025.odp"),
        ("Selkie", "supplier-memo.rtf"),
        ("gluten", "offsite-catering.eml"),
        ("\"3.14.2\"", "support-escalations.mbox"),
        ("\"EXP-9931\"", "expense-claims-2024-q4.csv"),
        ("Brjánslækur", "ferry-timetable.tsv"),
        ("instant_quote_widget", "feature-flags.json"),
        ("chat_transcripts", "data-retention.yaml"),
        ("LATE_CHECKIN_FEE_ISK", "late_checkin_fee.py"),
        ("shuttle", "2025-03-10-operator-call.txt"),
    ] {
        let hits = env.hits(&["--terms", term]);
        assert_eq!(
            paths(&hits).first().copied(),
            Some(format!("formats/{file}").as_str()),
            "--terms {term}: {:?}",
            paths(&hits)
        );
    }
    // A question that shares no words with its answer, in a format that is not text.
    let hits = env.hits(&[
        "--about",
        "how long did it take to move traffic to the backup site in the exercise",
        "-k",
        "3",
    ]);
    assert!(
        paths(&hits).contains(&"formats/regional-switchover-drill.pdf"),
        "{:?}",
        paths(&hits)
    );
    // An email's quoted reply is not indexed; its sender and date are metadata of the message.
    assert!(env.hits(&["--terms", "caterer"]).is_empty());
    let mail = env.hits(&["--terms", "Gamla"]);
    assert!(mail[0]["snippet"].as_str().unwrap().contains("From: Anna"));
    assert_eq!(mail[0]["date"], "2025-03-11");
    // A CSV is cut into row groups that each repeat the header.
    let csv = env.hits(&["--terms", "\"EXP-9931\""]);
    assert!(
        csv[0]["snippet"].as_str().unwrap().contains("amount_isk"),
        "{}",
        csv[0]["snippet"]
    );
}

/// AC 2 and 3: what cannot be converted is listed by `stats`, and indexing completes.
#[test]
fn scans_images_encrypted_and_corrupt_files_are_listed_not_errors() {
    let env = Env::new(&["formats"]);
    let index = env.json(&["index", "--json"]);
    let listed: Vec<(&str, &str)> = index["unconverted"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| {
            (
                u["path"].as_str().unwrap().trim_start_matches("formats/"),
                u["reason"].as_str().unwrap(),
            )
        })
        .collect();
    let reason = |file: &str| {
        listed
            .iter()
            .find(|(p, _)| p.ends_with(file))
            .unwrap_or_else(|| panic!("{file} is not listed: {listed:?}"))
            .1
    };
    assert!(reason("scanned-contract.pdf").contains("no text layer"));
    assert!(reason("office-photo.png").contains("image"));
    assert!(reason("encrypted-payroll.pdf").contains("encrypted"));
    assert!(reason("encrypted-offer-letter.docx").contains("encrypted"));
    assert!(reason("corrupt-report.docx").contains("not a readable"));
    assert_eq!(listed.len(), 5, "{listed:?}");
    // The rest is indexed, and `stats` says what was not.
    assert_eq!(index["files"], 17, "{index}");
    let out = env.cmd(&["stats"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("not converted (5)"), "{text}");
    for file in [
        "scanned-contract.pdf",
        "office-photo.png",
        "corrupt-report.docx",
    ] {
        assert!(text.contains(file), "{file} missing from:\n{text}");
    }
    let stats = env.json(&["stats", "--json"]);
    assert_eq!(
        stats["sources"][0]["last_index"]["unconverted"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
}

/// AC 4: three versions of a document return one hit, the newest, with the others in `versions`.
#[test]
fn versions_of_a_document_return_the_newest_with_the_others_listed() {
    let env = Env::new(&["duplicates"]);
    let index = env.json(&["index", "--json"]);
    assert_eq!(index["duplicates"], 3, "{index}");
    for args in [
        vec!["--terms", "notice"],
        vec![
            "--about",
            "how long is the notice for ending the partner agreement",
        ],
        vec![
            "--terms",
            "notice",
            "--about",
            "how long is the notice for ending the partner agreement",
        ],
    ] {
        let hits = env.hits(&args);
        let agreements: Vec<&Value> = hits
            .iter()
            .filter(|h| h["path"].as_str().unwrap().contains("partner-agreement"))
            .collect();
        assert_eq!(agreements.len(), 1, "{args:?}: {:?}", paths(&hits));
        assert_eq!(
            agreements[0]["path"],
            "duplicates/partner-agreement_final_final.docx"
        );
        assert_eq!(
            agreements[0]["versions"],
            serde_json::json!([
                "duplicates/partner-agreement_v2.docx",
                "duplicates/partner-agreement_v1.docx"
            ])
        );
    }
    // Exact copies too: one hit, the copy listed.
    let hits = env.hits(&["--terms", "insurance"]);
    assert_eq!(paths(&hits), ["duplicates/travel-insurance-faq.md"]);
    assert_eq!(
        hits[0]["versions"],
        serde_json::json!(["duplicates/archive/travel-insurance-faq.md"])
    );
}

/// AC 5: running the index again over unchanged files converts nothing.
#[test]
fn indexing_unchanged_files_again_converts_nothing() {
    let env = Env::new(&["formats", "duplicates"]);
    let first = env.json(&["index", "--json"]);
    assert!(first["converted"].as_u64().unwrap() >= 20, "{first}");
    let second = env.json(&["index", "--json"]);
    assert_eq!(second["converted"], 0, "{second}");
    assert_eq!(second["files_changed"], 0, "{second}");
    assert_eq!(second["embedded_chunks"], 0, "{second}");
    // The files that cannot be converted are not tried again either, but are still listed.
    assert_eq!(
        second["unconverted"].as_array().unwrap().len(),
        5,
        "{second}"
    );
}

/// AC 6: the eval has a questions file for formats and one for duplicates, and in the eval every
/// question of both finds its file.
#[test]
fn the_eval_asks_about_formats_and_duplicates_and_finds_them() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    for file in ["formats.yaml", "duplicates.yaml"] {
        assert!(dir.join("eval/questions").join(file).is_file(), "{file}");
    }
    // The baseline goes to a scratch file: speed depends on the machine, and CI compares it.
    let scratch = TempDir::new().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_brain"))
        .current_dir(dir)
        .env_clear()
        .env("SUPERCHARGE_CACHE_DIR", scratch.path())
        .args([
            "eval",
            "--details",
            "--json",
            "--update-baseline",
            "--baseline",
        ])
        .arg(scratch.path().join("baseline.json"))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    for line in stderr
        .lines()
        .filter(|l| l.starts_with("fmt-") || l.starts_with("dup-"))
    {
        // `<id> terms <rank> about <rank> hybrid <rank>`: the hybrid rank is in the top five.
        let rank = line.split_whitespace().last().unwrap();
        assert!(
            rank.parse::<usize>().is_ok_and(|r| r <= 5),
            "not found in the eval: {line}"
        );
    }
    assert!(
        stderr.lines().filter(|l| l.starts_with("fmt-")).count() >= 17
            && stderr.lines().filter(|l| l.starts_with("dup-")).count() >= 3,
        "{stderr}"
    );
}
