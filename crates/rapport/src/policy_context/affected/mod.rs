//! Select affected components using revision-specific ownership and review policy.

mod snapshot;
#[cfg(test)]
mod tests;

use crate::CommandContext;
use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};
use rapport_git::{Git, ObjectId, Revision};
use serde::Serialize;
use snapshot::Snapshot;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::process::ExitCode;

#[derive(Debug, clap::Args)]
#[command(group(clap::ArgGroup::new("scope").args(["base", "pending"]).required(true).multiple(true)))]
pub(super) struct Args {
    /// Include staged, unstaged and non-ignored untracked changes.
    #[arg(long)]
    pending: bool,
    /// Compare the merge base of this revision and the selected head through head.
    #[arg(long)]
    base: Option<String>,
    /// Comparison head (defaults to HEAD); requires --base.
    #[arg(long, requires = "base")]
    head: Option<String>,
    /// Emit the versioned structured result on stdout.
    #[arg(long)]
    json: bool,
}

pub(super) fn run<F: FileSystem, O: Write, E: Write>(
    args: &Args,
    context: &mut CommandContext<'_, F, O, E>,
) -> ExitCode {
    match discover(args, &context.cwd).and_then(|result| render(&result, args.json)) {
        Ok(output) => match context.out.write_all(output.as_bytes()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                let _ = writeln!(context.err, "rapport context affected: {error}");
                ExitCode::from(2)
            }
        },
        Err(error) => {
            let _ = writeln!(context.err, "rapport context affected: {error}");
            ExitCode::from(2)
        }
    }
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct Selection {
    schema_version: u8,
    comparison: Option<Comparison>,
    pending: Pending,
    components: Vec<Component>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct Comparison {
    base: String,
    head: String,
    merge_base: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct Pending {
    included: bool,
    head: Option<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct Component {
    path: String,
    changed_files: BTreeSet<String>,
    policy_sources: BTreeSet<String>,
}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("supply --pending, --base, or both; --head requires --base")]
    InvalidScope,
    #[error("--pending requires the selected head {selected} to equal checkout HEAD {checkout}")]
    UnrelatedHead { selected: String, checkout: String },
    #[error("resolve unmerged paths before discovering affected components: {0:?}")]
    Unmerged(BTreeSet<Utf8PathBuf>),
    #[error(
        "component {0:?} was removed; it cannot be reviewed at the selected head/worktree; review its deletion against the earlier tree explicitly"
    )]
    RemovedComponent(String),
    #[error(
        "changed path {0:?} has no enclosing context.toml in its relevant snapshot; declare ownership before reviewing"
    )]
    Unmappable(String),
    #[error(transparent)]
    Git(#[from] rapport_git::GitError),
    #[error(transparent)]
    Policy(#[from] super::Error),
    #[error("cannot read local policy {path}: {source}")]
    Io {
        path: Utf8PathBuf,
        source: std::io::Error,
    },
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

fn resolve(git: &Git, repo: &rapport_git::Repository, revision: &str) -> Result<ObjectId, Error> {
    let revision = Revision::new(revision).map_err(rapport_git::GitError::from)?;
    Ok(git.resolve(repo, &revision)?)
}

fn discover(args: &Args, cwd: &Utf8Path) -> Result<Selection, Error> {
    if (!args.pending && args.base.is_none()) || (args.head.is_some() && args.base.is_none()) {
        return Err(Error::InvalidScope);
    }
    let git = Git::default();
    let repo = git.discover(cwd)?;
    let head = resolve(&git, &repo, args.head.as_deref().unwrap_or("HEAD"))?;
    let checkout = if args.pending {
        let status = git.status(&repo)?;
        if !status.conflicted().is_empty() {
            return Err(Error::Unmerged(status.conflicted().clone()));
        }
        if status.head() != &head {
            return Err(Error::UnrelatedHead {
                selected: head.as_str().into(),
                checkout: status.head().as_str().into(),
            });
        }
        Some(status)
    } else {
        None
    };
    let head_snapshot = Snapshot::committed(&git, &repo, Some(&head))?;
    let mut selected = BTreeMap::new();
    let comparison = if let Some(base) = &args.base {
        let base = resolve(&git, &repo, base)?;
        let merge_base = git.merge_base_between(&repo, &base, &head)?;
        let before = Snapshot::committed(&git, &repo, Some(&merge_base))?;
        select(
            &before,
            &head_snapshot,
            &git.changed_paths(&repo, &merge_base, &head)?,
            &mut selected,
        )?;
        Some(Comparison {
            base: base.as_str().into(),
            head: head.as_str().into(),
            merge_base: merge_base.as_str().into(),
        })
    } else {
        None
    };
    let final_snapshot = if let Some(status) = &checkout {
        let index = Snapshot::committed(&git, &repo, None)?;
        let worktree = Snapshot::working(&git, &repo)?;
        select(
            &head_snapshot,
            &index,
            &git.pending_paths(&repo, true)?,
            &mut selected,
        )?;
        let mut unstaged = git.pending_paths(&repo, false)?;
        unstaged.extend(status.untracked().iter().cloned());
        select(&index, &worktree, &unstaged, &mut selected)?;
        worktree
    } else {
        head_snapshot
    };
    for path in selected.keys() {
        if !final_snapshot.components.contains_key(path) {
            return Err(Error::RemovedComponent(path.clone()));
        }
    }
    Ok(Selection {
        schema_version: 1,
        comparison,
        pending: Pending {
            included: args.pending,
            head: checkout.map(|status| status.head().as_str().into()),
        },
        components: selected.into_values().collect(),
    })
}

fn select(
    before: &Snapshot,
    after: &Snapshot,
    changed: &BTreeSet<Utf8PathBuf>,
    selected: &mut BTreeMap<String, Component>,
) -> Result<(), Error> {
    for file in changed {
        let mut mapped = false;
        for snapshot in [before, after] {
            if snapshot.files.contains(file) {
                let owner = snapshot.owner(file)?;
                component(selected, &owner)
                    .changed_files
                    .insert(file.to_string());
                mapped = true;
            }
        }
        if !mapped {
            return Err(Error::Unmappable(file.to_string()));
        }
    }
    for (path, policy) in &after.components {
        if let Some(old) = before.components.get(path) {
            if old.markdown == policy.markdown {
                continue;
            }
            let sources = old
                .sources
                .keys()
                .chain(policy.sources.keys())
                .filter(|source| {
                    changed.contains(Utf8Path::new(source.as_str()))
                        && old.sources.get(*source) != policy.sources.get(*source)
                });
            component(selected, path)
                .policy_sources
                .extend(sources.cloned());
        }
    }
    Ok(())
}

fn component<'a>(selected: &'a mut BTreeMap<String, Component>, path: &str) -> &'a mut Component {
    selected.entry(path.into()).or_insert_with(|| Component {
        path: path.into(),
        changed_files: BTreeSet::new(),
        policy_sources: BTreeSet::new(),
    })
}

fn render(result: &Selection, json: bool) -> Result<String, Error> {
    if json {
        return Ok(format!("{}\n", serde_json::to_string_pretty(result)?));
    }
    let mut output = String::new();
    for component in &result.components {
        output.push_str(&shell_path(&component.path));
        output.push('\n');
    }
    Ok(output)
}

fn shell_path(path: &str) -> String {
    if path
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"_./-".contains(&b))
        && !path.starts_with('-')
    {
        path.into()
    } else {
        format!("'{}'", path.replace('\'', "'\\''"))
    }
}
