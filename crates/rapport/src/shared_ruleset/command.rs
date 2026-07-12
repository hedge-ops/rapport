//! Shared Ruleset command boundary.

use super::Error;
use super::catalog::Catalog;
use super::domain::{ExampleUpdate, NewRule, ReferenceUpdate, Rule, RuleUpdate, Ruleset};
use super::repository::{Snapshot, Store, StoredRuleset};
use crate::context::{Clock, CommandContext};
use clap::{Args, Subcommand};
use rapport_files::FileSystem;
use std::fmt;
use std::io::Write;
use std::process::ExitCode;

#[derive(Args)]
#[command(arg_required_else_help = true)]
pub(crate) struct Cli {
    #[command(subcommand)]
    command: Action,
}

impl fmt::Debug for Cli {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RulesetCli")
            .field("action", &self.command.name())
            .finish()
    }
}

#[derive(Subcommand)]
enum Action {
    /// Inspect or install Rapport's versioned Ruleset catalog.
    Catalog(CatalogArgs),
    /// List installed catalog and repository-owned Rulesets.
    List,
    /// Show one repository-available Ruleset.
    Show(ShowArgs),
    /// Create a repository-owned Ruleset at its conventional path.
    Init(InitArgs),
    /// Change why a repository-owned Ruleset exists.
    Purpose(PurposeArgs),
    /// Remove an unused repository-owned Ruleset.
    Remove { id: String },
    /// Compose other Rulesets into a repository-owned Ruleset.
    Compose(ComposeArgs),
    /// Manage Rules owned by a repository Ruleset.
    Rule(RuleArgs),
}

impl Action {
    fn name(&self) -> &'static str {
        match self {
            Self::Catalog(_) => "catalog",
            Self::List => "list",
            Self::Show(_) => "show",
            Self::Init(_) => "init",
            Self::Purpose(_) => "purpose",
            Self::Remove { .. } => "remove",
            Self::Compose(_) => "compose",
            Self::Rule(_) => "rule",
        }
    }
}

#[derive(Args)]
#[command(arg_required_else_help = true)]
struct CatalogArgs {
    #[command(subcommand)]
    command: CatalogAction,
}

#[derive(Subcommand)]
enum CatalogAction {
    /// List catalog Rulesets.
    List,
    /// Show catalog composition, Rules, or one complete Rule.
    Show(ShowArgs),
    /// Install a catalog Ruleset and its dependency closure.
    Install { id: String },
    /// Update an installed catalog Ruleset and its dependency closure.
    Update { id: String },
}

#[derive(Args)]
struct ShowArgs {
    id: String,
    /// Show one complete direct or composed Rule.
    #[arg(long)]
    rule: Option<String>,
}

#[derive(Args)]
struct InitArgs {
    id: String,
    #[arg(long)]
    purpose: String,
}

#[derive(Args)]
#[command(arg_required_else_help = true)]
struct PurposeArgs {
    #[command(subcommand)]
    command: PurposeAction,
}

#[derive(Subcommand)]
enum PurposeAction {
    /// Set the Ruleset purpose.
    Set {
        id: String,
        #[arg(long)]
        purpose: String,
    },
}

#[derive(Args)]
#[command(arg_required_else_help = true)]
struct ComposeArgs {
    #[command(subcommand)]
    command: ComposeAction,
}

#[derive(Subcommand)]
enum ComposeAction {
    /// Show direct and transitive composition.
    List { id: String },
    /// Compose another Ruleset directly.
    Add {
        id: String,
        #[arg(long = "ruleset")]
        included: String,
    },
    /// Remove a directly composed Ruleset.
    Remove {
        id: String,
        #[arg(long = "ruleset")]
        included: String,
    },
}

#[derive(Args)]
#[command(arg_required_else_help = true)]
struct RuleArgs {
    #[command(subcommand)]
    command: RuleAction,
}

#[derive(Subcommand)]
enum RuleAction {
    /// Add one complete Rule.
    Add(RuleAddArgs),
    /// Update supplied fields of one Rule.
    Update(RuleUpdateArgs),
    /// Remove one Rule.
    Remove {
        id: String,
        #[arg(long = "rule")]
        rule: String,
    },
}

