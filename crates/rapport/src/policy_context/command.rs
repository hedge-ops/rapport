//! Context policy command boundary.

use super::domain::{BoundaryOwnerUpdate, BuildSignoff, ContextId, Grade};
use super::repository::{Record, Repository};
use super::{Error, workflow};
use crate::context::{Clock, CommandContext};
use crate::shared_ruleset::{ExampleUpdate, NewRule, ReferenceUpdate, RuleUpdate, RulesetId};
use clap::{Args, Subcommand};
use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write as _};
use std::io::Write;
use std::process::ExitCode;
use std::str::FromStr;

#[derive(Args)]
#[command(arg_required_else_help = true)]
pub(crate) struct Cli {
    #[command(subcommand)]
    command: Action,
}

impl fmt::Debug for Cli {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextCli")
            .field("action", &self.command.name())
            .finish()
    }
}

#[derive(Subcommand)]
enum Action {
    /// Create a Context for a meaningful repository area.
    Init {
        path: Utf8PathBuf,
        #[arg(long)]
        purpose: String,
    },
    /// List Contexts at or below a path.
    List { path: Option<Utf8PathBuf> },
    /// Show effective or directly declared Context policy.
    Show {
        path: Utf8PathBuf,
        #[arg(long)]
        declared: bool,
    },
    /// Update a Context purpose.
    Update {
        path: Utf8PathBuf,
        #[arg(long)]
        purpose: String,
    },
    /// Remove a Context and report affected descendants.
    Remove { path: Utf8PathBuf },
    /// Manage Ownership entries.
    Ownership(OwnershipArgs),
    /// Manage Boundary entries.
    Boundary(BoundaryArgs),
    /// Manage the Context-owned Ruleset.
    Ruleset(RulesetArgs),
    /// Manage inherited Review quality.
    Review(ReviewArgs),
    /// Manage required Build signoffs.
    Signoff(SignoffArgs),
    /// Validate Context policy and generated workflows.
    Doctor { path: Option<Utf8PathBuf> },
}

impl Action {
    fn name(&self) -> &'static str {
        match self {
            Self::Init { .. } => "init",
            Self::List { .. } => "list",
            Self::Show { .. } => "show",
            Self::Update { .. } => "update",
            Self::Remove { .. } => "remove",
            Self::Ownership(_) => "ownership",
            Self::Boundary(_) => "boundary",
            Self::Ruleset(_) => "ruleset",
            Self::Review(_) => "review",
            Self::Signoff(_) => "signoff",
            Self::Doctor { .. } => "doctor",
        }
    }
}

#[derive(Args)]
#[command(arg_required_else_help = true)]
struct OwnershipArgs {
    #[command(subcommand)]
    command: OwnershipAction,
}

#[derive(Subcommand)]
enum OwnershipAction {
    List {
        path: Utf8PathBuf,
    },
    Add {
        path: Utf8PathBuf,
        #[arg(long)]
        text: String,
    },
    Update {
        path: Utf8PathBuf,
        #[arg(long)]
        id: String,
        #[arg(long)]
        text: String,
    },
    Remove {
        path: Utf8PathBuf,
        #[arg(long)]
        id: String,
    },
}

#[derive(Args)]
#[command(arg_required_else_help = true)]
struct BoundaryArgs {
    #[command(subcommand)]
    command: BoundaryAction,
}

#[derive(Subcommand)]
enum BoundaryAction {
    List {
        path: Utf8PathBuf,
    },
    Add {
        path: Utf8PathBuf,
        #[arg(long)]
        text: String,
        #[arg(long)]
        owner: Option<String>,
    },
    Update {
        path: Utf8PathBuf,
        #[arg(long)]
        id: String,
        #[arg(long)]
        text: Option<String>,
        #[arg(long, conflicts_with = "clear_owner")]
        owner: Option<String>,
        #[arg(long, conflicts_with = "owner")]
        clear_owner: bool,
    },
    Remove {
        path: Utf8PathBuf,
        #[arg(long)]
        id: String,
    },
}

#[derive(Args)]
#[command(arg_required_else_help = true)]
struct RulesetArgs {
    #[command(subcommand)]
    command: RulesetAction,
}

#[derive(Subcommand)]
enum RulesetAction {
    Compose(ComposeArgs),
    Rule(ContextRuleArgs),
}

#[derive(Args)]
#[command(arg_required_else_help = true)]
struct ComposeArgs {
    #[command(subcommand)]
    command: ComposeAction,
}

