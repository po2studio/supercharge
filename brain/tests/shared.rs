//! The shared index (feature 0002, slice 6), through the `brain` CLI with the real model and a fake
//! `gh` that serves a release from a folder: a brain's CI packs its index, a fresh machine pulls
//! it, and falls back to building when nothing matches or there is no network.

use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use serde_json::Value;
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Output},
};
use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_brain");
const VERSION: &str = env!("CARGO_PKG_VERSION");
const MODEL: &str = "multilingual-e5-small-int8";

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .args([
            "-c",
            "init.defaultBranch=main",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn write(root: &Path, path: &str, text: &str) {
    let p = root.join(path);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

/// The brain repo as CI sees it: a git repo on GitHub with the workflow in it.
fn brain_repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    let r = dir.path();
    git(r, &["init", "-q"]);
    git(
        r,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/brain.git",
        ],
    );
    write(r, ".github/workflows/brain.yml", "name: brain\n");
    write(r, "policies/animals.md", "# Animals\n\nAnimals are not permitted on the premises except certified assistance canines.\n");
    write(r, "policies/holidays.md", "# Holidays\n\nStaff get 25 days of paid leave per year, taken in agreement with the team lead.\n");
    write(
        r,
        "finance/invoices.md",
        "# Invoices\n\nInvoice INV-2024-0042 was paid by Hafnarbakki ehf on 3 March.\n",
    );
    git(r, &["add", "-A"]);
    git(r, &["commit", "-qm", "first"]);
    dir
}

/// A fresh clone of the brain on another machine, its origin still GitHub's.
fn clone_of(brain: &Path) -> TempDir {
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["clone", "-q", brain.to_str().unwrap(), "."]);
    git(
        dir.path(),
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com/acme/brain.git",
        ],
    );
    dir
}

/// A project that lists the brain as an outside source.
fn project_listing(brain: &Path) -> TempDir {
    let dir = TempDir::new().unwrap();
    let r = dir.path();
    git(r, &["init", "-q"]);
    write(
        r,
        "README.md",
        "# Courier app\n\nCouriers deliver parcels.\n",
    );
    write(
        r,
        "docs/PRODUCT.md",
        "# Product\n\nThe signup form asks for a second phone number.\n",
    );
    write(
        r,
        "AGENTS.md",
        &format!(
            "# AGENTS.md\n\n## Supercharge\n\n```yaml\nbrain:\n  - path: {}\n```\n",
            brain.display()
        ),
    );
    git(r, &["add", "-A"]);
    git(r, &["commit", "-qm", "first"]);
    dir
}

/// A fake `gh` serving the release `brain-index` from a folder: `release view` prints `view.json`,
/// `release download` copies the asset asked for. Every call is logged.
struct FakeGh {
    bin: TempDir,
    release: TempDir,
}

impl FakeGh {
    fn new() -> Self {
        let g = FakeGh {
            bin: TempDir::new().unwrap(),
            release: TempDir::new().unwrap(),
        };
        let script = g.bin.path().join("gh");
        fs::write(
            &script,
            r#"#!/bin/sh
echo "$@" >> "$FAKE_GH_DIR/calls.log"
if [ -f "$FAKE_GH_DIR/fail" ]; then cat "$FAKE_GH_DIR/fail" >&2; exit 1; fi
[ "$1 $2 $3" = "release view brain-index" ] && { cat "$FAKE_GH_DIR/view.json"; exit 0; }
if [ "$1 $2 $3" = "release download brain-index" ]; then
  while [ $# -gt 0 ]; do
    case "$1" in --pattern) pattern=$2;; --dir) dir=$2;; esac
    shift
  done
  cp "$FAKE_GH_DIR/$pattern" "$dir/$pattern" || exit 1
  exit 0
fi
echo "unexpected: $@" >&2
exit 1
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        }
        g.publish(&[]);
        g
    }

    /// Serves these assets as the release's, newest last.
    fn publish(&self, names: &[&str]) {
        let assets: Vec<Value> = names
            .iter()
            .enumerate()
            .map(|(i, n)| {
                serde_json::json!({
                    "name": n,
                    "size": fs::metadata(self.release.path().join(n)).map_or(0, |m| m.len()),
                    "createdAt": format!("2026-11-22T10:00:{i:02}Z"),
                })
            })
            .collect();
        write(
            self.release.path(),
            "view.json",
            &serde_json::json!({ "assets": assets }).to_string(),
        );
    }

    fn calls(&self) -> String {
        fs::read_to_string(self.release.path().join("calls.log")).unwrap_or_default()
    }

    fn fail_with(&self, message: &str) {
        write(self.release.path(), "fail", message);
    }

    fn path(&self) -> String {
        format!("{}:/usr/bin:/bin", self.bin.path().display())
    }
}

