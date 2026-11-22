//! The shared index: a brain repo's CI builds the index of the brain's own files once and
//! publishes it as an asset of the release `brain-index`, and a machine with no index pulls it
//! instead of embedding everything. It is the same file `index` keeps, so a pulled index is only
//! brought up to date: the files that differ are embedded, nothing else.
//!
//! The asset is named `brain-index_<model>_<binary version>_<commit>.sqlite.gz.001`, and a big index
//! goes on in `.002` and so on: a release asset may not pass 2 GiB, and a million chunks packs to
//! about that. Only an index of this model and this binary version is pulled, and what is pulled is
//! checked once more against the meta the index records. The GitHub access is `gh`, which the
//! bundle already needs.

use crate::{embed, store::Store};
use anyhow::{bail, Context, Result};
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, BufReader, BufWriter},
    path::{Path, PathBuf},
    process::Command,
};

/// The release the CI keeps its indexes in.
pub const TAG: &str = "brain-index";
/// The file the brain skill's CI template is written to; a folder that has it is a brain repo.
const WORKFLOW: &str = ".github/workflows/brain.yml";
const SUFFIX: &str = ".sqlite.gz";
/// The most one part of an index may hold: under the 2 GiB GitHub allows an asset.
pub const PART_BYTES: u64 = 1_900_000_000;

/// What a pulled index is, kept in its meta so `stats` can say where an index came from.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Pulled {
    /// The brain commit it was built at.
    pub commit: String,
    /// Its asset's size, compressed.
    pub bytes: u64,
}

pub enum Outcome {
    /// Not a brain repo: nothing to ask GitHub, and nothing to say.
    NotABrain,
    Pulled(Pulled),
    /// Why a local build runs instead.
    Declined(String),
}

/// The name of an index without its part number.
pub fn base_name(commit: &str) -> String {
    format!(
        "brain-index_{}_{}_{commit}{SUFFIX}",
        embed::MODEL_ID,
        env!("CARGO_PKG_VERSION")
    )
}

/// What an asset name says: the index it is a part of (its name without the part number), the
/// model, binary version and commit, and which part it is.
struct Name<'a> {
    base: &'a str,
    model: &'a str,
    version: &'a str,
    commit: &'a str,
    part: usize,
}

fn parse(name: &str) -> Option<Name<'_>> {
    let (base, part) = name.rsplit_once('.')?;
    let part: usize = part.parse().ok().filter(|&p| p >= 1)?;
    let rest = base.strip_prefix("brain-index_")?.strip_suffix(SUFFIX)?;
    let mut fields = rest.split('_');
    let (model, version, commit) = (fields.next()?, fields.next()?, fields.next()?);
    fields.next().is_none().then_some(Name {
        base,
        model,
        version,
        commit,
        part,
    })
}

/// `owner/name` of a GitHub remote URL (https, scp-like or ssh).
fn github_repo(url: &str) -> Option<String> {
    let rest = url.trim().split_once("github.com")?.1;
    let rest = rest.trim_start_matches([':', '/']);
    let rest = rest.trim_end_matches('/');
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    let (owner, name) = rest.split_once('/')?;
    (!owner.is_empty() && !name.is_empty() && !name.contains('/'))
        .then(|| format!("{owner}/{name}"))
}

/// The GitHub repo of a folder that is a brain repo: `origin` is on github.com and the folder has
/// the CI workflow of the `brain` skill.
fn brain_repo(root: &Path) -> Option<String> {
    if !root.join(WORKFLOW).is_file() {
        return None;
    }
    let out = Command::new("git")
        .args(["config", "--get", "remote.origin.url"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    github_repo(&String::from_utf8_lossy(&out.stdout))
}

/// The first line `gh` said about why it failed.
fn why(out: &std::process::Output) -> String {
    let text = String::from_utf8_lossy(&out.stderr);
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map_or_else(|| format!("gh {}", out.status), |l| format!("gh said: {l}"))
}

fn gh(args: &[&str]) -> Result<std::process::Output, String> {
    match Command::new("gh").args(args).output() {
        Ok(out) if out.status.success() => Ok(out),
        Ok(out) => Err(why(&out)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            Err("`gh` (the GitHub CLI) is not installed".to_string())
        }
        Err(e) => Err(format!("cannot run `gh`: {e}")),
    }
}

#[derive(Deserialize)]
struct View {
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    #[serde(rename = "createdAt")]
    created_at: String,
}

/// The assets of the release, oldest first.
fn assets(repo: &str) -> Result<Vec<Asset>, String> {
    let out = gh(&["release", "view", TAG, "--repo", repo, "--json", "assets"])?;
    let mut view: View = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("gh gave no list of assets: {e}"))?;
    view.assets
        .sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.name.cmp(&b.name)));
    Ok(view.assets)
}

