use clap::{Args, Parser, Subcommand};
use rapport_files::Utf8PathBuf;

const ROOT_ABOUT: &str = "repository rapport for human-directed agent work";
const ROOT_LONG_ABOUT: &str = "\
Rapport keeps human-directed agent work grounded in repository-owned rules, \
build conventions, Git/GitHub integration, and local state.";
const ROOT_AFTER_HELP: &str = "\
First loop:
  work -> build -> integrate

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

impl Cli {
    #[must_use]
    pub fn command_path(&self) -> &'static str {
        match &self.command {
            Command::Work(work) => work.command_path(),
            Command::Build(_) => "build",
            Command::Integrate(_) => "integrate",
        }
    }

    #[must_use]
    pub fn pending_issue(&self) -> &'static str {
        match &self.command {
            Command::Work(work) => work.pending_issue(),
            Command::Build(_) => "#56",
            Command::Integrate(_) => "#57",
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum Command {
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

impl WorkArgs {
    #[must_use]
    pub fn command_path(&self) -> &'static str {
        match &self.command {
            WorkCommand::Start(_) => "work start",
            WorkCommand::Status => "work status",
            WorkCommand::Add(add) => add.command_path(),
            WorkCommand::Rules(rules) => rules.command_path(),
        }
    }

    #[must_use]
    pub fn pending_issue(&self) -> &'static str {
        match &self.command {
            WorkCommand::Start(_) => "#53",
            WorkCommand::Status => "#52",
            WorkCommand::Add(_) => "#55",
            WorkCommand::Rules(_) => "#54",
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum WorkCommand {
    /// Create the active local work session.
    Start(WorkStartArgs),
    /// Show active local work state.
    Status,
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
#[command(arg_required_else_help = true)]
pub struct WorkAddArgs {
    #[command(subcommand)]
    pub command: WorkAddCommand,
}

impl WorkAddArgs {
    #[must_use]
    pub fn command_path(&self) -> &'static str {
        match &self.command {
            WorkAddCommand::Path { .. } => "work add path",
        }
    }
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

impl WorkRulesArgs {
    #[must_use]
    pub fn command_path(&self) -> &'static str {
        match &self.command {
            WorkRulesCommand::List { .. } => "work rules list",
            WorkRulesCommand::Show { .. } => "work rules show",
        }
    }
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
    pub summary: Option<String>,
    /// Git commit or PR message body.
    #[arg(long)]
    pub message: Option<String>,
}
