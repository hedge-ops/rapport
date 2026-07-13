//! Work command-line grammar.
//!
//! This module owns clap parsing types; Work orchestration remains in the
//! command boundary and focused workflow modules.

use clap::{ArgGroup, Args, Subcommand};
use rapport_files::Utf8PathBuf;
use std::fmt;

#[derive(Args)]
#[command(arg_required_else_help = true)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(super) command: Action,
}

impl fmt::Debug for Cli {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WorkCli")
            .field("action", &self.command.name())
            .finish()
    }
}

#[derive(Subcommand)]
pub(super) enum Action {
    /// Start Work from exactly one durable request source.
    Start(StartArgs),
    /// Derive the complete current Work state.
    Status,
    /// Inspect the Task ledger.
    Task(TaskArgs),
    /// Commit intentionally staged changes as a checkpoint.
    Checkpoint(CheckpointArgs),
    /// Rebase the source branch onto its current target.
    Rebase(RebaseArgs),
    /// Complete Work without Integration.
    Complete {
        #[arg(long)]
        result: String,
    },
    /// Stop tracking Work without claiming completion.
    Abandon {
        #[arg(long)]
        reason: String,
    },
    /// Inspect or permanently remove finalized Work History.
    History(super::history::Cli),
}

impl Action {
    fn name(&self) -> &'static str {
        match self {
            Self::Start(_) => "start",
            Self::Status => "status",
            Self::Task(_) => "task",
            Self::Checkpoint(_) => "checkpoint",
            Self::Rebase(_) => "rebase",
            Self::Complete { .. } => "complete",
            Self::Abandon { .. } => "abandon",
            Self::History(_) => "history",
        }
    }
}

#[derive(Args)]
#[command(group(
    ArgGroup::new("request")
        .required(true)
        .multiple(false)
        .args(["ticket", "plan", "ad_hoc"])
))]
pub(super) struct StartArgs {
    #[arg(long)]
    pub(super) ticket: Option<String>,
    #[arg(long)]
    pub(super) plan: Option<Utf8PathBuf>,
    #[arg(long)]
    pub(super) ad_hoc: Option<String>,
    #[arg(long)]
    pub(super) title: String,
    #[arg(long, required_unless_present = "ad_hoc", conflicts_with = "ad_hoc")]
    pub(super) description: Option<String>,
    #[arg(long)]
    pub(super) target: Option<String>,
}

#[derive(Args)]
#[command(arg_required_else_help = true)]
pub(super) struct TaskArgs {
    #[command(subcommand)]
    pub(super) command: TaskAction,
}

#[derive(Subcommand)]
pub(super) enum TaskAction {
    /// List and filter Tasks.
    List {
        #[arg(long)]
        status: Vec<String>,
        #[arg(long = "type")]
        task_type: Vec<String>,
        #[arg(long)]
        workflow: Vec<String>,
        #[arg(long)]
        related_to: Option<String>,
        #[arg(long)]
        since_checkpoint: bool,
        #[arg(long)]
        all: bool,
    },
    /// Show one complete Task envelope.
    Show { id: String },
    /// Show the next action without executing it.
    Next,
}

#[derive(Args)]
#[command(arg_required_else_help = true)]
pub(super) struct CheckpointArgs {
    #[command(subcommand)]
    pub(super) command: CheckpointAction,
}

#[derive(Subcommand)]
pub(super) enum CheckpointAction {
    Start,
    Complete {
        summary: String,
        #[arg(long)]
        description: Option<String>,
    },
    Cancel {
        #[arg(long)]
        reason: String,
    },
}

#[derive(Args)]
#[command(arg_required_else_help = true)]
pub(super) struct RebaseArgs {
    #[command(subcommand)]
    pub(super) command: RebaseAction,
}

#[derive(Subcommand)]
pub(super) enum RebaseAction {
    Start,
    Continue,
    Abort {
        #[arg(long)]
        reason: String,
    },
}
