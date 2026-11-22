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
fn search_indexes_first_finds_what_changed_and_forgets_what_went() {
    let env = small_brain();
    // No `index` before: the search brings the index up to date itself. An exact term, including
    // an ID that isn't valid full-text syntax as typed, asked as a plain question.
    assert_eq!(
        env.paths(&["search", "Who paid invoice INV-2024-0042?", "--json"])[0],
        "finance/invoices.md"
    );
    // A question that shares no words with its answer, and an Icelandic one.
    let paths = env.paths(&["search", "Can I bring my dog to work?", "--json", "-k", "5"]);
    assert!(
        paths.contains(&"policies/animals.md".to_string()),
        "{paths:?}"
    );
    let paths = env.paths(&[
        "search",
        "Hvað fæ ég marga frídaga á ári?",
        "--json",
        "-k",
        "3",
    ]);
    assert_eq!(paths[0], "icelandic/orlof.md", "{paths:?}");
    // The question may be given as separate words.
    let words = env.paths(&[
        "search",
        "how",
        "often",
        "are",
        "computers",
        "replaced",
        "--json",
    ]);
    assert_eq!(words[0], "it/laptops.md", "{words:?}");
    // Hits carry the settled fields.
    let hits = env.json(&["search", "how often are laptops replaced", "--json"]);
    for key in [
        "source", "path", "file", "title", "heading", "date", "kind", "status", "score", "snippet",
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
    assert_eq!(
        env.paths(&["search", "What is the door code, zebrafish?", "--json"])[0],
        "misc/chat-export.txt"
    );
    // A changed file is found by its new content, with no `index` between.
    env.write(
        "it/laptops.md",
        "# Laptops\n\nNew laptops are replaced every three years now, with a walrus sticker.\n",
    );
    assert_eq!(
        env.paths(&["search", "Which laptops carry a walrus sticker?", "--json"])[0],
        "it/laptops.md"
    );
    // Only the changed file was embedded again; `stats` says so.
    let last = env.json(&["stats", "--json"])["sources"][0]["last_index"].clone();
    assert_eq!(
        (
            last["files_changed"].as_u64(),
            last["embedded_chunks"].as_u64()
        ),
        (Some(1), Some(1)),
        "{last}"
    );
    // A deleted file leaves the index.
    fs::remove_file(env.brain.path().join("it/laptops.md")).unwrap();
    assert!(!env
        .paths(&["search", "Which laptops carry a walrus sticker?", "--json"])
        .contains(&"it/laptops.md".to_string()));
}

#[test]
fn nothing_is_written_into_the_brain() {
    let env = small_brain();
    let before = snapshot(env.brain.path());
    env.json(&["index", "--json"]);
    env.json(&["search", "Which laptops do we use?", "--json"]);
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
fn eval_prints_the_metrics_and_fails_on_a_regression() {
    let corpus = TempDir::new().unwrap();
    let questions = TempDir::new().unwrap();
    let env = small_brain();
    for path in [
        "policies/animals.md",
        "policies/holidays.md",
        "finance/invoices.md",
        "it/laptops.md",
        "icelandic/orlof.md",
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
  expect: [policies/animals.md]
- id: laptop-age
  question: How often do computers get swapped out?
  expect: [it/laptops.md]
- id: invoice
  question: Who paid INV-2024-0042?
  expect: [finance/invoices.md]
- id: leave
  question: How many vacation days do employees get?
  expect: [policies/holidays.md]
- id: rollback
  question: What do I type to undo a bad release ROLLBACK-77?
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
    for needle in [
        "recall@5",
        "MRR",
        "p50",
        "chunks/s",
        "hybrid",
        "full-text",
        "meaning",
    ] {
        assert!(text.contains(needle), "eval output lacks {needle}:\n{text}");
    }
    let json: Value = serde_json::from_str(&fs::read_to_string(&baseline).unwrap()).unwrap();
    let recall = |mode: &str| json[mode]["recall_at_5"].as_f64().unwrap();
    assert!(recall("hybrid") >= recall("full_text"));
    assert!(recall("hybrid") >= recall("meaning"));
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
/// and a source its AGENTS.md lists is read beside them; nothing is written into either.
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
    let hit = |question: &str| -> Vec<(String, String)> {
        serde_json::from_str::<Value>(&run(&["search", question, "--json"]))
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
        hit("when does a paused invoice resume")[0].1,
        "docs/PRODUCT.md"
    );
    assert!(hit("can I bring my dog to work")
        .contains(&("../policies".to_string(), "animals.md".to_string())));
    assert!(hit("what day is the plant rota")
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
    let stats = run(&["stats"]);
    assert!(
        stats.contains("../brain") && !stats.contains("agents write"),
        "{stats}"
    );
    assert!(!at("brain/BRAIN.md").exists() && !at("repo/decisions").exists());
}

/// What is decided stays found, with its status: inside a project, an ADR that a later one
/// superseded comes after the current one, marked with what replaced it; an amended one brings
/// its amendments; and nothing is written to the project.
#[test]
fn a_superseded_adr_comes_after_what_replaced_it_and_an_amended_adr_brings_its_amendments() {
    let dir = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();
    let write = |p: &str, t: &str| {
        let full = dir.path().join(p);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, t).unwrap();
    };
    write("repo/AGENTS.md", "# AGENTS.md\n\nA service.\n");
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
    let repo = dir.path().join("repo");
    let search = |args: &[&str]| -> Vec<Value> {
        let mut full = vec!["search"];
        full.extend_from_slice(args);
        full.push("--json");
        let out = Command::new(env!("CARGO_BIN_EXE_brain"))
            .current_dir(&repo)
            .env_clear()
            .env("SUPERCHARGE_CACHE_DIR", cache.path())
            .args(&full)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{full:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice::<Value>(&out.stdout)
            .unwrap()
            .as_array()
            .cloned()
            .unwrap()
    };
    let paths = |hits: &[Value]| -> Vec<String> {
        hits.iter()
            .map(|h| h["path"].as_str().unwrap().to_string())
            .collect()
    };
    let before = snapshot(&repo);

    let hits = search(&["How often are invoices batched?", "-k", "10"]);
    let found = paths(&hits);
    let at = |p: &str| found.iter().position(|f| f == p).unwrap();
    assert!(
        at("docs/adr/0003-daily.md") < at("docs/adr/0001-weekly.md"),
        "{found:?}"
    );
    let weekly = &hits[at("docs/adr/0001-weekly.md")];
    assert_eq!(weekly["status"], "superseded");
    assert_eq!(weekly["superseded_by"], "docs/adr/0003-daily.md");
    assert_eq!(hits[at("docs/adr/0003-daily.md")]["status"], "accepted");

    let amended = search(&["Amounts are stored in ISK and EUR", "-k", "1"]);
    assert_eq!(
        paths(&amended),
        ["docs/adr/0002-currency.md", "docs/adr/0004-nok.md"]
    );
    assert_eq!(
        paths(&search(&["Who pays for Selkie? The bookkeepers?"]))[0],
        "docs/BUSINESS.md"
    );
    assert_eq!(before, snapshot(&repo), "nothing in the project is written");
}