#[derive(Subcommand)]
enum ComposeAction {
    List {
        path: Utf8PathBuf,
    },
    Add {
        path: Utf8PathBuf,
        #[arg(long = "ruleset")]
        ruleset: String,
    },
    Remove {
        path: Utf8PathBuf,
        #[arg(long = "ruleset")]
        ruleset: String,
    },
}

#[derive(Args)]
#[command(arg_required_else_help = true)]
struct ContextRuleArgs {
    #[command(subcommand)]
    command: ContextRuleAction,
}

#[derive(Subcommand)]
enum ContextRuleAction {
    Add(RuleAddArgs),
    Update(RuleUpdateArgs),
    Remove {
        path: Utf8PathBuf,
        #[arg(long = "rule")]
        rule: String,
    },
}

#[derive(Args)]
struct RuleAddArgs {
    path: Utf8PathBuf,
    #[arg(long)]
    id: String,
    #[arg(long)]
    text: String,
    #[arg(long)]
    rationale: String,
    #[arg(long)]
    avoid_example: String,
    #[arg(long)]
    avoid_language: String,
    #[arg(long)]
    prefer_example: String,
    #[arg(long)]
    prefer_language: String,
    #[arg(long)]
    reference: Option<String>,
}

#[derive(Args)]
struct RuleUpdateArgs {
    path: Utf8PathBuf,
    #[arg(long = "rule")]
    rule: String,
    #[arg(long)]
    text: Option<String>,
    #[arg(long)]
    rationale: Option<String>,
    #[arg(long, requires = "avoid_language")]
    avoid_example: Option<String>,
    #[arg(long, requires = "avoid_example")]
    avoid_language: Option<String>,
    #[arg(long, requires = "prefer_language")]
    prefer_example: Option<String>,
    #[arg(long, requires = "prefer_example")]
    prefer_language: Option<String>,
    #[arg(long, conflicts_with = "clear_reference")]
    reference: Option<String>,
    #[arg(long, conflicts_with = "reference")]
    clear_reference: bool,
}

#[derive(Args)]
#[command(arg_required_else_help = true)]
struct ReviewArgs {
    #[command(subcommand)]
    command: ReviewAction,
}

#[derive(Subcommand)]
enum ReviewAction {
    Show {
        path: Utf8PathBuf,
    },
    Set {
        path: Utf8PathBuf,
        #[arg(long)]
        minimum_grade: String,
    },
    Clear {
        path: Utf8PathBuf,
    },
}

#[derive(Args)]
#[command(arg_required_else_help = true)]
struct SignoffArgs {
    #[command(subcommand)]
    command: SignoffAction,
}

#[derive(Subcommand)]
enum SignoffAction {
    List {
        path: Utf8PathBuf,
    },
    Add {
        path: Utf8PathBuf,
        #[arg(long)]
        target: String,
        #[arg(long, default_value_t = 0)]
        stage: u32,
        #[arg(long)]
        resource_group: Option<String>,
        #[arg(long = "include")]
        include: Vec<Utf8PathBuf>,
    },
    Remove {
        path: Utf8PathBuf,
        #[arg(long)]
        signoff: String,
    },
    Repair {
        path: Utf8PathBuf,
        #[arg(long)]
        signoff: String,
    },
    Include(SignoffIncludeArgs),
}

#[derive(Args)]
#[command(arg_required_else_help = true)]
struct SignoffIncludeArgs {
    #[command(subcommand)]
    command: SignoffIncludeAction,
}

#[derive(Subcommand)]
enum SignoffIncludeAction {
    List {
        path: Utf8PathBuf,
        #[arg(long)]
        signoff: String,
    },
    Add {
        path: Utf8PathBuf,
        #[arg(long)]
        signoff: String,
        #[arg(long = "path")]
        path_included: Utf8PathBuf,
    },
    Remove {
        path: Utf8PathBuf,
        #[arg(long)]
        signoff: String,
        #[arg(long = "path")]
        path_included: Utf8PathBuf,
    },
}

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
        Action::List { path } => list(fs, repo_root, path.as_deref().unwrap_or(Utf8Path::new("."))),
        Action::Show { path, declared } => show(fs, repo_root, path, *declared),
        Action::Update { path, purpose } => mutate(fs, repo_root, path, |record| {
            record.context_mut().set_purpose(purpose.clone())
        }),
        Action::Remove { path } => remove_context(fs, repo_root, path),
        Action::Ownership(args) => ownership(&args.command, fs, repo_root),
        Action::Boundary(args) => boundary(&args.command, fs, repo_root),
        Action::Ruleset(args) => ruleset(&args.command, fs, repo_root),
        Action::Review(args) => review(&args.command, fs, repo_root),
        Action::Signoff(args) => signoff(&args.command, fs, repo_root, runner),
        Action::Doctor { path } => doctor(
            fs,
            repo_root,
            path.as_deref().unwrap_or(Utf8Path::new(".")),
            runner,
        ),
    }
}

