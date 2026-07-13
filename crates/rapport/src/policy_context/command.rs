//! Context policy command boundary.
//!
//! This module dispatches parsed Context actions into repository mutations and
//! renders their user-facing outcomes.

use super::cli::*;
use super::domain::{BoundaryOwnerUpdate, BuildSignoff, ContextId, Grade};
use super::render::or_none;
use super::repository::{Record, Repository};
use super::{Error, workflow};
use crate::context::{Clock, CommandContext};
use crate::shared_ruleset::{ExampleUpdate, NewRule, ReferenceUpdate, RuleUpdate, RulesetId};
use rapport_files::{FileSystem, Utf8Path};
use std::io::Write;
use std::process::ExitCode;
use std::str::FromStr;

pub(crate) fn run<F, C, O, E>(cli: &Cli, context: &mut CommandContext<'_, F, C, O, E>) -> ExitCode
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let result = execute(&cli.command, context.fs, &context.repo_root, context.runner);
    match result {
        Ok(output) => {
            let _ = writeln!(context.out, "{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            let _ = writeln!(context.err, "# rapport context\n\n{error}");
            ExitCode::from(2)
        }
    }
}

fn execute(
    action: &Action,
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
    runner: &dyn crate::CommandRunner,
) -> Result<String, Error> {
    match action {
        Action::Init { path, purpose } => {
            let mut repository = Repository::load(fs, repo_root)?;
            let record = repository.init(fs, path, purpose.clone())?;
            Ok(changed("initialized", record))
        }
        Action::List { path } => {
            super::render::list(fs, repo_root, path.as_deref().unwrap_or(Utf8Path::new(".")))
        }
        Action::Show { path, declared } => super::render::show(fs, repo_root, path, *declared),
        Action::Update { path, purpose } => mutate(fs, repo_root, path, |record| {
            record.context_mut().set_purpose(purpose.clone())
        }),
        Action::Remove { path } => remove_context(fs, repo_root, path),
        Action::Ownership(args) => ownership(&args.command, fs, repo_root),
        Action::Boundary(args) => boundary(&args.command, fs, repo_root),
        Action::Ruleset(args) => ruleset(&args.command, fs, repo_root),
        Action::Review(args) => review(&args.command, fs, repo_root),
        Action::Signoff(args) => super::signoff::run(&args.command, fs, repo_root, runner),
        Action::Doctor { path } => super::doctor::run(
            fs,
            repo_root,
            path.as_deref().unwrap_or(Utf8Path::new(".")),
            runner,
        ),
    }
}

fn ownership(
    action: &OwnershipAction,
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
) -> Result<String, Error> {
    match action {
        OwnershipAction::List { path } => {
            let repository = Repository::load(fs, repo_root)?;
            let record = repository.at(path)?;
            Ok(format!(
                "# rapport context ownership list\n\n{}",
                or_none(
                    &record
                        .context()
                        .ownership()
                        .iter()
                        .map(|entry| format!("- `{}` — {}", entry.id(), entry.text()))
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            ))
        }
        OwnershipAction::Add { path, text } => mutate_with_detail(fs, repo_root, path, |record| {
            Ok((
                "ownership",
                record
                    .context_mut()
                    .add_ownership(text.clone())?
                    .id()
                    .to_owned(),
            ))
        }),
        OwnershipAction::Update { path, id, text } => mutate(fs, repo_root, path, |record| {
            record
                .context_mut()
                .ownership_mut(id)?
                .set_text(text.clone())
        }),
        OwnershipAction::Remove { path, id } => mutate(fs, repo_root, path, |record| {
            record.context_mut().remove_ownership(id)
        }),
    }
}

fn boundary(
    action: &BoundaryAction,
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
) -> Result<String, Error> {
    match action {
        BoundaryAction::List { path } => {
            let repository = Repository::load(fs, repo_root)?;
            let record = repository.at(path)?;
            Ok(format!(
                "# rapport context boundary list\n\n{}",
                or_none(
                    &record
                        .context()
                        .boundaries()
                        .iter()
                        .map(|entry| {
                            format!(
                                "- `{}` — {} — owner {}",
                                entry.id(),
                                entry.text(),
                                entry.owner().map_or("none", ContextId::as_str)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            ))
        }
        BoundaryAction::Add { path, text, owner } => {
            let owner = owner.as_ref().map(ContextId::parse).transpose()?;
            let repository = Repository::load(fs, repo_root)?;
            if let Some(owner) = &owner
                && !repository
                    .records()
                    .iter()
                    .any(|candidate| candidate.context().id() == owner)
            {
                return Err(Error::UnknownBoundaryOwner {
                    context: repository.at(path)?.context().id().to_string(),
                    owner: owner.to_string(),
                });
            }
            mutate_with_detail(fs, repo_root, path, |record| {
                Ok((
                    "boundary",
                    record
                        .context_mut()
                        .add_boundary(text.clone(), owner.clone())?
                        .id()
                        .to_owned(),
                ))
            })
        }
        BoundaryAction::Update {
            path,
            id,
            text,
            owner,
            clear_owner,
        } => {
            let owner = if *clear_owner {
                BoundaryOwnerUpdate::Clear
            } else if let Some(owner) = owner {
                BoundaryOwnerUpdate::Set(ContextId::parse(owner.clone())?)
            } else {
                BoundaryOwnerUpdate::Preserve
            };
            mutate(fs, repo_root, path, |record| {
                record
                    .context_mut()
                    .boundary_mut(id)?
                    .update(text.clone(), owner)
            })
        }
        BoundaryAction::Remove { path, id } => mutate(fs, repo_root, path, |record| {
            record.context_mut().remove_boundary(id)
        }),
    }
}

fn ruleset(
    action: &RulesetAction,
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
) -> Result<String, Error> {
    match action {
        RulesetAction::Compose(args) => compose(&args.command, fs, repo_root),
        RulesetAction::Rule(args) => context_rule(&args.command, fs, repo_root),
    }
}

fn compose(
    action: &ComposeAction,
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
) -> Result<String, Error> {
    match action {
        ComposeAction::List { path } => {
            let repository = Repository::load(fs, repo_root)?;
            let record = repository.at(path)?;
            let direct = record
                .context()
                .ruleset()
                .includes()
                .iter()
                .map(|id| format!("`{id}`"))
                .collect::<Vec<_>>()
                .join(", ");
            let mut transitive = Vec::new();
            for id in record.context().ruleset().includes() {
                let summary = repository.shared().require(id)?;
                transitive.extend(summary.transitive().iter().map(|id| format!("`{id}`")));
            }
            transitive.sort();
            transitive.dedup();
            Ok(format!(
                "# rapport context ruleset compose list\n\n- `direct` — {}\n- `transitive` — {}",
                or_none(&direct),
                or_none(&transitive.join(", "))
            ))
        }
        ComposeAction::Add { path, ruleset } => {
            let id = RulesetId::parse(ruleset.clone())?;
            Repository::load(fs, repo_root)?.shared().require(&id)?;
            mutate(fs, repo_root, path, |record| {
                record.context_mut().ruleset_mut().compose(id.clone());
                Ok(())
            })
        }
        ComposeAction::Remove { path, ruleset } => {
            let id = RulesetId::parse(ruleset.clone())?;
            mutate(fs, repo_root, path, |record| {
                if record.context_mut().ruleset_mut().uncompose(&id) {
                    Ok(())
                } else {
                    Err(Error::Ruleset(
                        crate::shared_ruleset::Error::UnknownRuleset(id.to_string()),
                    ))
                }
            })
        }
    }
}

fn context_rule(
    action: &ContextRuleAction,
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
) -> Result<String, Error> {
    match action {
        ContextRuleAction::Add(args) => mutate(fs, repo_root, &args.path, |record| {
            record.context_mut().add_rule(NewRule {
                id: args.id.clone(),
                text: args.text.clone(),
                rationale: args.rationale.clone(),
                avoid_example: args.avoid_example.clone(),
                avoid_language: args.avoid_language.clone(),
                prefer_example: args.prefer_example.clone(),
                prefer_language: args.prefer_language.clone(),
                reference: args.reference.clone(),
            })
        }),
        ContextRuleAction::Update(args) => {
            let reference = if args.clear_reference {
                ReferenceUpdate::Clear
            } else if let Some(reference) = &args.reference {
                ReferenceUpdate::Set(reference.clone())
            } else {
                ReferenceUpdate::Preserve
            };
            let update = RuleUpdate {
                text: args.text.clone(),
                rationale: args.rationale.clone(),
                avoid: args
                    .avoid_example
                    .as_ref()
                    .zip(args.avoid_language.as_ref())
                    .map(|(text, language)| ExampleUpdate {
                        text: text.clone(),
                        language: language.clone(),
                    }),
                prefer: args
                    .prefer_example
                    .as_ref()
                    .zip(args.prefer_language.as_ref())
                    .map(|(text, language)| ExampleUpdate {
                        text: text.clone(),
                        language: language.clone(),
                    }),
                reference,
            };
            mutate(fs, repo_root, &args.path, |record| {
                record.context_mut().update_rule(&args.rule, update)
            })
        }
        ContextRuleAction::Remove { path, rule } => mutate(fs, repo_root, path, |record| {
            record.context_mut().remove_rule(rule)
        }),
    }
}

fn review(
    action: &ReviewAction,
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
) -> Result<String, Error> {
    match action {
        ReviewAction::Show { path } => {
            let repository = Repository::load(fs, repo_root)?;
            let record = repository.at(path)?;
            Ok(format!(
                "# rapport context review show\n\n- `declared` — {}\n- `effective` — {}",
                record
                    .context()
                    .minimum_grade()
                    .map_or_else(|| "none".to_owned(), |grade| grade.to_string()),
                repository.effective_grade(path)?
            ))
        }
        ReviewAction::Set {
            path,
            minimum_grade,
        } => {
            let grade = Grade::from_str(minimum_grade)?;
            let mut repository = Repository::load(fs, repo_root)?;
            let record_path = repository.at(path)?.path().to_path_buf();
            let directory = repository.at(path)?.directory().to_path_buf();
            let inherited = repository.inherited_grade(&directory);
            if grade < inherited {
                return Err(Error::LowerReviewGrade {
                    requested: grade.to_string(),
                    inherited: inherited.to_string(),
                });
            }
            repository
                .at_mut(path)?
                .context_mut()
                .set_minimum_grade(Some(grade));
            repository.validate()?;
            repository.save(fs, &record_path)?;
            Ok(changed("updated Review grade", repository.at(path)?))
        }
        ReviewAction::Clear { path } => mutate(fs, repo_root, path, |record| {
            record.context_mut().set_minimum_grade(None);
            Ok(())
        }),
    }
}

fn remove_context(
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
    path: &Utf8Path,
) -> Result<String, Error> {
    let mut repository = Repository::load(fs, repo_root)?;
    let record = repository.at(path)?;
    let workflows = record
        .context()
        .signoffs()
        .iter()
        .map(|signoff| workflow::path(repo_root, record.context().id(), signoff))
        .collect::<Vec<_>>();
    let (removed, affected) = repository.remove(fs, path)?;
    for path in workflows {
        if fs.is_file(&path) {
            fs.remove_file(&path)
                .map_err(|source| Error::Io { path, source })?;
        }
    }
    let shared_path = workflow::shared_path(repo_root);
    if repository_has_signoffs(&repository) {
        fs.write_string(&shared_path, workflow::shared_contents())
            .map_err(|source| Error::Io {
                path: shared_path,
                source,
            })?;
    } else if fs.is_file(&shared_path) {
        fs.remove_file(&shared_path).map_err(|source| Error::Io {
            path: shared_path,
            source,
        })?;
    }
    Ok(format!(
        "# rapport context remove\n\n- `status` — removed\n- `context` — {}\n- `affected descendants` — {}",
        removed.context().id(),
        or_none(
            &affected
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )
    ))
}

pub(super) fn repository_has_signoffs(repository: &Repository) -> bool {
    repository
        .records()
        .iter()
        .any(|record| !record.context().signoffs().is_empty())
}

fn mutate(
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
    path: &Utf8Path,
    change: impl FnOnce(&mut Record) -> Result<(), Error>,
) -> Result<String, Error> {
    let mut repository = Repository::load(fs, repo_root)?;
    let record_path = repository.at(path)?.path().to_path_buf();
    change(repository.at_mut(path)?)?;
    repository.validate()?;
    repository.save(fs, &record_path)?;
    Ok(changed("updated", repository.at(path)?))
}

fn mutate_with_detail(
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
    path: &Utf8Path,
    change: impl FnOnce(&mut Record) -> Result<(&'static str, String), Error>,
) -> Result<String, Error> {
    let mut repository = Repository::load(fs, repo_root)?;
    let record_path = repository.at(path)?.path().to_path_buf();
    let (kind, id) = change(repository.at_mut(path)?)?;
    repository.validate()?;
    repository.save(fs, &record_path)?;
    Ok(format!(
        "{}\n- `{kind}` — `{id}`",
        changed("updated", repository.at(path)?)
    ))
}

pub(super) fn find_signoff<'record>(
    record: &'record Record,
    id: &str,
) -> Result<&'record BuildSignoff, Error> {
    record
        .context()
        .signoffs()
        .iter()
        .find(|candidate| candidate.id() == id)
        .ok_or_else(|| Error::MissingSignoff(id.to_owned()))
}

pub(super) fn changed(status: &str, record: &Record) -> String {
    format!(
        "# rapport context\n\n- `status` — {status}\n- `context` — {}\n- `path` — {}",
        record.context().id(),
        record.path()
    )
}