#[derive(Args)]
struct RuleAddArgs {
    id: String,
    #[arg(long = "id")]
    id_rule: String,
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
    id: String,
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

pub(crate) fn run<F, C, O, E>(cli: &Cli, context: &mut CommandContext<'_, F, C, O, E>) -> ExitCode
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    match execute(&cli.command, context.fs, &context.repo_root) {
        Ok(output) => {
            let _ = writeln!(context.out, "{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            let _ = writeln!(context.err, "# rapport ruleset\n\n{error}");
            ExitCode::from(2)
        }
    }
}

fn execute(
    action: &Action,
    fs: &mut impl FileSystem,
    repo_root: &rapport_files::Utf8Path,
) -> Result<String, Error> {
    let catalog = Catalog::load()?;
    match action {
        Action::Catalog(args) => execute_catalog(&args.command, fs, repo_root, &catalog),
        Action::List => {
            let snapshot = Store::new(fs, repo_root, &catalog).load()?;
            Ok(render_list(&snapshot, repo_root))
        }
        Action::Show(args) => {
            let snapshot = Store::new(fs, repo_root, &catalog).load()?;
            render_repository_show(&snapshot, args)
        }
        Action::Init(args) => {
            let stored = Store::new(fs, repo_root, &catalog).init(&args.id, &args.purpose)?;
            Ok(render_changed("initialized", &stored, repo_root))
        }
        Action::Purpose(args) => match &args.command {
            PurposeAction::Set { id, purpose } => {
                let stored = Store::new(fs, repo_root, &catalog).set_purpose(id, purpose)?;
                Ok(render_changed("updated purpose", &stored, repo_root))
            }
        },
        Action::Remove { id } => {
            let stored = Store::new(fs, repo_root, &catalog).remove(id)?;
            Ok(render_changed("removed", &stored, repo_root))
        }
        Action::Compose(args) => execute_compose(&args.command, fs, repo_root, &catalog),
        Action::Rule(args) => execute_rule(&args.command, fs, repo_root, &catalog),
    }
}

fn execute_catalog(
    action: &CatalogAction,
    fs: &mut impl FileSystem,
    repo_root: &rapport_files::Utf8Path,
    catalog: &Catalog,
) -> Result<String, Error> {
    match action {
        CatalogAction::List => {
            let mut lines = vec!["# rapport ruleset catalog list".to_owned(), String::new()];
            lines.extend(catalog.entries().map(|entry| {
                let ruleset = entry.ruleset();
                format!(
                    "- `{}` {} — {}",
                    ruleset.id(),
                    ruleset.catalog_version().unwrap_or("unversioned"),
                    ruleset.purpose()
                )
            }));
            Ok(lines.join("\n"))
        }
        CatalogAction::Show(args) => render_catalog_show(catalog, args),
        CatalogAction::Install { id } => {
            let installed = catalog.install(fs, repo_root, id)?;
            Ok(render_catalog_change("installed", id, &installed))
        }
        CatalogAction::Update { id } => {
            let updated = catalog.update(fs, repo_root, id)?;
            Ok(render_catalog_change("updated", id, &updated))
        }
    }
}

fn execute_compose(
    action: &ComposeAction,
    fs: &mut impl FileSystem,
    repo_root: &rapport_files::Utf8Path,
    catalog: &Catalog,
) -> Result<String, Error> {
    match action {
        ComposeAction::List { id } => {
            let snapshot = Store::new(fs, repo_root, catalog).load()?;
            let stored = snapshot.get(id)?;
            let closure = snapshot.closure(stored.ruleset().id())?;
            let direct = stored
                .ruleset()
                .includes()
                .iter()
                .map(|id| format!("`{id}`"))
                .collect::<Vec<_>>()
                .join(", ");
            let transitive = closure
                .iter()
                .filter(|candidate| candidate.ruleset().id() != stored.ruleset().id())
                .map(|candidate| format!("`{}`", candidate.ruleset().id()))
                .collect::<Vec<_>>()
                .join(", ");
            Ok(format!(
                "# rapport ruleset compose list\n\n- `ruleset` — {}\n- `direct` — {}\n- `transitive` — {}",
                stored.ruleset().id(),
                or_none(&direct),
                or_none(&transitive)
            ))
        }
        ComposeAction::Add { id, included } => {
            let stored = Store::new(fs, repo_root, catalog).compose(id, included)?;
            Ok(render_changed("composed", &stored, repo_root))
        }
        ComposeAction::Remove { id, included } => {
            let stored = Store::new(fs, repo_root, catalog).uncompose(id, included)?;
            Ok(render_changed("removed composition", &stored, repo_root))
        }
    }
}

fn execute_rule(
    action: &RuleAction,
    fs: &mut impl FileSystem,
    repo_root: &rapport_files::Utf8Path,
    catalog: &Catalog,
) -> Result<String, Error> {
    match action {
        RuleAction::Add(args) => {
            let stored = Store::new(fs, repo_root, catalog).add_rule(
                &args.id,
                NewRule {
                    id: args.id_rule.clone(),
                    text: args.text.clone(),
                    rationale: args.rationale.clone(),
                    avoid_example: args.avoid_example.clone(),
                    avoid_language: args.avoid_language.clone(),
                    prefer_example: args.prefer_example.clone(),
                    prefer_language: args.prefer_language.clone(),
                    reference: args.reference.clone(),
                },
            )?;
            Ok(render_changed("added Rule", &stored, repo_root))
        }
        RuleAction::Update(args) => {
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
                        language: language.clone(),
                        text: text.clone(),
                    }),
                prefer: args
                    .prefer_example
                    .as_ref()
                    .zip(args.prefer_language.as_ref())
                    .map(|(text, language)| ExampleUpdate {
                        language: language.clone(),
                        text: text.clone(),
                    }),
                reference,
            };
            let stored =
                Store::new(fs, repo_root, catalog).update_rule(&args.id, &args.rule, update)?;
            Ok(render_changed("updated Rule", &stored, repo_root))
        }
        RuleAction::Remove { id, rule } => {
            let stored = Store::new(fs, repo_root, catalog).remove_rule(id, rule)?;
            Ok(render_changed("removed Rule", &stored, repo_root))
        }
    }
}