fn list(fs: &mut impl FileSystem, repo_root: &Utf8Path, path: &Utf8Path) -> Result<String, Error> {
    let repository = Repository::load(fs, repo_root)?;
    let records = repository.descendants(path)?;
    let lines = records
        .iter()
        .map(|record| {
            format!(
                "- `{}` — {} — {}",
                record.context().id(),
                record.context().purpose(),
                display(repo_root, record.directory())
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!("# rapport context list\n\n{}", or_none(&lines)))
}

fn show(
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
    path: &Utf8Path,
    declared: bool,
) -> Result<String, Error> {
    let repository = Repository::load(fs, repo_root)?;
    let records = if declared {
        vec![repository.at(path)?]
    } else {
        repository.effective(path)?
    };
    let grade = repository.effective_grade(path)?;
    let nearest = records
        .last()
        .ok_or_else(|| Error::MissingContext(path.to_path_buf()))?;
    let mut output = String::from("# rapport context show\n");
    for record in &records {
        render_context_record(&mut output, record, repo_root, nearest);
    }

    let shared = shared_attributions(&records, nearest, &repository)?;
    render_shared_rulesets(&mut output, &shared, &repository)?;
    render_required_signoffs(&mut output, declared, nearest, &repository, path, repo_root)?;
    let digest = format!("{:x}", Sha256::digest(output.as_bytes()));
    let _ = write!(
        output,
        "\n\n- `effective review minimum` — {grade}\n- `policy digest` — `{digest}`"
    );
    Ok(output)
}

fn render_context_record(
    output: &mut String,
    record: &Record,
    repo_root: &Utf8Path,
    nearest: &Record,
) {
    let scope = if record.context().id() == nearest.context().id() {
        "direct"
    } else {
        "inherited"
    };
    let _ = write!(
        output,
        "\n## `{}`\n\n- `path` — {}\n- `scope` — {scope}\n- `purpose` — {}\n- `embedded Ruleset` — `{}`\n- `declared review minimum` — {}\n",
        record.context().id(),
        display(repo_root, record.directory()),
        record.context().purpose(),
        record.context().ruleset().id(),
        record
            .context()
            .minimum_grade()
            .map_or_else(|| "inherited".to_owned(), |grade| grade.to_string())
    );
    output.push_str("\n### Ownership — Prefer Here\n\n");
    output.push_str(&or_none(
        &record
            .context()
            .ownership()
            .iter()
            .map(|entry| format!("- `{}` — {}", entry.id(), entry.text()))
            .collect::<Vec<_>>()
            .join("\n"),
    ));
    output.push_str("\n\n### Boundaries — Avoid Here\n\n");
    output.push_str(&or_none(
        &record
            .context()
            .boundaries()
            .iter()
            .map(|entry| {
                format!(
                    "- `{}` — {}{}",
                    entry.id(),
                    entry.text(),
                    entry
                        .owner()
                        .map_or_else(String::new, |owner| format!(" — owner `{owner}`"))
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
    ));
    output.push_str("\n\n### Context Rules\n\n");
    output.push_str(&or_none(
        &record
            .context()
            .ruleset()
            .rules()
            .map(|rule| format!("- `{}` — {}", rule.id(), rule.text()))
            .collect::<Vec<_>>()
            .join("\n"),
    ));
}

fn shared_attributions(
    records: &[&Record],
    nearest: &Record,
    repository: &Repository,
) -> Result<BTreeMap<RulesetId, BTreeSet<String>>, Error> {
    let mut shared = BTreeMap::<RulesetId, BTreeSet<String>>::new();
    for record in records {
        let scope = if record.context().id() == nearest.context().id() {
            "direct"
        } else {
            "inherited"
        };
        for id in record.context().ruleset().includes() {
            shared.entry(id.clone()).or_default().insert(format!(
                "{} ({scope}, direct composition)",
                record.context().id()
            ));
            for transitive in repository.shared().require(id)?.transitive() {
                shared
                    .entry(transitive.clone())
                    .or_default()
                    .insert(format!(
                        "{} ({scope}, transitive composition)",
                        record.context().id()
                    ));
            }
        }
    }
    Ok(shared)
}

fn render_shared_rulesets(
    output: &mut String,
    shared: &BTreeMap<RulesetId, BTreeSet<String>>,
    repository: &Repository,
) -> Result<(), Error> {
    output.push_str("\n\n## Effective Shared Rulesets\n\n");
    let shared_lines = shared
        .iter()
        .map(|(id, declarations)| {
            let summary = repository.shared().require(id)?;
            Ok(format!(
                "- `{}` — {} — source {} — {} Rules — digest `{}` — declared by {}",
                summary.id(),
                summary.purpose(),
                summary.source(),
                summary.rule_count(),
                summary.digest(),
                declarations.iter().cloned().collect::<Vec<_>>().join(", ")
            ))
        })
        .collect::<Result<Vec<_>, Error>>()?
        .join("\n");
    output.push_str(&or_none(&shared_lines));
    if !shared.is_empty() {
        output.push_str("\n\nUse `rapport ruleset show <RULESET_ID>` for complete shared Rules.");
    }
    Ok(())
}

fn render_required_signoffs(
    output: &mut String,
    declared: bool,
    nearest: &Record,
    repository: &Repository,
    path: &Utf8Path,
    repo_root: &Utf8Path,
) -> Result<(), Error> {
    output.push_str("\n\n## Required Build Signoffs\n\n");
    let signoff_lines = if declared {
        nearest
            .context()
            .signoffs()
            .iter()
            .map(|signoff| {
                format!(
                    "- `{}` — source `{}` — just {} — stage {} — resource {} — trigger {}",
                    signoff.id(),
                    nearest.context().id(),
                    signoff.target(),
                    signoff.stage(),
                    signoff.resource_group().unwrap_or("none"),
                    display(repo_root, nearest.directory())
                )
            })
            .collect::<Vec<_>>()
    } else {
        repository
            .applicable_signoffs(path)?
            .into_iter()
            .map(|matched| {
                format!(
                    "- `{}` — source `{}` — just {} — stage {} — resource {} — trigger {}",
                    matched.signoff.id(),
                    matched.record.context().id(),
                    matched.signoff.target(),
                    matched.signoff.stage(),
                    matched.signoff.resource_group().unwrap_or("none"),
                    matched.trigger
                )
            })
            .collect::<Vec<_>>()
    };
    output.push_str(&or_none(&signoff_lines.join("\n")));
    Ok(())
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

fn signoff(
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

fn doctor(
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
    path: &Utf8Path,
    runner: &dyn crate::CommandRunner,
) -> Result<String, Error> {
    let repository = Repository::load(fs, repo_root)?;
    repository.validate_included_path_existence(fs)?;
    let records = repository.descendants(path)?;
    let mut signoff_count = 0;
    if records
        .iter()
        .any(|record| !record.context().signoffs().is_empty())
    {
        workflow::validate_shared(fs, repo_root)?;
    }
    for record in &records {
        for signoff in record.context().signoffs() {
            workflow::validate_target(runner, record.directory(), signoff.target())?;
            workflow::validate_file(
                fs,
                repo_root,
                record.context().id(),
                record.directory(),
                signoff,
            )?;
            signoff_count += 1;
        }
    }
    Ok(format!(
        "# rapport context doctor\n\n- `status` — pass\n- `contexts` — {}\n- `signoffs` — {signoff_count}",
        records.len()
    ))
}

pub(crate) fn doctor_all(
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
    runner: &dyn crate::CommandRunner,
) -> Result<(), Error> {
    doctor(fs, repo_root, Utf8Path::new("."), runner).map(|_| ())
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

fn repository_has_signoffs(repository: &Repository) -> bool {
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

fn find_signoff<'record>(
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

fn changed(status: &str, record: &Record) -> String {
    format!(
        "# rapport context\n\n- `status` — {status}\n- `context` — {}\n- `path` — {}",
        record.context().id(),
        record.path()
    )
}

fn display(root: &Utf8Path, path: &Utf8Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string()
}
fn or_none(value: &str) -> String {
    if value.is_empty() {
        "none".to_owned()
    } else {
        value.to_owned()
    }
}
