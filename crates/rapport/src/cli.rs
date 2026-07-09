use clap::{Args, Parser, Subcommand};
use rapport_files::Utf8PathBuf;

const ROOT_ABOUT: &str = "repository rapport for human-directed agent work";
const ROOT_LONG_ABOUT: &str = "\
Rapport keeps human-directed agent work grounded in repository-owned rules, \
build conventions, Git/GitHub integration, and local state.";
const ROOT_AFTER_HELP: &str = "\
First loop:
  prime -> doctor -> work -> build -> integrate -> work complete

Rapport coordinates repository workflow; it does not replace Just or implement release/deploy behavior.";

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
    /// Validate active work with existing repository Just conventions.
    Build(BuildArgs),
    /// Turn validated local work into Git/GitHub integration state.
    Integrate(IntegrateArgs),
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
    /// Human summary for the integration record.
    #[arg(long)]
    pub summary: String,
    /// Git commit or PR message body.
    #[arg(long)]
    pub message: String,
}
