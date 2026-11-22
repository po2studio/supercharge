//! The "Ask the brain" flow through the `brain` CLI, with the real model and no network: index a
//! folder, search it, change a file, index again, and never write into the folder.

use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
use tempfile::TempDir;

/// A machine with one folder of documents, read as a source from a directory that is no project.
struct Env {
    brain: TempDir,
    cache: TempDir,
    cwd: TempDir,
}

impl Env {
    fn new(files: &[(&str, &str)]) -> Self {
        let env = Env {
            brain: TempDir::new().unwrap(),
            cache: TempDir::new().unwrap(),
            cwd: TempDir::new().unwrap(),
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
            self.cwd.path(),
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

fn run(bin: &str, cwd: &Path, source: &Path, cache: &Path, args: &[&str]) -> Output {
    Command::new(bin)
        .current_dir(cwd)
        .env_clear()
        .env("SUPERCHARGE_CACHE_DIR", cache)
        .arg("--source")
        .arg(source)
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
        env.json(&["stats", "--json"])["sources"][0]["last_index"]["embedded_chunks"],
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
fn files_with_no_text_are_listed_not_failed() {
    let env = small_brain();
    env.write("scans/receipt.pdf", "%PDF-1.4 not really");
    env.write("photos/office.png", "not a picture either");
    env.write(".git/config", "[core]");
    let index = env.json(&["index", "--json"]);
    assert_eq!(index["files"], 5);
    let listed: Vec<&str> = index["unconverted"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["path"].as_str().unwrap())
        .collect();
    assert_eq!(listed, ["photos/office.png", "scans/receipt.pdf"]);
    let stats = env.json(&["stats", "--json"]);
    assert_eq!(
        stats["sources"][0]["last_index"]["unconverted"][1]["path"],
        "scans/receipt.pdf"
    );
    let text = String::from_utf8_lossy(&env.cmd(&["stats"]).stdout).to_string();
    assert!(
        text.contains("not converted (2)") && text.contains("photos/office.png: image"),
        "{text}"
    );
}

/// A workbook with a cell at A1 and one at XFD1048576 has 17 billion cells in its range.
fn huge_xlsx() -> Vec<u8> {
    use std::io::Write;
    let mut out = Vec::new();
    {
        let mut z = zip::ZipWriter::new(std::io::Cursor::new(&mut out));
        let opts = zip::write::SimpleFileOptions::default();
        let parts = [
            (
                "[Content_Types].xml",
                r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"/>"#,
            ),
            (
                "_rels/.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
            ),
            (
                "xl/workbook.xml",
                r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="S" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
            ),
            (
                "xl/worksheets/sheet1.xml",
                r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>first</t></is></c></row><row r="1048576"><c r="XFD1048576" t="inlineStr"><is><t>last</t></is></c></row></sheetData></worksheet>"#,
            ),
        ];
        for (name, body) in parts {
            z.start_file(name, opts).unwrap();
            z.write_all(body.as_bytes()).unwrap();
        }
        z.finish().unwrap();
    }
    out
}

/// A page whose content draws a form that draws itself.
fn looping_pdf() -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /XObject << /X 5 0 R >> >> /Contents 4 0 R >>".to_string(),
        "<< /Length 5 >>\nstream\n/X Do\nendstream".to_string(),
        "<< /Type /XObject /Subtype /Form /BBox [0 0 100 100] /Resources << /XObject << /X 5 0 R >> >> /Length 5 >>\nstream\n/X Do\nendstream".to_string(),
    ];
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
    }
    let xref = pdf.len();
    pdf.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
    for o in offsets {
        pdf.extend(format!("{o:010} 00000 n \n").bytes());
    }
    pdf.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .bytes(),
    );
    pdf
}

