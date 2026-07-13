//! Context Build-signoff mutations and workflow files.
//!
//! This module owns signoff declarations, included paths, generated workflow
//! state, repair, and rollback of multi-file mutations.

use super::cli::{SignoffAction, SignoffIncludeAction};
use super::command::{changed, find_signoff, repository_has_signoffs};
use super::domain::BuildSignoff;
use super::render::{display, or_none};
use super::repository::Repository;
use super::{Error, workflow};
use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};

pub(super) fn run(
    action: &SignoffAction,
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
    runner: &dyn crate::CommandRunner,
) -> Result<String, Error> {
    match action {
        SignoffAction::List { path } => {
            let repository = Repository::load(fs, repo_root)?;
            let record = repository.at(path)?;
            let lines = record
                .context()
                .signoffs()
                .iter()
                .map(|signoff| {
                    format!(
                        "- `{}` — just {} — stage {} — resource {}",
                        signoff.id(),
                        signoff.target(),
                        signoff.stage(),
                        signoff.resource_group().unwrap_or("none")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            Ok(format!(
                "# rapport context signoff list\n\n{}",
                or_none(&lines)
            ))
        }
        SignoffAction::Add {
            path,
            target,
            stage,
            resource_group,
            include,
        } => {
            let mut repository = Repository::load(fs, repo_root)?;
            let record = repository.at(path)?;
            workflow::validate_target(runner, record.directory(), target)?;
            let included = include
                .iter()
                .map(|value| {
                    repository.normalize_included_path(record.directory(), value, fs, true)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let signoff = BuildSignoff::try_new(
                record.context().id(),
                target.clone(),
                *stage,
                resource_group.clone(),
                included,
            )?;
            if record
                .context()
                .signoffs()
                .iter()
                .any(|existing| existing.id() == signoff.id())
            {
                return Err(Error::DuplicateSignoff(signoff.id().to_owned()));
            }
            let record_path = record.path().to_path_buf();
            repository
                .at_mut(path)?
                .context_mut()
                .signoffs_mut()
                .push(signoff);
            repository.validate()?;
            write_signoff_state(fs, repo_root, &repository, &record_path)?;
            Ok(changed("added Build signoff", repository.at(path)?))
        }
        SignoffAction::Remove { path, signoff } => {
            let mut repository = Repository::load(fs, repo_root)?;
            let record = repository.at(path)?;
            let existing = record
                .context()
                .signoffs()
                .iter()
                .find(|candidate| candidate.id() == signoff)
                .cloned()
                .ok_or_else(|| Error::MissingSignoff(signoff.clone()))?;
            let workflow_path = workflow::path(repo_root, record.context().id(), &existing);
            let record_path = record.path().to_path_buf();
            repository
                .at_mut(path)?
                .context_mut()
                .signoffs_mut()
                .retain(|candidate| candidate.id() != signoff);
            remove_signoff_state(fs, repo_root, &repository, &record_path, &workflow_path)?;
            Ok(changed("removed Build signoff", repository.at(path)?))
        }
        SignoffAction::Repair { path, signoff } => repair(fs, repo_root, path, signoff),
        SignoffAction::Include(args) => signoff_include(&args.command, fs, repo_root),
    }
}

fn signoff_include(
    action: &SignoffIncludeAction,
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
) -> Result<String, Error> {
    match action {
        SignoffIncludeAction::List { path, signoff } => {
            let repository = Repository::load(fs, repo_root)?;
            let record = repository.at(path)?;
            let signoff = find_signoff(record, signoff)?;
            let own = display(repo_root, record.directory());
            let paths = signoff.included_paths().join(", ");
            Ok(format!(
                "# rapport context signoff include list\n\n- `context subtree` — {own}\n- `additional` — {}",
                or_none(&paths)
            ))
        }
        SignoffIncludeAction::Add {
            path,
            signoff,
            path_included,
        } => change_signoff_path(fs, repo_root, path, signoff, path_included, true),
        SignoffIncludeAction::Remove {
            path,
            signoff,
            path_included,
        } => change_signoff_path(fs, repo_root, path, signoff, path_included, false),
    }
}

fn change_signoff_path(
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
    path: &Utf8Path,
    signoff_id: &str,
    included: &Utf8Path,
    add: bool,
) -> Result<String, Error> {
    let mut repository = Repository::load(fs, repo_root)?;
    let record = repository.at(path)?;
    let normalized = repository.normalize_included_path(record.directory(), included, fs, add)?;
    let record_path = record.path().to_path_buf();
    let signoff = repository
        .at_mut(path)?
        .context_mut()
        .signoffs_mut()
        .iter_mut()
        .find(|candidate| candidate.id() == signoff_id)
        .ok_or_else(|| Error::MissingSignoff(signoff_id.to_owned()))?;
    if add {
        signoff.add_path(normalized)?;
    } else {
        signoff.remove_path(&normalized)?;
    }
    repository.validate()?;
    write_signoff_state(fs, repo_root, &repository, &record_path)?;
    Ok(changed("updated signoff paths", repository.at(path)?))
}

fn repair(
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
    path: &Utf8Path,
    signoff_id: &str,
) -> Result<String, Error> {
    let repository = Repository::load(fs, repo_root)?;
    let record = repository.at(path)?;
    let signoff = find_signoff(record, signoff_id)?;
    let workflow_path = workflow::path(repo_root, record.context().id(), signoff);
    let contents = workflow::render(
        record.context().id(),
        record.directory(),
        repo_root,
        signoff,
    );
    let shared_path = workflow::shared_path(repo_root);
    let snapshots = [snapshot(fs, &workflow_path)?, snapshot(fs, &shared_path)?];
    let result = fs
        .write_string(&shared_path, workflow::shared_contents())
        .map_err(|source| Error::Io {
            path: shared_path,
            source,
        })
        .and_then(|()| {
            fs.write_string(&workflow_path, contents)
                .map_err(|source| Error::Io {
                    path: workflow_path,
                    source,
                })
        });
    if result.is_err() {
        restore(fs, &snapshots);
    }
    result?;
    Ok(changed("repaired signoff workflow", record))
}

fn write_signoff_state(
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
    repository: &Repository,
    record_path: &Utf8Path,
) -> Result<(), Error> {
    let record = repository
        .records()
        .iter()
        .find(|record| record.path() == record_path)
        .ok_or(Error::InvalidPath)?;
    let shared_path = workflow::shared_path(repo_root);
    let mut snapshots = vec![snapshot(fs, record_path)?, snapshot(fs, &shared_path)?];
    for signoff in record.context().signoffs() {
        snapshots.push(snapshot(
            fs,
            &workflow::path(repo_root, record.context().id(), signoff),
        )?);
    }
    let result = (|| {
        repository.save(fs, record_path)?;
        fs.write_string(&shared_path, workflow::shared_contents())
            .map_err(|source| Error::Io {
                path: shared_path.clone(),
                source,
            })?;
        for signoff in record.context().signoffs() {
            let path = workflow::path(repo_root, record.context().id(), signoff);
            let contents = workflow::render(
                record.context().id(),
                record.directory(),
                repo_root,
                signoff,
            );
            fs.write_string(&path, contents)
                .map_err(|source| Error::Io { path, source })?;
        }
        Ok(())
    })();
    if result.is_err() {
        restore(fs, &snapshots);
    }
    result
}

fn remove_signoff_state(
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
    repository: &Repository,
    record_path: &Utf8Path,
    workflow_path: &Utf8Path,
) -> Result<(), Error> {
    let shared_path = workflow::shared_path(repo_root);
    let snapshots = [
        snapshot(fs, record_path)?,
        snapshot(fs, workflow_path)?,
        snapshot(fs, &shared_path)?,
    ];
    let result = (|| {
        repository.save(fs, record_path)?;
        if fs.is_file(workflow_path) {
            fs.remove_file(workflow_path).map_err(|source| Error::Io {
                path: workflow_path.to_path_buf(),
                source,
            })?;
        }
        if repository_has_signoffs(repository) {
            fs.write_string(&shared_path, workflow::shared_contents())
                .map_err(|source| Error::Io {
                    path: shared_path.clone(),
                    source,
                })?;
        } else if fs.is_file(&shared_path) {
            fs.remove_file(&shared_path).map_err(|source| Error::Io {
                path: shared_path.clone(),
                source,
            })?;
        }
        Ok(())
    })();
    if result.is_err() {
        restore(fs, &snapshots);
    }
    result
}

struct FileSnapshot {
    path: Utf8PathBuf,
    contents: Option<String>,
}

fn snapshot(fs: &impl FileSystem, path: &Utf8Path) -> Result<FileSnapshot, Error> {
    let contents = if fs.is_file(path) {
        Some(fs.read_to_string(path).map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?)
    } else {
        None
    };
    Ok(FileSnapshot {
        path: path.to_path_buf(),
        contents,
    })
}

fn restore(fs: &mut impl FileSystem, snapshots: &[FileSnapshot]) {
    for snapshot in snapshots.iter().rev() {
        if let Some(contents) = &snapshot.contents {
            let _ = fs.write_string(&snapshot.path, contents);
        } else if fs.is_file(&snapshot.path) {
            let _ = fs.remove_file(&snapshot.path);
        }
    }
}