fn brain(cwd: &Path, cache: &Path, path: &str, gh_dir: &Path, args: &[&str]) -> Output {
    Command::new(BIN)
        .current_dir(cwd)
        .env_clear()
        .env("PATH", path)
        .env("SUPERCHARGE_CACHE_DIR", cache)
        .env("FAKE_GH_DIR", gh_dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args(args)
        .output()
        .unwrap()
}

fn ok(out: Output) -> String {
    assert!(
        out.status.success(),
        "failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// What the brain's CI does: pack its index into a folder, and return the parts it made.
fn pack(brain: &Path, gh: &FakeGh, extra: &[&str]) -> Vec<String> {
    let cache = TempDir::new().unwrap();
    let out_dir = gh.release.path().to_str().unwrap();
    let mut args = vec!["pack", "--out", out_dir];
    args.extend_from_slice(extra);
    let out = ok(brain_in(brain, cache.path(), gh, &args));
    let names: Vec<String> = out
        .lines()
        .filter_map(|l| l.strip_prefix("asset "))
        .map(|n| n.trim().to_string())
        .collect();
    assert!(!names.is_empty(), "pack named no asset: {out}");
    for name in &names {
        assert!(
            gh.release.path().join(name).is_file(),
            "{name} was not written"
        );
    }
    names
}

fn brain_in(cwd: &Path, cache: &Path, gh: &FakeGh, args: &[&str]) -> Output {
    brain(cwd, cache, &gh.path(), gh.release.path(), args)
}

fn json(out: Output) -> Value {
    serde_json::from_str(&ok(out)).unwrap()
}

fn source<'a>(index: &'a Value, label: &str) -> &'a Value {
    index["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["source"] == label)
        .unwrap_or_else(|| panic!("no source {label}: {index}"))
}

fn asset_name(model: &str, version: &str, commit: &str) -> String {
    format!("brain-index_{model}_{version}_{commit}.sqlite.gz.001")
}

// AC 1: a fresh clone of a brain whose CI has published an index downloads it and embeds only
// the files changed since that commit, plus the project's docs.
#[test]
fn a_fresh_clone_pulls_the_published_index_and_embeds_only_what_changed() {
    let maker = brain_repo();
    let gh = FakeGh::new();
    let names = pack(maker.path(), &gh, &[]);
    let commit = git(maker.path(), &["rev-parse", "HEAD"]);
    assert_eq!(names, [asset_name(MODEL, VERSION, &commit)]);
    gh.publish(&[&names[0]]);

    let fresh = clone_of(maker.path());
    write(
        fresh.path(),
        "policies/holidays.md",
        "# Holidays\n\nStaff get 30 days of paid leave per year, taken in agreement with the team lead.\n",
    );
    let project = project_listing(fresh.path());
    let cache = TempDir::new().unwrap();
    let index = json(brain_in(
        project.path(),
        cache.path(),
        &gh,
        &["index", "--json"],
    ));

    let label = fresh.path().to_str().unwrap();
    let brain_run = source(&index, label);
    assert_eq!(brain_run["files"], 3, "{index}");
    assert_eq!(brain_run["files_changed"], 1, "{index}");
    let changed_chunks = brain_run["embedded_chunks"].as_u64().unwrap();
    assert!(changed_chunks >= 1 && changed_chunks < brain_run["chunks"].as_u64().unwrap());
    assert_eq!(brain_run["shared"]["commit"], commit.as_str(), "{index}");
    // The project's docs are built here, never pulled.
    let project_run = source(&index, "project");
    assert_eq!(project_run["files_changed"], project_run["files"]);
    assert!(project_run.get("shared").is_none());

    let calls = gh.calls();
    assert_eq!(calls.matches("release download").count(), 1, "{calls}");
    assert!(calls.contains("--repo acme/brain"), "{calls}");

    // `stats` shows what the last run embedded and where the index came from.
    let stats = json(brain_in(
        project.path(),
        cache.path(),
        &gh,
        &["stats", "--json"],
    ));
    let s = source(&stats, label);
    assert_eq!(s["last_index"]["files_changed"], 1);
    assert_eq!(s["last_index"]["embedded_chunks"], changed_chunks);
    assert_eq!(s["shared_index"]["commit"], commit.as_str());

    // The pulled index finds things, and an index that is there is never fetched again.
    let hits = json(brain_in(
        project.path(),
        cache.path(),
        &gh,
        &["search", "--terms", "INV-2024-0042", "--json"],
    ));
    assert!(hits
        .as_array()
        .unwrap()
        .iter()
        .any(|h| h["path"] == "finance/invoices.md"));
    ok(brain_in(project.path(), cache.path(), &gh, &["index"]));
    assert_eq!(gh.calls().matches("release download").count(), 1);
}

// A big index goes up in parts, each under what a release asset may hold, and comes back whole.
#[test]
fn an_index_in_several_parts_comes_back_whole() {
    let maker = brain_repo();
    let gh = FakeGh::new();
    let parts = pack(maker.path(), &gh, &["--part-bytes", "2000"]);
    assert!(parts.len() >= 2, "{parts:?}");
    for (i, p) in parts.iter().enumerate() {
        assert!(p.ends_with(&format!(".{:03}", i + 1)), "{p}");
        assert!(fs::metadata(gh.release.path().join(p)).unwrap().len() <= 2000);
    }
    let all: Vec<&str> = parts.iter().map(String::as_str).collect();
    gh.publish(&all);
    let fresh = clone_of(maker.path());
    let project = project_listing(fresh.path());
    let cache = TempDir::new().unwrap();
    let index = json(brain_in(
        project.path(),
        cache.path(),
        &gh,
        &["index", "--json"],
    ));
    let run = source(&index, fresh.path().to_str().unwrap());
    assert_eq!(run["files_changed"], 0, "{index}");
    assert_eq!(run["embedded_chunks"], 0, "{index}");
    assert_eq!(gh.calls().matches("release download").count(), parts.len());
}

/// The index file of `asset`, unpacked, with the meta `key` set to `value`, and packed again.
fn tampered(gh: &FakeGh, asset: &str, key: &str, value: &str, as_name: &str) {
    let mut raw = Vec::new();
    GzDecoder::new(fs::File::open(gh.release.path().join(asset)).unwrap())
        .read_to_end(&mut raw)
        .unwrap();
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("index.sqlite");
    fs::write(&db, raw).unwrap();
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute("UPDATE meta SET value = ?1 WHERE key = ?2", [value, key])
            .unwrap();
        conn.pragma_update(None, "journal_mode", "DELETE").unwrap();
    }
    let mut enc = GzEncoder::new(
        fs::File::create(gh.release.path().join(as_name)).unwrap(),
        Compression::fast(),
    );
    enc.write_all(&fs::read(&db).unwrap()).unwrap();
    enc.finish().unwrap();
}

// An index still being uploaded (its first part is there and is not a whole index) must not cost
// the machine the older whole one.
#[test]
fn a_newest_index_that_cannot_be_unpacked_falls_back_to_the_one_before() {
    let maker = brain_repo();
    let gh = FakeGh::new();
    let good = pack(maker.path(), &gh, &[]).remove(0);
    let broken = asset_name(MODEL, VERSION, &"e".repeat(40));
    fs::write(gh.release.path().join(&broken), b"not a gzip file").unwrap();
    gh.publish(&[&good, &broken]);
    let fresh = clone_of(maker.path());
    let project = project_listing(fresh.path());
    let cache = TempDir::new().unwrap();
    let index = json(brain_in(
        project.path(),
        cache.path(),
        &gh,
        &["index", "--json"],
    ));
    let run = source(&index, fresh.path().to_str().unwrap());
    assert_eq!(run["embedded_chunks"], 0, "{index}");
    assert_eq!(
        run["shared"]["commit"],
        git(maker.path(), &["rev-parse", "HEAD"]).as_str()
    );
    assert_eq!(gh.calls().matches("release download").count(), 2);
}

// `pack` keys the index by the commit, so it refuses a folder holding indexable files the commit
// does not, and honours --repo like every command.
#[test]
fn pack_refuses_ignored_files_and_honours_repo() {
    let maker = brain_repo();
    write(maker.path(), ".gitignore", "scratch/\n");
    git(maker.path(), &["add", "-A"]);
    git(maker.path(), &["commit", "-qm", "ignore scratch"]);
    write(
        maker.path(),
        "scratch/draft.md",
        "# Draft\n\nNot in the commit.\n",
    );
    let gh = FakeGh::new();
    let cache = TempDir::new().unwrap();
    let elsewhere = TempDir::new().unwrap();
    let out_dir = gh.release.path().to_str().unwrap().to_string();
    let args = [
        "--repo",
        maker.path().to_str().unwrap(),
        "pack",
        "--out",
        &out_dir,
    ];
    let out = brain_in(elsewhere.path(), cache.path(), &gh, &args);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("scratch/draft.md") && stderr.contains("not in the commit"),
        "{stderr}"
    );
    fs::remove_dir_all(maker.path().join("scratch")).unwrap();
    ok(brain_in(elsewhere.path(), cache.path(), &gh, &args));
}

