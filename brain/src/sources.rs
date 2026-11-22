//! Which folders a command works on ([ADR 0004](../../docs/adr/0004-brain-sources-per-project.md)):
//! the project's own docs, read in place, and the outside sources its `AGENTS.md` lists.
//! Resolving reads; it never writes.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::{Component, Path, PathBuf};

/// The label of the project's docs in a hit.
pub const PROJECT: &str = "project";
/// What the project's docs source reads besides its docs folder, at the project root.
pub const PROJECT_FILES: [&str; 2] = ["README.md", "AGENTS.md"];
const DEFAULT_DOCS: &str = "docs";

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// The project: its docs folder (relative to the root, `/`-separated; empty is the root),
    /// `README.md` and `AGENTS.md`. Never written by the brain.
    Project { docs: String },
    /// A folder listed in `brain:` or given with `--source`: every text file in it.
    Outside,
}

#[derive(Debug, Clone)]
pub struct Source {
    /// `project`, or the path as it was listed.
    pub label: String,
    pub kind: Kind,
    /// Absolute; canonical when the folder exists.
    pub root: PathBuf,
    pub exists: bool,
    /// Only a `brain:` entry can set it: the one source agents write their own files into.
    pub update: bool,
}

impl Source {
    pub fn is_project(&self) -> bool {
        matches!(self.kind, Kind::Project { .. })
    }

    /// What names this source's index, so projects that list the same folder share it.
    pub fn key(&self) -> String {
        match self.kind {
            Kind::Project { .. } => format!("project:{}", self.root.display()),
            Kind::Outside => self.root.to_string_lossy().into_owned(),
        }
    }