/// Pulls the newest index built by this model and binary version into `index_path`, ready for
/// `index` to bring up to date, when `root` is a brain repo.
pub fn pull(root: &Path, index_path: &Path) -> Outcome {
    let Some(repo) = brain_repo(root) else {
        return Outcome::NotABrain;
    };
    match try_pull(&repo, root, index_path) {
        Ok(p) => Outcome::Pulled(p),
        Err(reason) => Outcome::Declined(reason),
    }
}

/// An index published whole: its parts, in order.
struct Published<'a> {
    base: &'a str,
    commit: &'a str,
    parts: Vec<&'a Asset>,
}

/// The indexes of this model and binary version whose parts are all there, newest first.
fn complete<'a>(all: &'a [Asset]) -> Vec<Published<'a>> {
    let mut found: Vec<Published<'a>> = Vec::new();
    let mut groups: Vec<(Name<'a>, &'a Asset)> = all
        .iter()
        .filter_map(|a| parse(&a.name).map(|n| (n, a)))
        .filter(|(n, _)| n.model == embed::MODEL_ID && n.version == env!("CARGO_PKG_VERSION"))
        .collect();
    groups.sort_by_key(|(n, _)| (n.base, n.part));
    for (name, asset) in groups {
        match found.last_mut() {
            Some(p) if p.base == name.base => p.parts.push(asset),
            _ => found.push(Published {
                base: name.base,
                commit: name.commit,
                parts: vec![asset],
            }),
        }
    }
    // A set that is still being uploaded (a part missing) is not an index yet.
    found.retain(|p| {
        p.parts
            .iter()
            .enumerate()
            .all(|(i, a)| parse(&a.name).is_some_and(|n| n.part == i + 1))
    });
    found.sort_by(|a, b| newest(b).cmp(newest(a)));
    found
}

fn newest<'a>(p: &Published<'a>) -> &'a str {
    p.parts
        .iter()
        .map(|a| a.created_at.as_str())
        .max()
        .unwrap_or("")
}

fn try_pull(repo: &str, root: &Path, index_path: &Path) -> Result<Pulled, String> {
    let all = assets(repo)?;
    let found = complete(&all);
    if found.is_empty() {
        return Err(if all.is_empty() {
            format!("no index is published for {repo} yet")
        } else {
            format!(
                "none of the {} assets published for {repo} is a whole index built by this model and binary version ({} {})",
                all.len(),
                embed::MODEL_ID,
                env!("CARGO_PKG_VERSION")
            )
        });
    }
    let dir = index_path.parent().ok_or("the index has no folder")?;
    let tmp = dir.join("pull");
    // The newest first; one that cannot be used (its upload was still going on, say) costs only
    // the next-newest.
    let mut last = String::new();
    for index in &found {
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
        let result = download(repo, index, root, index_path, &tmp);
        let _ = fs::remove_dir_all(&tmp);
        match result {
            Ok(pulled) => return Ok(pulled),
            Err(why) => last = why,
        }
    }
    Err(last)
}

