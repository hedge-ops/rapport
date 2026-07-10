use clap::{Args, Parser, Subcommand};
use rapport_files::Utf8PathBuf;

const ROOT_ABOUT: &str = "repository rapport for human-directed agent work";
const ROOT_LONG_ABOUT: &str = "\
Rapport keeps human-directed agent work grounded in repository-owned rules, \
build conventions, Git/GitHub integration, and local state.";
const ROOT_AFTER_HELP: &str = "\
First loop:
  prime -> doctor -> work -> context -> build -> integrate -> work complete

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
    arg_required_else_help = true
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
    /// Manage active local work state.
    Work(WorkArgs),
    /// Manage folder-local structured project context.
    #[command(
        about = "Manage folder-local structured project context.",
        long_about = CONTEXT_LONG_ABOUT,
        after_help = CONTEXT_AFTER_HELP,
        after_long_help = CONTEXT_AFTER_HELP
    )]
    Context(ContextArgs),
    /// Validate active work with existing repository Just conventions.
    Build(BuildArgs),
    /// Turn validated local work into Git/GitHub integration state.
    Integrate(IntegrateArgs),
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
    /// Manage signoff targets and their generated GitHub request workflows.
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
    /// Declare a signoff target and generate its GitHub request workflow.
    Add {
        /// Folder whose context.toml owns the target.
        path: Utf8PathBuf,
        /// Kebab-case signoff target, such as ci or regression-ios.
        target: String,
    },
    /// Remove a signoff target and its generated GitHub request workflow.
    Remove {
        /// Folder whose context.toml owns the target.
        path: Utf8PathBuf,
        /// Existing signoff target.
        target: String,
    },
    /// Rewrite the exact Rapport-owned workflows for a declared target.
    Repair {
        /// Folder whose context.toml owns the target.
        path: Utf8PathBuf,
        /// Existing signoff target.
        target: String,
    },
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
pub struct BuildArgs {
    /// Optional paths to validate instead of the active work paths.
    #[arg(value_name = "PATH")]
    pub paths: Vec<Utf8PathBuf>,
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