    /// An outside source, for tests and the eval.
    pub fn outside(label: &str, root: &Path) -> Source {
        Source {
            label: label.to_string(),
            kind: Kind::Outside,
            root: root.to_path_buf(),
            exists: root.is_dir(),
            update: false,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct Config {
    docs: Option<String>,
    #[serde(default)]
    brain: Vec<Entry>,
}

#[derive(Debug, Deserialize)]
struct Entry {
    path: String,
    #[serde(default)]
    update: bool,
}

/// The YAML block under the `## Supercharge` heading of an `AGENTS.md`; no block, no config.
fn config_of(agents_md: &str) -> Result<Config> {
    let mut lines = agents_md.lines();
    if !lines.any(|l| l.trim_end() == "## Supercharge" || l.starts_with("## Supercharge ")) {
        return Ok(Config::default());
    }
    let mut block = String::new();
    let mut inside = false;
    for line in lines {
        if inside {
            if line.trim_start().starts_with("```") {
                break;
            }
            block.push_str(line);
            block.push('\n');
        } else if line.trim_start().starts_with("```yaml")
            || line.trim_start().starts_with("```yml")
        {
            inside = true;
        } else if line.starts_with("## ") {
            break;
        }
    }
    serde_yaml_ng::from_str::<Option<Config>>(&block)
        .map(Option::unwrap_or_default)
        .context("the config block under `## Supercharge` in AGENTS.md is not valid: `docs:` is a folder, `brain:` a list of `path:` with an optional `update:`")
}

/// The project containing `from`: the nearest folder with a `.git`, else the nearest with an
/// `AGENTS.md`.
pub fn find_project(from: &Path) -> Option<PathBuf> {
    from.ancestors()
        .find(|d| d.join(".git").exists())
        .or_else(|| from.ancestors().find(|d| d.join("AGENTS.md").is_file()))
        .map(Path::to_path_buf)
}

/// What the command line says, which wins over what `AGENTS.md` says.
#[derive(Debug, Default)]
pub struct Flags {
    /// The project's folder; by default the one containing the current directory.
    pub repo: Option<PathBuf>,
    /// Outside sources; when given, they replace the `brain:` list.
    pub sources: Vec<PathBuf>,
}

#[derive(Debug)]
pub struct Resolved {
    pub project: Option<Source>,
    pub outside: Vec<Source>,
    /// Sources that were left out, and why (a listed folder this machine lacks).
    pub warnings: Vec<String>,
}

impl Resolved {
    /// The sources a search covers: the project, then the outside sources that exist.
    pub fn live(&self) -> Vec<&Source> {
        self.project
            .iter()
            .chain(self.outside.iter().filter(|s| s.exists))
            .collect()
    }

    /// The source agents write their own files into, if the project set one.
    pub fn update_source(&self) -> Option<&Source> {
        self.outside.iter().find(|s| s.update)
    }
}

/// An absolute path with its symlinks resolved as far as it exists, and `.` and `..` applied in
/// order: a folder that isn't there yet still lands where its parent really is.
fn resolve_path(abs: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in abs.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            Component::Normal(name) => {
                out.push(name);
                if let Ok(real) = out.canonicalize() {
                    out = real;
                }
            }
            c => out.push(c),
        }
    }
    out
}

fn expand(listed: &str, base: &Path) -> Result<PathBuf> {
    let home = || {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .context("HOME is not set, so `~` in a source path cannot be resolved")
    };
    let path = if listed == "~" {
        home()?
    } else if let Some(rest) = listed.strip_prefix("~/") {
        home()?.join(rest)
    } else {
        PathBuf::from(listed)
    };
    let abs = if path.is_absolute() {
        path
    } else {
        base.join(path)
    };
    Ok(resolve_path(&abs))
}

/// The docs folder from the config, relative to the project and inside it.
fn docs_folder(docs: Option<&str>) -> Result<String> {
    let docs = docs.unwrap_or(DEFAULT_DOCS);
    let path = Path::new(docs);
    let mut parts = Vec::new();
    for c in path.components() {
        match c {
            Component::Normal(p) => parts.push(p.to_string_lossy().into_owned()),
            Component::CurDir => {}
            _ => bail!("`docs: {docs}` in AGENTS.md must be a folder inside the project"),
        }
    }
    Ok(parts.join("/"))
}

pub fn resolve(cwd: &Path, flags: &Flags) -> Result<Resolved> {
    let cwd = cwd
        .canonicalize()
        .with_context(|| format!("cannot read the current directory {}", cwd.display()))?;
    let root = match &flags.repo {
        Some(dir) => Some(
            dir.canonicalize()
                .with_context(|| format!("the project folder {} does not exist", dir.display()))?,
        ),
        None => find_project(&cwd),
    };
    let mut config = Config::default();
    if let Some(root) = &root {
        match std::fs::read_to_string(root.join("AGENTS.md")) {
            Ok(text) => config = config_of(&text)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).context("reading AGENTS.md"),
        }
    }
    let project = match &root {
        Some(root) => Some(Source {
            label: PROJECT.to_string(),
            kind: Kind::Project {
                docs: docs_folder(config.docs.as_deref())?,
            },
            root: root.clone(),
            exists: true,
            update: false,
        }),
        None => None,
    };