fn download(
    repo: &str,
    index: &Published,
    root: &Path,
    index_path: &Path,
    tmp: &Path,
) -> Result<Pulled, String> {
    let dir = tmp.to_string_lossy();
    let mut parts = Vec::new();
    let mut bytes = 0;
    for part in &index.parts {
        let name = part.name.as_str();
        gh(&[
            "release",
            "download",
            TAG,
            "--repo",
            repo,
            "--pattern",
            name,
            "--dir",
            &dir,
        ])?;
        let path = tmp.join(name);
        bytes += fs::metadata(&path)
            .map_err(|_| format!("gh downloaded no {name}"))?
            .len();
        parts.push(path);
    }
    let name = index.base;
    let unpacked = tmp.join("index.sqlite");
    unpack(&parts, &unpacked).map_err(|e| format!("{name} is not a usable index: {e:#}"))?;
    for p in &parts {
        let _ = fs::remove_file(p);
    }
    let pulled = Pulled {
        commit: index.commit.to_string(),
        bytes,
    };
    {
        let Some(store) = Store::open(&unpacked).map_err(|e| format!("{name}: {e:#}"))? else {
            return Err(format!(
                "{name} was built by another model, binary version or chunker than this one"
            ));
        };
        let meta = |r: Result<()>| r.map_err(|e| format!("{name}: {e:#}"));
        // Where the files are on this machine, and where the index came from.
        meta(store.set_meta("root", &root.to_string_lossy()))?;
        meta(store.set_meta(
            "shared_index",
            &serde_json::to_string(&pulled).unwrap_or_default(),
        ))?;
        meta(store.checkpoint())?;
    }
    Store::remove_files(index_path);
    for suffix in ["", "-wal", "-shm"] {
        let mut from = unpacked.as_os_str().to_owned();
        from.push(suffix);
        let mut to = index_path.as_os_str().to_owned();
        to.push(suffix);
        if Path::new(&from).exists() {
            fs::rename(&from, &to).map_err(|e| format!("cannot move the index into place: {e}"))?;
        }
    }
    Ok(pulled)
}

/// The parts, one after the other, decompressed into `to`.
fn unpack(parts: &[PathBuf], to: &Path) -> Result<()> {
    let mut joined: Box<dyn io::Read> = Box::new(io::empty());
    for p in parts {
        joined = Box::new(io::Read::chain(joined, BufReader::new(fs::File::open(p)?)));
    }
    let mut output = BufWriter::new(fs::File::create(to)?);
    io::copy(&mut GzDecoder::new(joined), &mut output)?;
    Ok(())
}

/// A writer that goes on in a new file when the one it is writing holds `limit` bytes.
struct Parts {
    dir: PathBuf,
    base: String,
    limit: u64,
    files: Vec<PathBuf>,
    current: Option<BufWriter<fs::File>>,
    written: u64,
}

impl io::Write for Parts {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.current.is_none() || self.written >= self.limit {
            let path = self
                .dir
                .join(format!("{}.{:03}", self.base, self.files.len() + 1));
            if let Some(mut done) = self.current.take() {
                done.flush()?;
            }
            self.current = Some(BufWriter::new(fs::File::create(&path)?));
            self.files.push(path);
            self.written = 0;
        }
        let room = (self.limit - self.written) as usize;
        let n = buf.len().min(room.max(1));
        self.current
            .as_mut()
            .expect("opened above")
            .write_all(&buf[..n])?;
        self.written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.current.as_mut().map_or(Ok(()), io::Write::flush)
    }
}

/// Writes the index of `store`, built at `commit`, into `out` as the parts to publish, none over
/// `part_bytes`; returns their paths in order.
pub fn pack(store: &Store, commit: &str, out: &Path, part_bytes: u64) -> Result<Vec<PathBuf>> {
    store.checkpoint()?;
    pack_file(&store.path()?, &base_name(commit), out, part_bytes)
}

fn pack_file(from: &Path, base: &str, out: &Path, part_bytes: u64) -> Result<Vec<PathBuf>> {
    fs::create_dir_all(out)?;
    let parts = Parts {
        dir: out.to_path_buf(),
        base: base.to_string(),
        limit: part_bytes.max(1),
        files: Vec::new(),
        current: None,
        written: 0,
    };
    let mut encoder = GzEncoder::new(parts, Compression::new(6));
    io::copy(&mut BufReader::new(fs::File::open(from)?), &mut encoder)?;
    let mut parts = encoder.finish()?;
    io::Write::flush(&mut parts)?;
    Ok(parts.files)
}