#[test]
fn a_hostile_file_is_listed_and_does_not_take_the_indexer_down() {
    let env = small_brain();
    fs::write(env.brain.path().join("huge.xlsx"), huge_xlsx()).unwrap();
    fs::write(env.brain.path().join("loop.pdf"), looping_pdf()).unwrap();
    let mut wide = "a,".repeat(20_000);
    wide.push('\n');
    wide.push_str(&"1\n".repeat(2_000));
    env.write("wide.csv", &wide);
    let index = env.json(&["index", "--json"]);
    assert_eq!(index["files"], 5, "{index}");
    let listed: Vec<&str> = index["unconverted"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["path"].as_str().unwrap())
        .collect();
    for file in ["huge.xlsx", "wide.csv"] {
        assert!(listed.contains(&file), "{file} not listed: {listed:?}");
    }
    // The page that draws itself is read as nothing or refused, never a hang or a crash of `brain`.
    assert!(listed.contains(&"loop.pdf"), "{listed:?}");
}

#[test]
fn a_text_file_over_the_size_cap_is_listed_as_left_out() {
    let env = small_brain();
    fs::File::create(env.brain.path().join("dump.txt"))
        .unwrap()
        .set_len((1 << 20) + 1)
        .unwrap();
    let index = env.json(&["index", "--json"]);
    assert_eq!(index["files"], 5);
    assert!(
        index["skipped"][0]
            .as_str()
            .unwrap()
            .ends_with(": dump.txt (over 1 MiB)"),
        "{index}"
    );
    let out = env.cmd(&["stats"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("left out (1)") && text.contains("dump.txt (over 1 MiB)"),
        "{text}"
    );
}

#[test]
fn one_huge_unbroken_line_is_indexed_in_bounded_time() {
    let env = small_brain();
    // Letters and digits with no space, newline or full stop: only a hard cut can split it.
    let mut x: u64 = 88172645463325252;
    let blob: String = (0..150_000)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            char::from(b"abcdefghijklmnopqrstuvwxyz0123456789"[(x % 36) as usize])
        })
        .collect();
    env.write("dump/blob.txt", &blob);
    let mut child = Command::new(env!("CARGO_BIN_EXE_brain"))
        .current_dir(env.cwd.path())
        .env_clear()
        .env("SUPERCHARGE_CACHE_DIR", env.cache.path())
        .arg("--source")
        .arg(env.brain.path())
        .args(["index", "--json"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let started = std::time::Instant::now();
    while child.try_wait().unwrap().is_none() {
        if started.elapsed() > std::time::Duration::from_secs(300) {
            child.kill().unwrap();
            panic!("indexing a 150 KB unbroken line took over 300 s");
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    let out: Value = serde_json::from_reader(child.stdout.take().unwrap()).unwrap();
    assert_eq!(out["files_changed"], 6, "{out}");
    assert!(out["chunks"].as_u64() > Some(100), "{out}");
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
    let project = TempDir::new().unwrap();
    fs::create_dir_all(project.path().join("docs")).unwrap();
    fs::write(
        project.path().join("docs/deploy.md"),
        "# Deploy\n\nRoll back a bad release with make rollback and the token ROLLBACK-77.\n",
    )
    .unwrap();
    fs::write(
        project.path().join("README.md"),
        "# The project\n\nA service.\n",
    )
    .unwrap();
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
- id: rollback
  question: What do I type to undo a bad release?
  terms: ROLLBACK-77
  expect: ["project:docs/deploy.md"]
"#,
    )
    .unwrap();
    let baseline = env.cache.path().join("baseline.json");
    let eval = |extra: &[&str]| {
        let mut args = vec![
            "eval",
            "--corpus",
            corpus.path().to_str().unwrap(),
            "--project",
            project.path().to_str().unwrap(),
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
    assert!(text.contains("2 sources"), "{text}");
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

/// The flow as a person runs it: inside a project, no flags. The project's docs are read in place,
/// a source its AGENTS.md lists is read beside them, and `init` creates the layout only where
/// `update: true` says.
#[test]
fn inside_a_project_with_no_flags_the_docs_and_the_listed_sources_are_searched() {
    let dir = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();
    let at = |p: &str| dir.path().join(p);
    let write = |p: &str, t: &str| {
        let full = at(p);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, t).unwrap();
    };
    write(
        "repo/AGENTS.md",
        "# AGENTS.md\n\n## Supercharge\n\n```yaml\ndocs: docs\nbrain:\n  - path: ../policies\n  - path: ../brain\n    update: true\n```\n",
    );
    write(
        "repo/README.md",
        "# Selkie\n\nSelkie chases late invoices.\n",
    );
    write(
        "repo/docs/PRODUCT.md",
        "# Product\n\nA paused invoice stays paused until someone resumes it.\n",
    );
    write("policies/animals.md", "# Animals\n\nAnimals are not permitted on the premises except certified assistance canines.\n");
    write(
        "brain/old-decision.md",
        "# Old\n\nThe office plant rota is on Fridays.\n",
    );
    let repo = at("repo");
    let run = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_brain"))
            .current_dir(&repo)
            .env_clear()
            .env("SUPERCHARGE_CACHE_DIR", cache.path())
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let sources_before = (
        snapshot(&at("policies")),
        snapshot(&at("brain")),
        snapshot(&repo),
    );
    run(&["index"]);
    let hit = |args: &[&str]| -> Vec<(String, String)> {
        let mut full = vec!["search", "--json"];
        full.extend_from_slice(args);
        serde_json::from_str::<Value>(&run(&full))
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|h| {
                (
                    h["source"].as_str().unwrap().into(),
                    h["path"].as_str().unwrap().into(),
                )
            })
            .collect()
    };
    assert_eq!(
        hit(&["--about", "when does a paused invoice resume"])[0].1,
        "docs/PRODUCT.md"
    );
    assert!(hit(&["--about", "can I bring my dog to work"])
        .contains(&("../policies".to_string(), "animals.md".to_string())));
    assert!(hit(&["--terms", "plant"])
        .contains(&("../brain".to_string(), "old-decision.md".to_string())));
    assert_eq!(
        sources_before,
        (
            snapshot(&at("policies")),
            snapshot(&at("brain")),
            snapshot(&repo)
        ),
        "reading writes nothing"
    );
    assert!(run(&["stats"]).contains("agents write here"));
    run(&["init"]);
    assert!(at("brain/BRAIN.md").is_file() && at("brain/decisions").is_dir());
    assert!(!at("policies/BRAIN.md").exists() && !at("repo/decisions").exists());
}

/// What is decided stays found, with its status: inside a project, a decision superseded in the
/// `update: true` source and an ADR superseded or amended in the project's docs are searched
/// where they are. The old ones come back only with `--all`, pointing to what replaced them, and
/// nothing is written to the project.
#[test]
fn a_superseded_decision_never_comes_back_as_current_and_an_amended_adr_brings_its_amendments() {
    let dir = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();
    let write = |p: &str, t: &str| {
        let full = dir.path().join(p);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, t).unwrap();
    };
    write(
        "repo/AGENTS.md",
        "# AGENTS.md\n\n## Supercharge\n\n```yaml\ndocs: docs\nbrain:\n  - path: ../brain\n    update: true\n```\n",
    );
    write(
        "repo/docs/BUSINESS.md",
        "# Business\n\nSelkie is sold to bookkeepers, who are the paying customer.\n",
    );
    write("repo/docs/adr/0001-weekly.md", "# 0001: Invoices go out weekly\n\nStatus: superseded by [0003](0003-daily.md)\n\nInvoices are batched weekly.\n");
    write("repo/docs/adr/0002-currency.md", "# 0002: Currencies\n\nStatus: accepted; amended by [0004](0004-nok.md)\n\nAmounts are stored in ISK and EUR.\n");
    write(
        "repo/docs/adr/0003-daily.md",
        "# 0003: Invoices go out daily\n\nStatus: accepted\n\nInvoices are batched daily.\n",
    );
    write(
        "repo/docs/adr/0004-nok.md",
        "# 0004: NOK too\n\nStatus: accepted\n\nNorwegian krone amounts are stored as well.\n",
    );
    write(
        "brain/decisions/2023-11-02-single-phone.md",
        "---\ntitle: One phone number\ndate: 2023-11-02\nkind: product\nstatus: superseded\nsuperseded-by: 2026-01-10-sms-fallback\n---\nSignup asks for one phone number.\n",
    );
    write(
        "brain/decisions/2026-01-10-sms-fallback.md",
        "---\ntitle: A second phone number\ndate: 2026-01-10\nkind: product\nstatus: accepted\nsupersedes: [2023-11-02-single-phone]\n---\nSignup asks for a second phone number, an SMS fallback.\n",
    );
    let repo = dir.path().join("repo");
    let run = |args: &[&str]| -> Vec<Value> {
        let out = Command::new(env!("CARGO_BIN_EXE_brain"))
            .current_dir(&repo)
            .env_clear()
            .env("SUPERCHARGE_CACHE_DIR", cache.path())
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let text = String::from_utf8_lossy(&out.stdout);
        serde_json::from_str::<Value>(&text)
            .unwrap_or(Value::Null)
            .as_array()
            .cloned()
            .unwrap_or_default()
    };
    let before = snapshot(&repo);
    run(&["index"]);
    let paths = |hits: &[Value]| -> Vec<String> {
        hits.iter()
            .map(|h| h["path"].as_str().unwrap().to_string())
            .collect()
    };
    let search = |extra: &[&str]| {
        let mut args = vec!["search", "--json", "-k", "10"];
        args.extend_from_slice(extra);
        run(&args)
    };

    let phone = paths(&search(&["--terms", "phone number"]));
    assert_eq!(phone, ["decisions/2026-01-10-sms-fallback.md"]);
    let all = search(&["--terms", "phone number", "--all"]);
    let old = all
        .iter()
        .find(|h| h["path"] == "decisions/2023-11-02-single-phone.md")
        .unwrap();
    assert_eq!(old["status"], "superseded");
    assert_eq!(old["superseded_by"], "decisions/2026-01-10-sms-fallback.md");
    assert_eq!(old["kind"], "product");

    let batch = paths(&search(&["--terms", "invoices batched"]));
    assert!(
        batch.contains(&"docs/adr/0003-daily.md".to_string()),
        "{batch:?}"
    );
    assert!(
        !batch.contains(&"docs/adr/0001-weekly.md".to_string()),
        "{batch:?}"
    );
    let adr = search(&["--terms", "invoices batched", "--all"]);
    let weekly = adr
        .iter()
        .find(|h| h["path"] == "docs/adr/0001-weekly.md")
        .unwrap();
    assert_eq!(weekly["superseded_by"], "docs/adr/0003-daily.md");

    let amended = run(&["search", "--json", "--terms", "ISK EUR", "-k", "1"]);
    assert_eq!(
        paths(&amended),
        ["docs/adr/0002-currency.md", "docs/adr/0004-nok.md"]
    );
    assert_eq!(
        paths(&search(&["--terms", "bookkeepers"]))[0],
        "docs/BUSINESS.md"
    );
    assert_eq!(
        paths(&search(&[
            "--terms",
            "phone number",
            "--kind",
            "product",
            "--since",
            "2025-01-01"
        ])),
        phone
    );
    assert_eq!(before, snapshot(&repo), "nothing in the project is written");
}

fn sha256(text: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

/// A note as an agent writes it: frontmatter with the sources it was made from, and a body.
fn note(title: &str, questions: &[&str], sources: &[(&str, &str)], verified: &str) -> String {
    let mut text = format!(
        "---\ntitle: {title}\nsummary: {title}, in one line\naliases: [trip rules]\nquestions:\n"
    );
    for q in questions {
        text += &format!("  - {q}\n");
    }
    text += "sources:\n";
    for (path, content) in sources {
        text += &format!("  - path: {path}\n    hash: {}\n", sha256(content));
    }
    text += &format!("verified: {verified}\n---\n\n");
    text += "Trips need approval ten days ahead, and meals abroad are a flat EUR 45 a day.\n";
    text
}

/// Notes stay true: a note whose sources changed or went is flagged `stale` in hits and listed
/// by `stats`, and ranks below the raw files; a refreshed note is fresh again and ranks first; a
/// note whose only source is gone is reported; two notes from the same sources are reported as
/// duplicates. Nothing in a source is written.
#[test]
fn a_note_is_stale_when_its_sources_change_and_fresh_again_once_refreshed() {
    let dir = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();
    let write = |p: &str, t: &str| {
        let full = dir.path().join(p);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, t).unwrap();
    };
    write(
        "repo/AGENTS.md",
        "# AGENTS.md\n\n## Supercharge\n\n```yaml\ndocs: docs\nbrain:\n  - path: ../policies\n  - path: ../brain\n    update: true\n```\n",
    );
    let product = "# Product\n\nA paused invoice stays paused until someone resumes it.\n";
    write("repo/docs/PRODUCT.md", product);
    let travel =
        "# Trips\n\nEvery business trip needs written approval ten days before departure.\n";
    let meals = "# Meals\n\nOn trips abroad meals are reimbursed at a flat EUR 45 per day.\n";
    write("policies/travel.md", travel);
    write("policies/meals.md", meals);
    write(
        "policies/old-rule.md",
        "# Old rule\n\nParking is free on Fridays.\n",
    );
    let trip_note = |verified: &str, travel: &str| {
        note(
            "Business trip approval and meals",
            &["How early must a trip be approved and what can I claim for meals?"],
            &[
                ("../policies:travel.md", travel),
                ("../policies:meals.md", meals),
            ],
            verified,
        )
    };
    write("brain/notes/trip.md", &trip_note("2025-03-01", travel));
    // The same two sources again: two notes about the same thing.
    write(
        "brain/notes/trip-again.md",
        &note(
            "Trips, once more",
            &["Rules for business trips?"],
            &[
                ("../policies:meals.md", meals),
                ("../policies:travel.md", travel),
            ],
            "2025-03-02",
        ),
    );
    // A note on a project file, and one whose only source is deleted below.
    write(
        "brain/notes/paused.md",
        &note(
            "Paused invoices",
            &["What happens to a paused invoice?"],
            &[("project:docs/PRODUCT.md", product)],
            "2025-03-03",
        ),
    );
    write(
        "brain/notes/parking.md",
        &note(
            "Parking on Fridays",
            &["Is parking free on Fridays?"],
            &[(
                "../policies:old-rule.md",
                "# Old rule\n\nParking is free on Fridays.\n",
            )],
            "2025-03-04",
        ),
    );
    let repo = dir.path().join("repo");
    let run = |args: &[&str]| -> Value {
        let out = Command::new(env!("CARGO_BIN_EXE_brain"))
            .current_dir(&repo)
            .env_clear()
            .env("SUPERCHARGE_CACHE_DIR", cache.path())
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap_or(Value::Null)
    };
    let ask =
        || -> Vec<Value> {
            run(&[
            "search", "--json", "-k", "10", "--about",
            "How early must a business trip be approved, and what can I claim for meals abroad?",
        ])
        .as_array()
        .unwrap()
        .clone()
        };
    let notes_of = |stats: &Value| -> Value {
        stats["sources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["source"] == "../brain")
            .unwrap()["notes"]
            .clone()
    };
    let names = |v: &Value| -> Vec<String> {
        let mut out: Vec<String> = v
            .as_array()
            .unwrap()
            .iter()
            .map(|n| {
                n["path"]
                    .as_str()
                    .unwrap_or_else(|| n.as_str().unwrap())
                    .to_string()
            })
            .collect();
        out.sort();
        out
    };

    // Fresh: the note answers the question, is not stale and ranks first.
    let indexed = run(&["index", "--json"]);
    assert_eq!(indexed["notes"]["stale"], 0, "{indexed}");
    assert_eq!(indexed["notes"]["total"], 4);
    let hits = ask();
    assert_eq!(hits[0]["path"], "notes/trip.md", "{hits:?}");
    assert_eq!(hits[0]["kind"], "note");
    assert_eq!(hits[0]["stale"], false);
    assert_eq!(
        hits[0]["date"], "2025-03-01",
        "a note is dated by when it was verified"
    );
    assert_eq!(
        hits[0]["hash"].as_str().unwrap().len(),
        64,
        "hits give the hash a note cites"
    );

    // A source is gone: its note is listed as pointing at nothing, and is stale.
    fs::remove_file(dir.path().join("policies/old-rule.md")).unwrap();
    let indexed = run(&["index", "--json"]);
    assert_eq!(indexed["notes"]["stale"], 1);
    let notes = notes_of(&run(&["stats", "--json"]));
    assert_eq!(names(&notes["orphans"]), ["notes/parking.md"], "{notes}");
    assert!(names(&notes["stale"]).contains(&"notes/parking.md".to_string()));
    // Two notes from the same sources are reported once as duplicates.
    assert_eq!(notes["duplicates"].as_array().unwrap().len(), 1, "{notes}");
    assert_eq!(notes["total"], 4);

    // A raw source changes: the notes built from it are stale, flagged in hits, listed by stats
    // with what changed, and rank below the raw files.
    let changed =
        "# Trips\n\nEvery business trip needs written approval fourteen days before departure.\n";
    write("policies/travel.md", changed);
    let indexed = run(&["index", "--json"]);
    assert_eq!(indexed["notes"]["stale"], 3);
    let hits = ask();
    let at = |p: &str| {
        hits.iter()
            .position(|h| h["path"] == p)
            .unwrap_or(usize::MAX)
    };
    assert_eq!(hits[at("notes/trip.md")]["stale"], true, "{hits:?}");
    assert!(
        at("notes/trip.md") > at("travel.md") && at("notes/trip.md") > at("meals.md"),
        "a stale note ranks below the raw files: {hits:?}"
    );
    let stale = notes_of(&run(&["stats", "--json"]))["stale"].clone();
    assert_eq!(
        names(&stale),
        ["notes/parking.md", "notes/trip-again.md", "notes/trip.md"]
    );
    let trip = stale
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["path"] == "notes/trip.md")
        .unwrap();
    let states: Vec<(&str, &str)> = trip["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["path"].as_str().unwrap(), s["state"].as_str().unwrap()))
        .collect();
    assert_eq!(
        states,
        [
            ("../policies:travel.md", "changed"),
            ("../policies:meals.md", "fresh")
        ]
    );
    assert_eq!(trip["sources"][0]["current"], sha256(changed));
    // The note on the project's file is untouched by it.
    assert!(!names(&stale).contains(&"notes/paused.md".to_string()));
    // Text output says so too.
    let out = Command::new(env!("CARGO_BIN_EXE_brain"))
        .current_dir(&repo)
        .env_clear()
        .env("SUPERCHARGE_CACHE_DIR", cache.path())
        .arg("stats")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("stale notes (3)") && text.contains("notes/trip.md"),
        "{text}"
    );

    // The project's own docs change too.
    write(
        "repo/docs/PRODUCT.md",
        "# Product\n\nA paused invoice resumes after 7 days.\n",
    );
    run(&["index", "--json"]);
    let stale = notes_of(&run(&["stats", "--json"]))["stale"].clone();
    assert!(
        names(&stale).contains(&"notes/paused.md".to_string()),
        "{stale}"
    );

    // Refreshed: new hash, today's `verified`; fresh again, ranked first again.
    write("brain/notes/trip.md", &trip_note("2025-04-01", changed));
    run(&["index", "--json"]);
    let hits = ask();
    assert_eq!(hits[0]["path"], "notes/trip.md", "{hits:?}");
    assert_eq!(hits[0]["stale"], false);
    assert_eq!(hits[0]["date"], "2025-04-01");
    assert!(!names(&notes_of(&run(&["stats", "--json"]))["stale"])
        .contains(&"notes/trip.md".to_string()));
    // Reading and indexing wrote nothing: the sources hold what this test put there.
    let listing = |d: &str| -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(dir.path().join(d))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    };
    assert_eq!(listing("brain"), ["notes"]);
    assert_eq!(listing("policies"), ["meals.md", "travel.md"]);
    assert_eq!(listing("repo"), ["AGENTS.md", "docs"]);
}