    let listed: Vec<(String, PathBuf, bool)> = if flags.sources.is_empty() {
        let base = root.as_deref().unwrap_or(&cwd);
        config
            .brain
            .iter()
            .map(|e| Ok((e.path.clone(), expand(&e.path, base)?, e.update)))
            .collect::<Result<_>>()?
    } else {
        flags
            .sources
            .iter()
            .map(|p| {
                let text = p.to_string_lossy().into_owned();
                let path = expand(&text, &cwd)?;
                Ok((text, path, false))
            })
            .collect::<Result<_>>()?
    };
    let mut outside: Vec<Source> = Vec::new();
    let mut warnings = Vec::new();
    for (label, path, update) in listed {
        // The same folder listed twice (an alias, a symlink) is one source, and writable if any
        // entry for it says so.
        if let Some(seen) = outside.iter_mut().find(|s| s.root == path) {
            seen.update |= update;
            continue;
        }
        let exists = path.is_dir();
        if !exists && !update {
            warnings.push(format!(
                "the source {label} is not a folder on this machine, so it is left out"
            ));
        }
        // A folder that is literally called `project` must not pass for the project's docs.
        let label = if label == PROJECT {
            format!("./{label}")
        } else {
            label
        };
        outside.push(Source {
            label,
            kind: Kind::Outside,
            root: path,
            exists,
            update,
        });
    }
    let updating: Vec<&Source> = outside.iter().filter(|s| s.update).collect();
    if updating.len() > 1 {
        bail!(
            "two sources set `update: true`: {} and {}; at most one source per project may",
            updating[0].label,
            updating[1].label
        );
    }
    if let (Some(project), Some(writable)) = (&project, updating.first()) {
        if writable.root.starts_with(&project.root) {
            bail!(
                "the source {} is the project or inside it, so `update: true` would make agents write into the project; point it at a folder outside",
                writable.label
            );
        }
    }
    if project.is_none() && outside.is_empty() {
        bail!(
            "no project here: run `brain` inside a project (a folder with a .git or an AGENTS.md), or give --repo <folder> or --source <folder>"
        );
    }
    Ok(Resolved {
        project,
        outside,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn agents(config: &str) -> String {
        format!("# AGENTS.md\n\nAbout.\n\n## Supercharge\n\n```yaml\n{config}\n```\n")
    }

    fn project(config: Option<&str>) -> TempDir {
        let dir = TempDir::new().unwrap();
        if let Some(c) = config {
            fs::write(dir.path().join("AGENTS.md"), agents(c)).unwrap();
        }
        dir
    }

    fn resolved(dir: &Path) -> Result<Resolved> {
        resolve(dir, &Flags::default())
    }

    #[test]
    fn the_config_block_gives_docs_and_the_brain_list() {
        let c = config_of(&agents(
            "risk: medium   # a comment\ndocs: handbook\nbrain:\n  - path: ~/brains/client-a\n    update: true\n  - path: ../policies\n",
        ))
        .unwrap();
        assert_eq!(c.docs.as_deref(), Some("handbook"));
        assert_eq!(c.brain.len(), 2);
        assert!(c.brain[0].update && !c.brain[1].update);
    }

    #[test]
    fn without_a_block_or_with_an_empty_one_there_is_no_config() {
        assert!(config_of("# AGENTS.md\n\nnothing\n")
            .unwrap()
            .brain
            .is_empty());
        assert!(config_of(&agents("")).unwrap().docs.is_none());
        // Another YAML block earlier in the file is not the config.
        let other = "```yaml\ndocs: elsewhere\n```\n\n## Supercharge\n\n```yaml\nrisk: low\n```\n";
        assert!(config_of(other).unwrap().docs.is_none());
        // A block under a later heading is not either.
        let later = "## Supercharge\n\nText.\n\n## Other\n\n```yaml\ndocs: x\n```\n";
        assert!(config_of(later).unwrap().docs.is_none());
    }

    #[test]
    fn a_broken_config_is_an_error_not_silence() {
        assert!(config_of(&agents("brain: nope")).is_err());
        assert!(config_of(&agents("brain:\n  - update: true")).is_err());
    }

    #[test]
    fn the_project_is_the_nearest_git_folder_else_the_nearest_agents_md() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        fs::create_dir_all(root.join("a/b/c")).unwrap();
        assert_eq!(find_project(&root.join("a/b/c")), None);
        fs::write(root.join("a/AGENTS.md"), "x").unwrap();
        assert_eq!(find_project(&root.join("a/b/c")), Some(root.join("a")));
        fs::create_dir(root.join(".git")).unwrap();
        assert_eq!(find_project(&root.join("a/b/c")), Some(root.clone()));
    }

    #[test]
    fn nothing_listed_means_the_project_alone_with_the_default_docs_folder() {
        let dir = project(Some("risk: low"));
        let r = resolved(dir.path()).unwrap();
        assert_eq!(
            r.project.unwrap().kind,
            Kind::Project {
                docs: "docs".into()
            }
        );
        assert!(r.outside.is_empty() && r.warnings.is_empty());
    }

    #[test]
    fn paths_are_absolute_home_or_relative_to_the_project() {
        let home = TempDir::new().unwrap();
        let outer = TempDir::new().unwrap();
        let proj = outer.path().join("repo");
        fs::create_dir_all(&proj).unwrap();
        for d in ["abs", "rel", "elsewhere"] {
            fs::create_dir_all(outer.path().join(d)).unwrap();
        }
        fs::create_dir_all(home.path().join("brains/a")).unwrap();
        fs::write(
            proj.join("AGENTS.md"),
            agents(&format!(
                "brain:\n  - path: {}\n  - path: ~/brains/a\n  - path: ../rel\n  - path: ../elsewhere/../rel",
                outer.path().join("abs").display()
            )),
        )
        .unwrap();
        std::env::set_var("HOME", home.path());
        let r = resolved(&proj).unwrap();
        let roots: Vec<_> = r.outside.iter().map(|s| s.root.clone()).collect();
        let canon = |p: PathBuf| p.canonicalize().unwrap();
        assert_eq!(
            roots,
            [
                canon(outer.path().join("abs")),
                canon(home.path().join("brains/a")),
                canon(outer.path().join("rel")),
            ],
            "the last entry names the third folder again, so it is listed once"
        );
        assert_eq!(r.outside[1].label, "~/brains/a");
    }

    #[test]
    fn a_missing_source_is_left_out_with_a_warning() {
        let dir = project(Some("brain:\n  - path: ./nowhere"));
        let r = resolved(dir.path()).unwrap();
        assert_eq!(r.warnings.len(), 1);
        assert!(r.warnings[0].contains("./nowhere"));
        assert!(r.live().iter().all(|s| s.is_project()));
    }

    #[test]
    fn two_sources_that_update_stop_with_both_named() {
        let dir = project(Some(
            "brain:\n  - path: ./one\n    update: true\n  - path: ./two\n    update: true",
        ));
        let e = format!("{:#}", resolved(dir.path()).unwrap_err());
        assert!(
            e.contains("./one") && e.contains("./two") && e.contains("update: true"),
            "{e}"
        );
    }

    #[test]
    fn flags_replace_the_list_and_cannot_update() {
        let dir = project(Some("brain:\n  - path: ./listed\n    update: true"));
        let extra = TempDir::new().unwrap();
        let r = resolve(
            dir.path(),
            &Flags {
                repo: None,
                sources: vec![extra.path().to_path_buf()],
            },
        )
        .unwrap();
        assert_eq!(r.outside.len(), 1);
        assert!(r.update_source().is_none());
        assert_eq!(r.outside[0].root, extra.path().canonicalize().unwrap());
    }

    #[test]
    fn docs_must_stay_inside_the_project() {
        assert_eq!(docs_folder(None).unwrap(), "docs");
        assert_eq!(docs_folder(Some("./handbook/")).unwrap(), "handbook");
        assert_eq!(docs_folder(Some(".")).unwrap(), "");
        assert!(docs_folder(Some("../docs")).is_err());
        assert!(docs_folder(Some("/etc")).is_err());
    }

    #[test]
    fn outside_any_project_with_no_source_there_is_nothing_to_do() {
        let dir = TempDir::new().unwrap();
        let e = format!("{:#}", resolved(dir.path()).unwrap_err());
        assert!(e.contains("no project here"), "{e}");
        let ok = resolve(
            dir.path(),
            &Flags {
                repo: None,
                sources: vec![dir.path().to_path_buf()],
            },
        )
        .unwrap();
        assert!(ok.project.is_none() && ok.live().len() == 1);
    }

    #[test]
    fn index_keys_tell_a_project_from_a_source_at_the_same_path() {
        let dir = project(None);
        let p = Source {
            label: PROJECT.into(),
            kind: Kind::Project {
                docs: "docs".into(),
            },
            root: dir.path().to_path_buf(),
            exists: true,
            update: false,
        };
        let o = Source::outside("x", dir.path());
        assert_ne!(p.key(), o.key());
    }

    #[test]
    fn a_source_agents_write_into_cannot_be_the_project_or_inside_it() {
        for entry in [
            "  - path: .\n    update: true",
            "  - path: ./brain\n    update: true",
        ] {
            let dir = project(Some(&format!("brain:\n{entry}")));
            let e = format!("{:#}", resolved(dir.path()).unwrap_err());
            assert!(e.contains("write into the project"), "{entry}: {e}");
        }
        // Read-only, the same folder is fine: it is only read.
        let dir = project(Some("brain:\n  - path: ./brain"));
        assert!(resolved(dir.path()).is_ok());
        // A symlink out of the project to a folder inside it does not get round the rule.
        let dir = project(Some("brain:\n  - path: ../alias\n    update: true"));
        let parent = dir.path().parent().unwrap();
        let alias = parent.join(format!(
            "alias-{}",
            dir.path().file_name().unwrap().to_string_lossy()
        ));
        std::os::unix::fs::symlink(dir.path(), &alias).unwrap();
        fs::write(
            dir.path().join("AGENTS.md"),
            agents(&format!(
                "brain:\n  - path: {}\n    update: true",
                alias.display()
            )),
        )
        .unwrap();
        assert!(resolved(dir.path()).is_err());
        fs::remove_file(alias).unwrap();
    }

    #[test]
    fn a_folder_that_does_not_exist_yet_lands_where_its_symlinked_parent_really_is() {
        let outer = TempDir::new().unwrap();
        let real = outer.path().join("srv/team/subdir");
        fs::create_dir_all(&real).unwrap();
        std::os::unix::fs::symlink(&real, outer.path().join("link")).unwrap();
        let got = resolve_path(&outer.path().join("link/../new-brain"));
        assert_eq!(
            got,
            outer
                .path()
                .canonicalize()
                .unwrap()
                .join("srv/team/new-brain")
        );
        // And an existing folder resolves as canonicalize does.
        assert_eq!(
            resolve_path(&outer.path().join("srv/./team/../team")),
            outer.path().canonicalize().unwrap().join("srv/team")
        );
    }

    #[test]
    fn the_same_folder_listed_twice_is_one_source_and_writable_if_any_entry_says_so() {
        let outer = TempDir::new().unwrap();
        let proj = outer.path().join("repo");
        fs::create_dir_all(&proj).unwrap();
        fs::create_dir(outer.path().join("brain")).unwrap();
        fs::write(
            proj.join("AGENTS.md"),
            agents("brain:\n  - path: ../brain\n  - path: ../repo/../brain\n    update: true"),
        )
        .unwrap();
        let r = resolved(&proj).unwrap();
        assert_eq!(r.outside.len(), 1);
        assert!(r.update_source().is_some());
        assert_eq!(r.outside[0].label, "../brain");
    }

    #[test]
    fn a_source_folder_called_project_is_not_mistaken_for_the_project() {
        let outer = TempDir::new().unwrap();
        let proj = outer.path().join("repo");
        fs::create_dir_all(&proj).unwrap();
        fs::create_dir(outer.path().join("project")).unwrap();
        let r = resolve(
            &proj,
            &Flags {
                repo: None,
                sources: vec![PathBuf::from("../project")],
            },
        )
        .unwrap();
        assert_eq!(r.outside[0].label, "../project");
        let r = resolve(
            outer.path(),
            &Flags {
                repo: Some(proj),
                sources: vec![PathBuf::from("project")],
            },
        )
        .unwrap();
        assert_eq!(r.outside[0].label, "./project");
        assert_eq!(r.project.unwrap().label, PROJECT);
    }
}