// AC 2: an index from another model or binary version is ignored, and a local build runs.
#[test]
fn an_index_of_another_model_or_version_is_ignored_and_the_index_is_built_here() {
    let maker = brain_repo();
    let gh = FakeGh::new();
    let real = pack(maker.path(), &gh, &[]).remove(0);
    let commit = git(maker.path(), &["rev-parse", "HEAD"]);
    // Named for another model and another version: never even downloaded.
    let other_model = asset_name("some-other-model", VERSION, &commit);
    let other_version = asset_name(MODEL, "0.0.1", &commit);
    for n in [&other_model, &other_version] {
        fs::copy(gh.release.path().join(&real), gh.release.path().join(n)).unwrap();
    }
    // Named right, but built by another binary version inside: downloaded, checked, thrown away.
    let liar = asset_name(MODEL, VERSION, &"f".repeat(40));
    tampered(&gh, &real, "binary_version", "0.0.1", &liar);
    gh.publish(&[&other_model, &other_version, &liar]);

    let fresh = clone_of(maker.path());
    let project = project_listing(fresh.path());
    let cache = TempDir::new().unwrap();
    let out = brain_in(project.path(), cache.path(), &gh, &["index", "--json"]);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let index = json(out);
    let run = source(&index, fresh.path().to_str().unwrap());
    assert_eq!(run["files_changed"], run["files"], "{index}");
    assert_eq!(run["embedded_chunks"], run["chunks"], "{index}");
    assert!(run.get("shared").is_none());
    assert!(stderr.contains("building the index locally"), "{stderr}");
    let calls = gh.calls();
    assert_eq!(calls.matches("release download").count(), 1, "{calls}");
    assert!(calls.contains(&liar), "{calls}");
    assert!(
        !calls.contains("some-other-model") && !calls.contains("0.0.1_"),
        "{calls}"
    );
}

