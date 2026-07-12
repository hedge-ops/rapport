use crate::{policy_context, shared_ruleset};
use clap::{Args, Parser, Subcommand, ValueEnum};
use rapport_files::Utf8PathBuf;

const ROOT_ABOUT: &str = "repository rapport for human-directed agent work";
const ROOT_LONG_ABOUT: &str = "\
Rapport keeps human-directed agent work grounded in repository-owned rules, \
build conventions, Git/GitHub integration, and local state.";
const ROOT_AFTER_HELP: &str = "\
First loop:
  prime -> doctor -> work -> context -> build -> review -> integrate -> work complete

Rapport coordinates repository workflow; it does not replace Just or implement release/deploy behavior.";
const CONTEXT_LONG_ABOUT: &str = "\
Folder context answers what a project area is about before agents plan, code, test, build, \
review, or integrate. Ownership records what belongs in the folder. Boundaries describe \
neighboring responsibilities and where work should move instead. Signoffs declare the \
SHA-bound proof a pull request needs. Rules are numbered, reviewable benchmarks for judging local work.";
const CONTEXT_AFTER_HELP: &str = "\
`context.toml` is Rapport-owned structured project state. Edit it through \
`rapport context` commands so formatting, required fields, rule ids, includes, and \
schema evolution stay consistent.";

#[derive(Debug, Parser)]
#[command(
    name = "rapport",
    about = ROOT_ABOUT,
    long_about = ROOT_LONG_ABOUT,
    after_help = ROOT_AFTER_HELP,
    after_long_help = ROOT_AFTER_HELP,
    arg_required_else_help = true,
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Show how agents should use Rapport in this project.
    Prime,
    /// Check repository prerequisites for Rapport workflow.
    Doctor,
    /// Record Rapport usage in repository agent instructions.
    Init,
    /// Manage repository-owned standalone and embedded rulesets.
    #[command(hide = true)]
    Rules(RulesArgs),
    /// Define and compose shared repository standards.
    Ruleset(shared_ruleset::Cli),
    /// Manage active local work state.
    Work(WorkArgs),
    /// Manage folder-local structured project context.
    #[command(
        about = "Manage folder-local structured project context.",
        long_about = CONTEXT_LONG_ABOUT,
        after_help = CONTEXT_AFTER_HELP,
        after_long_help = CONTEXT_AFTER_HELP
    )]
    Context(policy_context::Cli),
    /// Validate active work with existing repository Just conventions.
    Build(BuildArgs),
    /// Request or record an independent adversarial review of active work.
    Review(ReviewArgs),
    /// Turn validated local work into Git/GitHub integration state.
    Integrate(IntegrateArgs),
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct RulesArgs {
    #[command(subcommand)]
    pub command: RulesCommand,
}

#[derive(Debug, Subcommand)]
pub enum RulesCommand {
    /// List Michael's built-in versioned rules packs.
    Catalog,
    /// Install a built-in rules pack into .rapport/rules.
    Add { pack: String },
    /// List every discovered repository ruleset.
    List,
    /// Show one ruleset and its declared rules.
    Show { id: String },
    /// Create a standalone ruleset under .rapport/rules.
    Init(RulesInitArgs),
    /// Edit ruleset includes.
    Include(RulesIncludeArgs),
    /// Edit rules declared by a ruleset.
    Rule(RulesRuleArgs),
}

#[derive(Debug, Args)]
pub struct RulesInitArgs {
    /// Organizational path below .rapport/rules, with or without .toml.
    pub path: Utf8PathBuf,
    /// Stable repository-unique ruleset id.
    #[arg(long)]
    pub id: String,
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct RulesIncludeArgs {
    #[command(subcommand)]
    pub command: RulesIncludeCommand,
}

#[derive(Debug, Subcommand)]
pub enum RulesIncludeCommand {
    Add { ruleset: String, included: String },
    Remove { ruleset: String, included: String },
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct RulesRuleArgs {
    #[command(subcommand)]
    pub command: RulesRuleCommand,
}

#[derive(Debug, Subcommand)]
pub enum RulesRuleCommand {
    Add(RulesRuleAddArgs),
    Update(RulesRuleUpdateArgs),
    Remove {
        ruleset: String,
        id: String,
    },
    /// Manage typed provenance references for a rule.
    Reference(RulesReferenceArgs),
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct RulesReferenceArgs {
    #[command(subcommand)]
    pub command: RulesReferenceCommand,
}

#[derive(Debug, Subcommand)]
pub enum RulesReferenceCommand {
    List {
        ruleset: String,
        id: String,
    },
    Add(RulesReferenceAddArgs),
    Remove {
        ruleset: String,
        id: String,
        target: String,
    },
}

#[derive(Debug, Args)]
pub struct RulesReferenceAddArgs {
    pub ruleset: String,
    pub id: String,
    #[arg(
        long,
        conflicts_with = "external",
        required_unless_present = "external"
    )]
    pub repository: Option<String>,
    #[arg(
        long,
        conflicts_with = "repository",
        required_unless_present = "repository"
    )]
    pub external: Option<String>,
    #[arg(long)]
    pub label: Option<String>,
}

