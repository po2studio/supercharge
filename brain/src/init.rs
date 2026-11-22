//! `brain init`: the only command that writes into a source, and only when asked. It creates the
//! files and folders agents own in the project's `update: true` source, if they are missing.

use anyhow::{Context, Result};
use std::{fs, path::Path};

use crate::sources::Source;

/// Where each agent-owned folder is described, in `BRAIN.md`.
pub const BRAIN_MD: &str = "# Brain

This folder is a brain: documents that agents search, and decisions they keep. Format version 1.

Everything here is raw, and agents never edit it, except what agents own:

- `BRAIN.md`: this file.
- `decisions/`: business, product and process decisions, one file each, named `YYYY-MM-DD-slug.md`.
- `notes/`: summaries agents wrote, each naming the files it came from.
- `projects/`: one file per project, the last commit the agents reconciled.
- `.github/workflows/brain.yml`: the CI check, when this folder is a GitHub repo.

The search index is not kept here; it lives under `~/.cache/supercharge/` on each machine.
";

const FOLDERS: [&str; 3] = ["decisions", "notes", "projects"];

/// Creates what the `update: true` source lacks, and nothing else; returns what it created, as
/// paths relative to the source (`.` for the folder itself).
pub fn init(source: &Source) -> Result<Vec<String>> {
    let root = &source.root;
    let mut created = Vec::new();
    if !root.is_dir() {
        fs::create_dir_all(root).with_context(|| format!("creating {}", root.display()))?;
        created.push(".".to_string());
    }
    make(root, "BRAIN.md", &mut created, write_new)?;
    for folder in FOLDERS {
        make(root, folder, &mut created, |p| fs::create_dir(p))?;
    }
    Ok(created)
}

/// Creates `name` if nothing is there; whatever is there, even created a moment ago by another
/// run, is left as it is.
fn make(
    root: &Path,
    name: &str,
    created: &mut Vec<String>,
    create: impl Fn(&Path) -> std::io::Result<()>,
) -> Result<()> {
    let path = root.join(name);
    match create(&path) {
        Ok(()) => created.push(name.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e).with_context(|| format!("creating {}", path.display())),
    }
    Ok(())
}

/// Writes `BRAIN.md` only where there is no file: `create_new` fails instead of truncating.
fn write_new(path: &Path) -> std::io::Result<()> {
    use std::io::Write;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?
        .write_all(BRAIN_MD.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::{resolve, Flags, Resolved};
    use std::path::PathBuf;
    use tempfile::TempDir;

    /// A project `repo` whose AGENTS.md lists `brain_entry`, beside the folders it points at.
    struct World {
        outer: TempDir,
    }

    impl World {
        fn new(brain_entry: &str) -> World {
            let outer = TempDir::new().unwrap();
            fs::create_dir(outer.path().join("repo")).unwrap();
            fs::write(
                outer.path().join("repo/AGENTS.md"),
                format!("## Supercharge\n\n```yaml\nbrain:\n{brain_entry}\n```\n"),
            )
            .unwrap();
            World { outer }
        }

        fn at(&self, rel: &str) -> PathBuf {
            self.outer.path().join(rel)
        }

        fn resolved(&self) -> Resolved {
            resolve(&self.at("repo"), &Flags::default()).unwrap()
        }

        fn listing(&self, rel: &str) -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(self.at(rel))
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        }
    }

    #[test]
    fn it_creates_only_what_is_missing_and_never_touches_raw_files() {
        let w = World::new("  - path: ../brain\n    update: true");
        fs::create_dir(w.at("brain")).unwrap();
        fs::write(w.at("brain/policy.md"), "raw").unwrap();
        fs::create_dir(w.at("brain/notes")).unwrap();
        fs::write(w.at("brain/notes/kept.md"), "mine").unwrap();
        let r = w.resolved();
        assert_eq!(
            init(r.update_source().unwrap()).unwrap(),
            ["BRAIN.md", "decisions", "projects"],
            "notes/ was there"
        );
        assert_eq!(
            w.listing("brain"),
            ["BRAIN.md", "decisions", "notes", "policy.md", "projects"]
        );
        assert_eq!(fs::read_to_string(w.at("brain/policy.md")).unwrap(), "raw");
        assert_eq!(
            fs::read_to_string(w.at("brain/notes/kept.md")).unwrap(),
            "mine"
        );
        assert!(
            init(r.update_source().unwrap()).unwrap().is_empty(),
            "a second run changes nothing"
        );
        assert_eq!(w.listing("repo"), ["AGENTS.md"], "nothing in the project");
    }

    #[test]
    fn an_existing_brain_md_is_left_as_it_is() {
        let w = World::new("  - path: ../brain\n    update: true");
        fs::create_dir(w.at("brain")).unwrap();
        fs::write(w.at("brain/BRAIN.md"), "their own words").unwrap();
        init(w.resolved().update_source().unwrap()).unwrap();
        assert_eq!(
            fs::read_to_string(w.at("brain/BRAIN.md")).unwrap(),
            "their own words"
        );
    }

    #[test]
    fn a_new_folder_is_created_for_the_source() {
        let w = World::new("  - path: ../brains/new\n    update: true");
        let made = init(w.resolved().update_source().unwrap()).unwrap();
        assert!(made.contains(&"BRAIN.md".to_string()));
        assert!(w.at("brains/new/decisions").is_dir());
    }

    #[test]
    fn a_source_that_does_not_update_is_not_an_update_source_and_stays_empty() {
        let w = World::new("  - path: ../brain");
        fs::create_dir(w.at("brain")).unwrap();
        assert!(w.resolved().update_source().is_none());
        assert!(w.listing("brain").is_empty());
    }

    #[test]
    fn runs_at_the_same_time_all_succeed_and_nothing_is_overwritten() {
        let w = World::new("  - path: ../brains/new\n    update: true");
        let r = w.resolved();
        let source = r.update_source().unwrap();
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let source = source.clone();
                std::thread::spawn(move || init(&source).unwrap())
            })
            .collect();
        let made: usize = threads
            .into_iter()
            .map(|t| {
                t.join()
                    .unwrap()
                    .iter()
                    .filter(|n| *n == "BRAIN.md")
                    .count()
            })
            .sum();
        assert_eq!(made, 1, "exactly one run wrote BRAIN.md");
        assert_eq!(
            fs::read_to_string(w.at("brains/new/BRAIN.md")).unwrap(),
            BRAIN_MD
        );
    }
}
