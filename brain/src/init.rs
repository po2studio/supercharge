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
    make(root, "BRAIN.md", &mut created, |p| fs::write(p, BRAIN_MD))?;
    for folder in FOLDERS {
        make(root, folder, &mut created, |p| fs::create_dir(p))?;
    }
    Ok(created)
}

fn make(
    root: &Path,
    name: &str,
    created: &mut Vec<String>,
    create: impl Fn(&Path) -> std::io::Result<()>,
) -> Result<()> {
    let path = root.join(name);
    if path.symlink_metadata().is_ok() {
        return Ok(());
    }
    create(&path).with_context(|| format!("creating {}", path.display()))?;
    created.push(name.to_string());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::{resolve, Flags};
    use tempfile::TempDir;

    fn project_with(brain_entry: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("AGENTS.md"),
            format!("## Supercharge\n\n```yaml\nbrain:\n{brain_entry}\n```\n"),
        )
        .unwrap();
        dir
    }

    fn listing(dir: &TempDir) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir.path().join("brain"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn it_creates_only_what_is_missing_and_never_touches_raw_files() {
        let dir = project_with("  - path: ./brain\n    update: true");
        fs::create_dir(dir.path().join("brain")).unwrap();
        fs::write(dir.path().join("brain/policy.md"), "raw").unwrap();
        fs::create_dir(dir.path().join("brain/notes")).unwrap();
        fs::write(dir.path().join("brain/notes/kept.md"), "mine").unwrap();
        let r = resolve(dir.path(), &Flags::default()).unwrap();
        assert_eq!(
            init(r.update_source().unwrap()).unwrap(),
            ["BRAIN.md", "decisions", "projects"],
            "notes/ was there"
        );
        assert_eq!(
            listing(&dir),
            ["BRAIN.md", "decisions", "notes", "policy.md", "projects"]
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("brain/policy.md")).unwrap(),
            "raw"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("brain/notes/kept.md")).unwrap(),
            "mine"
        );
        assert!(
            init(r.update_source().unwrap()).unwrap().is_empty(),
            "a second run changes nothing"
        );
    }

    #[test]
    fn an_existing_brain_md_is_left_as_it_is() {
        let dir = project_with("  - path: ./brain\n    update: true");
        fs::create_dir(dir.path().join("brain")).unwrap();
        fs::write(dir.path().join("brain/BRAIN.md"), "their own words").unwrap();
        let r = resolve(dir.path(), &Flags::default()).unwrap();
        init(r.update_source().unwrap()).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("brain/BRAIN.md")).unwrap(),
            "their own words"
        );
    }

    #[test]
    fn a_new_folder_is_created_for_the_source() {
        let dir = project_with("  - path: ./brains/new\n    update: true");
        let r = resolve(dir.path(), &Flags::default()).unwrap();
        assert!(init(r.update_source().unwrap())
            .unwrap()
            .contains(&"BRAIN.md".to_string()));
        assert!(dir.path().join("brains/new/decisions").is_dir());
    }

    #[test]
    fn a_source_that_does_not_update_is_not_an_update_source_and_stays_empty() {
        let dir = project_with("  - path: ./brain");
        fs::create_dir(dir.path().join("brain")).unwrap();
        let r = resolve(dir.path(), &Flags::default()).unwrap();
        assert!(r.update_source().is_none());
        assert!(listing(&dir).is_empty());
    }
}