#[derive(Debug, Args)]
pub struct RulesRuleAddArgs {
    pub ruleset: String,
    #[arg(long)]
    pub id: String,
    #[arg(long)]
    pub text: String,
    #[arg(long)]
    pub rationale: Option<String>,
    #[arg(long)]
    pub avoid_language: String,
    #[arg(long)]
    pub avoid: String,
    #[arg(long)]
    pub prefer_language: String,
    #[arg(long)]
    pub prefer: String,
    #[arg(long = "reference")]
    pub references: Vec<String>,
}

#[derive(Debug, Args)]
pub struct RulesRuleUpdateArgs {
    pub ruleset: String,
    pub id: String,
    #[arg(long)]
    pub text: Option<String>,
    #[arg(long)]
    pub rationale: Option<String>,
    /// Remove the existing rationale.
    #[arg(long, conflicts_with = "rationale")]
    pub clear_rationale: bool,
    #[arg(long = "reference")]
    pub references: Vec<String>,
    /// Remove all existing references.
    #[arg(long, conflicts_with = "references")]
    pub clear_references: bool,
    #[arg(long, requires = "avoid")]
    pub avoid_language: Option<String>,
    #[arg(long, requires = "avoid_language")]
    pub avoid: Option<String>,
    #[arg(long, requires = "prefer")]
    pub prefer_language: Option<String>,
    #[arg(long, requires = "prefer_language")]
    pub prefer: Option<String>,
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct ContextArgs {
    #[command(subcommand)]
    pub command: ContextCommand,
}

#[derive(Debug, Subcommand)]
pub enum ContextCommand {
    /// Show effective purpose, ownership, boundaries, signoffs, and benchmarks.
    Show {
        /// Folder or file path to inspect. Defaults to the current directory.
        path: Option<Utf8PathBuf>,
    },
    /// Create a Rapport-owned context.toml for a folder.
    Init(ContextInitArgs),
    /// Edit the folder purpose.
    Purpose(ContextPurposeArgs),
    /// Edit ownership and boundary statements.
    Ownership(ContextOwnershipArgs),
    /// Edit reusable rule includes and inline benchmarks.
    Rule(ContextRuleArgs),
    /// Manage the context's embedded ruleset identity.
    Ruleset(ContextRulesetArgs),
    /// Manage signoffs and their generated GitHub request workflows.
    Signoff(ContextSignoffArgs),
    /// Validate applicable context.toml files, signoff workflows, and rule includes.
    Doctor {
        /// Folder or file path to inspect. Defaults to the current directory.
        path: Option<Utf8PathBuf>,
    },
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct ContextSignoffArgs {
    #[command(subcommand)]
    pub command: ContextSignoffCommand,
}

#[derive(Debug, Subcommand)]
pub enum ContextSignoffCommand {
    /// Declare a signoff and generate its GitHub request workflow.
    Add {
        /// Folder whose context.toml owns the signoff.
        path: Utf8PathBuf,
        /// Signoff operation kind.
        kind: SignoffKindArg,
        /// Kebab-case build target, such as ci. Omit for review.
        target: Option<String>,
        /// Passing grade threshold for review signoffs. Defaults to A-.
        #[arg(long)]
        minimum_grade: Option<String>,
    },
    /// Remove a signoff and its generated GitHub request workflow.
    Remove {
        /// Folder whose context.toml owns the signoff.
        path: Utf8PathBuf,
        /// Signoff operation kind.
        kind: SignoffKindArg,
        /// Existing build target. Omit for review.
        target: Option<String>,
    },
    /// Rewrite the exact Rapport-owned workflows for a declared signoff.
    Repair {
        /// Folder whose context.toml owns the signoff.
        path: Utf8PathBuf,
        /// Signoff operation kind.
        kind: SignoffKindArg,
        /// Existing build target. Omit for review.
        target: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SignoffKindArg {
    Build,
    Review,
}

#[derive(Debug, Args)]
pub struct ContextInitArgs {
    /// Folder that will own the new context.toml.
    pub path: Utf8PathBuf,
    /// Plain-language purpose for this project area.
    #[arg(long)]
    pub purpose: String,
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct ContextPurposeArgs {
    #[command(subcommand)]
    pub command: ContextPurposeCommand,
}

#[derive(Debug, Subcommand)]
pub enum ContextPurposeCommand {
    /// Replace the purpose for a folder context.
    Set(ContextPurposeSetArgs),
}

#[derive(Debug, Args)]
pub struct ContextPurposeSetArgs {
    /// Folder whose context.toml should be updated.
    pub path: Utf8PathBuf,
    /// Plain-language purpose for this project area.
    pub purpose: String,
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct ContextOwnershipArgs {
    #[command(subcommand)]
    pub command: ContextOwnershipCommand,
}

#[derive(Debug, Subcommand)]
pub enum ContextOwnershipCommand {
    /// Edit statements describing what belongs in this folder.
    Owns(ContextOwnershipItemArgs),
    /// Edit boundary statements that point work toward neighboring owners.
    Boundary(ContextOwnershipItemArgs),
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct ContextOwnershipItemArgs {
    #[command(subcommand)]
    pub command: ContextListCommand,
}

#[derive(Debug, Subcommand)]
pub enum ContextListCommand {
    /// Add a citable statement.
    Add {
        /// Folder whose context.toml should be updated.
        path: Utf8PathBuf,
        /// Statement text.
        value: String,
    },
    /// Remove a citable statement.
    Remove {
        /// Folder whose context.toml should be updated.
        path: Utf8PathBuf,
        /// Statement text.
        value: String,
    },
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct ContextRuleArgs {
    #[command(subcommand)]
    pub command: ContextRuleCommand,
}

#[derive(Debug, Subcommand)]
pub enum ContextRuleCommand {
    /// Edit rule library includes for a folder context.
    Include(ContextRuleIncludeArgs),
    /// Add an inline numbered benchmark.
    Add(ContextRuleAddArgs),
    /// Update an inline numbered benchmark.
    Update(ContextRuleUpdateArgs),
    /// Remove an inline numbered benchmark.
    Remove {
        /// Folder whose context.toml should be updated.
        path: Utf8PathBuf,
        /// Rule id to remove.
        id: String,
    },
    /// Manage typed provenance references for a rule.
    Reference(ContextReferenceArgs),
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct ContextReferenceArgs {
    #[command(subcommand)]
    pub command: ContextReferenceCommand,
}

#[derive(Debug, Subcommand)]
pub enum ContextReferenceCommand {
    List {
        path: Utf8PathBuf,
        id: String,
    },
    Add(ContextReferenceAddArgs),
    Remove {
        path: Utf8PathBuf,
        id: String,
        target: String,
    },
}

#[derive(Debug, Args)]
pub struct ContextReferenceAddArgs {
    pub path: Utf8PathBuf,
    pub id: String,
    #[arg(
        long,
        conflicts_with = "external",
        required_unless_present = "external"
    )]
    pub repository: Option<String>,
    #[arg(
        long,
        conflicts_with = "repository",
        required_unless_present = "repository"
    )]
    pub external: Option<String>,
    #[arg(long)]
    pub label: Option<String>,
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct ContextRulesetArgs {
    #[command(subcommand)]
    pub command: ContextRulesetCommand,
}

#[derive(Debug, Subcommand)]
pub enum ContextRulesetCommand {
    Id(ContextRulesetIdArgs),
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct ContextRulesetIdArgs {
    #[command(subcommand)]
    pub command: ContextRulesetIdCommand,
}

#[derive(Debug, Subcommand)]
pub enum ContextRulesetIdCommand {
    Set { path: Utf8PathBuf, id: String },
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct ContextRuleIncludeArgs {
    #[command(subcommand)]
    pub command: ContextListCommand,
}

#[derive(Debug, Args)]
pub struct ContextRuleAddArgs {
    /// Folder whose context.toml should be updated.
    pub path: Utf8PathBuf,
    /// Stable, unique, citable rule id.
    #[arg(long)]
    pub id: String,
    /// The benchmark itself.
    #[arg(long)]
    pub text: String,
    /// Why the benchmark exists.
    #[arg(long)]
    pub rationale: Option<String>,
    #[arg(long)]
    pub avoid_language: String,
    #[arg(long)]
    pub avoid: String,
    #[arg(long)]
    pub prefer_language: String,
    #[arg(long)]
    pub prefer: String,
    /// Provenance such as docs, ADRs, issues, or external references.
    #[arg(long = "reference")]
    pub references: Vec<String>,
}

#[derive(Debug, Args)]
pub struct ContextRuleUpdateArgs {
    /// Folder whose context.toml should be updated.
    pub path: Utf8PathBuf,
    /// Rule id to update.
    pub id: String,
    /// Replacement benchmark text.
    #[arg(long)]
    pub text: String,
    /// Replacement rationale. Omitted values preserve the existing rationale.
    #[arg(long)]
    pub rationale: Option<String>,
    /// Remove the existing rationale.
    #[arg(long, conflicts_with = "rationale")]
    pub clear_rationale: bool,
    /// Replacement references. Omitted values preserve existing references.
    #[arg(long = "reference")]
    pub references: Vec<String>,
    /// Remove all existing references.
    #[arg(long, conflicts_with = "references")]
    pub clear_references: bool,
    #[arg(long, requires = "avoid")]
    pub avoid_language: Option<String>,
    #[arg(long, requires = "avoid_language")]
    pub avoid: Option<String>,
    #[arg(long, requires = "prefer")]
    pub prefer_language: Option<String>,
    #[arg(long, requires = "prefer_language")]
    pub prefer: Option<String>,
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct WorkArgs {
    #[command(subcommand)]
    pub command: WorkCommand,
}

#[derive(Debug, Subcommand)]
pub enum WorkCommand {
    /// Create the active local work session.
    Start(WorkStartArgs),
    /// Show active local work state.
    Status,
    /// Archive and clear completed local work.
    Complete(WorkCompleteArgs),
    /// Add facts to active local work.
    Add(WorkAddArgs),
    /// Inspect repository-owned rules for active work.
    Rules(WorkRulesArgs),
    /// Manage review actions attached to active work.
    Task(WorkTaskArgs),
}

#[derive(Debug, Args)]
pub struct WorkStartArgs {
    /// Human title for the work.
    #[arg(long)]
    pub title: String,
    /// Durable ticket or issue identifier.
    #[arg(long)]
    pub ticket: Option<String>,
    /// Durable plan identifier.
    #[arg(long)]
    pub plan: Option<String>,
    /// Desired outcome for the work.
    #[arg(long)]
    pub objective: Option<String>,
    /// Path to include in the active work session.
    #[arg(long = "path", value_name = "PATH")]
    pub paths: Vec<Utf8PathBuf>,
}

#[derive(Debug, Args)]
pub struct WorkCompleteArgs {
    /// Human summary for the completed work.
    #[arg(long)]
    pub summary: String,
    /// Complete local-only work that has not been integrated.
    #[arg(long)]
    pub without_integrate: bool,
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct WorkAddArgs {
    #[command(subcommand)]
    pub command: WorkAddCommand,
}

#[derive(Debug, Subcommand)]
pub enum WorkAddCommand {
    /// Add a path to active work.
    Path {
        /// Path to include in active work.
        path: Utf8PathBuf,
    },
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct WorkRulesArgs {
    #[command(subcommand)]
    pub command: WorkRulesCommand,
}

#[derive(Debug, Subcommand)]
pub enum WorkRulesCommand {
    /// List rules that apply to active work.
    List {
        /// Optional path to inspect instead of active work paths.
        path: Option<Utf8PathBuf>,
    },
    /// Show one rule by id.
    Show {
        /// Rule id to show.
        id: String,
    },
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct WorkTaskArgs {
    #[command(subcommand)]
    pub command: WorkTaskCommand,
}

#[derive(Debug, Subcommand)]
pub enum WorkTaskCommand {
    /// Mark an open review task addressed and ready for independent rereview.
    Address(WorkTaskAddressArgs),
}

#[derive(Debug, Args)]
pub struct WorkTaskAddressArgs {
    /// Rapport-assigned review task id, such as REV-001.
    pub id: String,
    /// What changed to address the review action.
    #[arg(long)]
    pub summary: String,
}

#[derive(Debug, Args)]
pub struct BuildArgs {
    /// Optional paths to validate instead of the active work paths.
    #[arg(value_name = "PATH")]
    pub paths: Vec<Utf8PathBuf>,
}

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct ReviewArgs {
    #[command(subcommand)]
    pub command: ReviewCommand,
}

#[derive(Debug, Subcommand)]
pub enum ReviewCommand {
    /// Start an independent review and emit its host-neutral request.
    Start(ReviewStartArgs),
    /// Validate and record a reviewer's structured result.
    Complete(ReviewCompleteArgs),
}

#[derive(Debug, Args)]
pub struct ReviewStartArgs {
    /// Optional paths to review instead of all active work paths.
    #[arg(value_name = "PATH")]
    pub paths: Vec<Utf8PathBuf>,
    /// Emit the request packet as JSON instead of the default Markdown prompt.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct ReviewCompleteArgs {
    /// Structured JSON review result to validate and record.
    #[arg(long, value_name = "FILE")]
    pub result: Utf8PathBuf,
}

#[derive(Debug, Args)]
pub struct IntegrateArgs {
    /// Human summary for a new integration record. Omit when resuming signoff.
    #[arg(long)]
    pub summary: Option<String>,
    /// Git commit or PR message body. Omit when resuming signoff.
    #[arg(long)]
    pub message: Option<String>,
}