/// The commit `root` is at, when it is a git repo with nothing uncommitted: the index is keyed by
/// the commit, so it must hold what that commit holds.
pub fn clean_commit(root: &Path) -> Result<String> {
    let git = |args: &[&str]| -> Result<String> {
        let out = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .context("cannot run git")?;
        if !out.status.success() {
            bail!(
                "{} is not a git repo with a commit: {}",
                root.display(),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let commit = git(&["rev-parse", "HEAD"])?;
    if !git(&["status", "--porcelain"])?.is_empty() {
        bail!(
            "{} has uncommitted changes, and the shared index is keyed by the commit",
            root.display()
        );
    }
    // Ignored files are read by the indexer like any other, but are not in the commit. Hidden
    // names (`.cache/`, `.DS_Store`) are never indexed, so they do not count.
    let ignored = git(&[
        "status",
        "--porcelain",
        "--ignored",
        "--untracked-files=all",
    ])?;
    let loose: Vec<&str> = ignored
        .lines()
        .filter_map(|l| l.strip_prefix("!! "))
        .filter(|p| !p.split('/').any(|c| c.starts_with('.')))
        .collect();
    if let Some(first) = loose.first() {
        bail!(
            "{} has ignored files that are not in the commit ({first}{}), and the shared index is keyed by the commit",
            root.display(),
            if loose.len() > 1 { ", …" } else { "" }
        );
    }
    Ok(commit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_names_round_trip() {
        let name = format!("{}.002", base_name("abc123"));
        let n = parse(&name).unwrap();
        assert_eq!(
            (n.model, n.version, n.commit, n.part),
            (embed::MODEL_ID, env!("CARGO_PKG_VERSION"), "abc123", 2)
        );
        assert_eq!(n.base, base_name("abc123"));
        for bad in [
            "brain-linux-x64",
            "brain-index_a_b.sqlite.gz.001",
            "brain-index_a_b_c_d.sqlite.gz.001",
            "brain-index_a_b_c.sqlite.gz",
            "brain-index_a_b_c.sqlite.gz.000",
        ] {
            assert!(parse(bad).is_none(), "{bad}");
        }
    }

    fn asset(commit: &str, part: usize, at: &str) -> Asset {
        Asset {
            name: format!("{}.{part:03}", base_name(commit)),
            created_at: at.to_string(),
        }
    }

    #[test]
    fn the_newest_whole_index_of_this_model_and_version_is_chosen() {
        let mut other = asset("c", 1, "9");
        other.name = other.name.replace(embed::MODEL_ID, "other-model");
        let all = vec![
            asset("a", 1, "1"),
            asset("b", 1, "2"),
            asset("b", 2, "3"),
            asset("c", 1, "4"),
            asset("c", 3, "6"), // part 2 is missing: still being uploaded
            asset("d", 1, "5"),
            asset("d", 2, "5"),
            other,
        ];
        let found = complete(&all);
        let commits: Vec<&str> = found.iter().map(|p| p.commit).collect();
        assert_eq!(commits, ["d", "b", "a"]);
        assert_eq!(found[1].parts.len(), 2);
    }

    #[test]
    fn an_index_goes_through_parts_and_back() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("index.sqlite");
        // Bytes that do not shrink, so the parts are really cut.
        let mut x = 12345u64;
        let data: Vec<u8> = (0..300_000)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x as u8
            })
            .collect();
        fs::write(&src, &data).unwrap();
        let out = dir.path().join("out");
        let parts = pack_file(&src, "brain-index_m_v_c.sqlite.gz", &out, 100_000).unwrap();
        assert!(parts.len() >= 3, "{parts:?}");
        assert!(parts
            .iter()
            .all(|p| fs::metadata(p).unwrap().len() <= 100_000));
        assert!(parts[1].to_string_lossy().ends_with(".002"));
        let back = dir.path().join("back");
        unpack(&parts, &back).unwrap();
        assert_eq!(fs::read(back).unwrap(), data);
        // One part when it fits.
        let one = pack_file(&src, "brain-index_m_v_d.sqlite.gz", &out, PART_BYTES).unwrap();
        assert_eq!(one.len(), 1);
    }

    #[test]
    fn github_remotes_of_every_shape() {
        for url in [
            "https://github.com/acme/brain.git",
            "https://github.com/acme/brain",
            "git@github.com:acme/brain.git",
            "ssh://git@github.com/acme/brain.git",
            "https://x-access-token:t@github.com/acme/brain/",
        ] {
            assert_eq!(github_repo(url).as_deref(), Some("acme/brain"), "{url}");
        }
        for url in [
            "https://gitlab.com/acme/brain.git",
            "/tmp/brain",
            "",
            "https://github.com/acme",
        ] {
            assert_eq!(github_repo(url), None, "{url}");
        }
    }

    #[test]
    fn a_folder_without_the_workflow_is_no_brain_repo() {
        let dir = tempfile::tempdir().unwrap();
        assert!(brain_repo(dir.path()).is_none());
    }
}