fn render_catalog_show(catalog: &Catalog, args: &ShowArgs) -> Result<String, Error> {
    let entry = catalog.get(&args.id)?;
    let ruleset = entry.ruleset();
    if let Some(rule_id) = &args.rule {
        let (owner, rule) = catalog
            .resolved_rules(ruleset.id())?
            .into_iter()
            .find(|(_, rule)| rule.id().as_str() == rule_id)
            .ok_or_else(|| Error::UnknownRule(rule_id.clone()))?;
        return Ok(render_rule(rule, owner.as_str()));
    }
    let dependencies = catalog
        .closure(ruleset.id())?
        .into_iter()
        .filter(|candidate| candidate.ruleset().id() != ruleset.id())
        .map(|candidate| format!("`{}`", candidate.ruleset().id()))
        .collect::<Vec<_>>()
        .join(", ");
    let summaries = catalog
        .resolved_rules(ruleset.id())?
        .into_iter()
        .map(|(owner, rule)| format!("- `{}` ({owner}) — {}", rule.id(), rule.text()))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(render_ruleset(
        ruleset,
        "catalog",
        &dependencies,
        &summaries,
    ))
}

fn render_repository_show(snapshot: &Snapshot, args: &ShowArgs) -> Result<String, Error> {
    let stored = snapshot.get(&args.id)?;
    if let Some(rule_id) = &args.rule {
        let (owner, rule) = snapshot
            .resolved_rules(stored.ruleset().id())?
            .into_iter()
            .find(|(_, rule)| rule.id().as_str() == rule_id)
            .ok_or_else(|| Error::UnknownRule(rule_id.clone()))?;
        return Ok(render_rule(rule, owner.as_str()));
    }
    let dependencies = snapshot
        .closure(stored.ruleset().id())?
        .into_iter()
        .filter(|candidate| candidate.ruleset().id() != stored.ruleset().id())
        .map(|candidate| format!("`{}`", candidate.ruleset().id()))
        .collect::<Vec<_>>()
        .join(", ");
    let summaries = snapshot
        .resolved_rules(stored.ruleset().id())?
        .into_iter()
        .map(|(owner, rule)| format!("- `{}` ({owner}) — {}", rule.id(), rule.text()))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(render_ruleset(
        stored.ruleset(),
        &stored.source().to_string(),
        &dependencies,
        &summaries,
    ))
}