// AC 3: with no network, indexing builds locally and says why.
#[test]
fn with_no_network_it_builds_locally_and_says_why() {
    let maker = brain_repo();
    let gh = FakeGh::new();
    gh.fail_with("error connecting to api.github.com\ncheck your internet connection");
    let fresh = clone_of(maker.path());
    let project = project_listing(fresh.path());
    let cache = TempDir::new().unwrap();
    let out = brain_in(project.path(), cache.path(), &gh, &["index", "--json"]);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let index = json(out);
    let run = source(&index, fresh.path().to_str().unwrap());
    assert_eq!(run["embedded_chunks"], run["chunks"], "{index}");
    assert!(
        stderr.contains("building the index locally")
            && stderr.contains("error connecting to api.github.com"),
        "{stderr}"
    );

    // And with no `gh` at all.
    let cache = TempDir::new().unwrap();
    let out = brain(
        project.path(),
        cache.path(),
        "/usr/bin:/bin",
        gh.release.path(),
        &["index", "--json"],
    );
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(json(out)["files"].as_u64().unwrap() > 0);
    assert!(
        stderr.contains("building the index locally") && stderr.contains("gh"),
        "{stderr}"
    );
}

// What no brain asks for: a source that isn't a brain repo is indexed with no network call at all.
#[test]
fn a_folder_that_is_no_brain_repo_never_asks_github() {
    let plain = TempDir::new().unwrap();
    write(plain.path(), "a.md", "# A\n\nSome words.\n");
    let gh = FakeGh::new();
    let cache = TempDir::new().unwrap();
    let out = brain_in(
        plain.path(),
        cache.path(),
        &gh,
        &["--source", plain.path().to_str().unwrap(), "index"],
    );
    ok(out);
    assert_eq!(gh.calls(), "");
}

// AC 5 (at a size a test can afford): `brain bench` reports search p50 and indexing chunks/s.
#[test]
fn bench_builds_a_synthetic_brain_and_reports_search_and_indexing_speed() {
    let cache = TempDir::new().unwrap();
    let out = brain(
        cache.path(),
        cache.path(),
        "/usr/bin:/bin",
        cache.path(),
        &[
            "bench",
            "--chunks",
            "3000",
            "--embed-sample",
            "64",
            "--json",
        ],
    );
    let r = json(out);
    assert_eq!(r["chunks"], 3000);
    assert!(r["index_chunks_per_s"].as_f64().unwrap() > 0.0, "{r}");
    assert!(r["search_p50_ms"].as_f64().unwrap() > 0.0, "{r}");
    assert!(r["search_terms_p50_ms"].as_f64().unwrap() >= 0.0, "{r}");
    // Nothing is left behind but what the user asked for.
    let left: Vec<PathBuf> = fs::read_dir(cache.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

// A relative `--dir` (or one through a symlink, as macOS's /tmp is) names the same synthetic brain
// to the bench and to the `brain search` processes it starts.
#[test]
fn bench_works_with_a_relative_dir() {
    let cwd = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();
    fs::create_dir(cwd.path().join("work")).unwrap();
    let out = brain(
        cwd.path(),
        cache.path(),
        "/usr/bin:/bin",
        cache.path(),
        &[
            "bench",
            "--chunks",
            "800",
            "--embed-sample",
            "8",
            "--dir",
            "work",
            "--json",
        ],
    );
    assert_eq!(json(out)["chunks"], 800);
}