fn render_ruleset(ruleset: &Ruleset, source: &str, dependencies: &str, rules: &str) -> String {
    format!(
        "# rapport ruleset show\n\n- `id` — {}\n- `purpose` — {}\n- `source` — {source}\n- `version` — {}\n- `composition` — {}\n- `dependencies` — {}\n\n## Rules\n\n{}",
        ruleset.id(),
        ruleset.purpose(),
        ruleset.catalog_version().unwrap_or("repository schema 1"),
        or_none(
            &ruleset
                .includes()
                .iter()
                .map(|id| format!("`{id}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        or_none(dependencies),
        or_none(rules)
    )
}

fn render_rule(rule: &Rule, owner: &str) -> String {
    let reference = rule
        .reference()
        .map_or_else(|| "none".to_owned(), super::domain::Reference::markdown);
    format!(
        "# rapport ruleset show --rule\n\n- `id` — {}\n- `owning ruleset` — {owner}\n- `reference` — {reference}\n\n{}\n\n## Rationale\n\n{}\n\n## Avoid\n\n```{}\n{}\n```\n\n## Prefer\n\n```{}\n{}\n```",
        rule.id(),
        rule.text(),
        rule.rationale(),
        rule.avoid().language().as_str(),
        rule.avoid().text(),
        rule.prefer().language().as_str(),
        rule.prefer().text()
    )
}

fn render_list(snapshot: &Snapshot, repo_root: &rapport_files::Utf8Path) -> String {
    let mut lines = vec!["# rapport ruleset list".to_owned(), String::new()];
    lines.extend(snapshot.entries().map(|stored| {
        format!(
            "- `{}` — {} — {} — {}",
            stored.ruleset().id(),
            stored.ruleset().purpose(),
            stored.source(),
            display_path(repo_root, stored.path())
        )
    }));
    lines.join("\n")
}

fn render_catalog_change(
    action: &str,
    selected: &str,
    changed: &[super::domain::RulesetId],
) -> String {
    format!(
        "# rapport ruleset catalog\n\n- `status` — {action}\n- `selected` — {selected}\n- `rulesets` — {}",
        changed
            .iter()
            .map(|id| format!("`{id}`"))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn render_changed(
    action: &str,
    stored: &StoredRuleset,
    repo_root: &rapport_files::Utf8Path,
) -> String {
    format!(
        "# rapport ruleset\n\n- `status` — {action}\n- `ruleset` — {}\n- `path` — {}",
        stored.ruleset().id(),
        display_path(repo_root, stored.path())
    )
}

fn display_path(root: &rapport_files::Utf8Path, path: &rapport_files::Utf8Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string()
}

fn or_none(value: &str) -> &str {
    if value.is_empty() { "none" } else { value }
}

#[cfg(test)]
mod tests {
    use super::{Action, CatalogAction, CatalogArgs, Cli, execute};
    use claims::assert_ok;
    use rapport_files::{FileSystem, InMemoryFileSystem, Utf8Path};

    /// Catalog installation makes the aggregate and every dependency available to repository commands.
    #[test]
    fn execute_should_install_and_list_a_catalog_dependency_closure() {
        let mut fs = InMemoryFileSystem::default();
        let cli = Cli {
            command: Action::Catalog(CatalogArgs {
                command: CatalogAction::Install {
                    id: "RUST_CRATE".to_owned(),
                },
            }),
        };
        let output = assert_ok!(execute(&cli.command, &mut fs, Utf8Path::new("/repo")));
        let list = assert_ok!(execute(&Action::List, &mut fs, Utf8Path::new("/repo")));

        assert!(
            output.contains("`RUST_CRATE`"),
            "expecting install output to name the selected aggregate"
        );
        assert!(
            list.contains("`RUST_CODING`"),
            "expecting repository listing to include installed dependencies"
        );
        assert!(fs.is_file("/repo/.rapport/rules.lock"));
    }
}
