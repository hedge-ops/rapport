//! Context command-line grammar.
//!
//! This module owns clap parsing types; command execution remains in the
//! boundary module.

use clap::{Args, Subcommand};
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
            .debug_struct("ContextCli")
            .field("action", &self.command.name())
            .finish()
    }
}

#[derive(Subcommand)]
pub(super) enum Action {
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
pub(super) struct OwnershipArgs {
    #[command(subcommand)]
    pub(super) command: OwnershipAction,
}

#[derive(Subcommand)]
pub(super) enum OwnershipAction {
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
pub(super) struct BoundaryArgs {
    #[command(subcommand)]
    pub(super) command: BoundaryAction,
}

#[derive(Subcommand)]
pub(super) enum BoundaryAction {
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
pub(super) struct RulesetArgs {
    #[command(subcommand)]
    pub(super) command: RulesetAction,
}

#[derive(Subcommand)]
pub(super) enum RulesetAction {
    Compose(ComposeArgs),
    Rule(ContextRuleArgs),
}

#[derive(Args)]
#[command(arg_required_else_help = true)]
pub(super) struct ComposeArgs {
    #[command(subcommand)]
    pub(super) command: ComposeAction,
}

#[derive(Subcommand)]
pub(super) enum ComposeAction {
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
pub(super) struct ContextRuleArgs {
    #[command(subcommand)]
    pub(super) command: ContextRuleAction,
}

#[derive(Subcommand)]
pub(super) enum ContextRuleAction {
    Add(RuleAddArgs),
    Update(RuleUpdateArgs),
    Remove {
        path: Utf8PathBuf,
        #[arg(long = "rule")]
        rule: String,
    },
}

#[derive(Args)]
pub(super) struct RuleAddArgs {
    pub(super) path: Utf8PathBuf,
    #[arg(long)]
    pub(super) id: String,
    #[arg(long)]
    pub(super) text: String,
    #[arg(long)]
    pub(super) rationale: String,
    #[arg(long)]
    pub(super) avoid_example: String,
    #[arg(long)]
    pub(super) avoid_language: String,
    #[arg(long)]
    pub(super) prefer_example: String,
    #[arg(long)]
    pub(super) prefer_language: String,
    #[arg(long)]
    pub(super) reference: Option<String>,
}

#[derive(Args)]
pub(super) struct RuleUpdateArgs {
    pub(super) path: Utf8PathBuf,
    #[arg(long = "rule")]
    pub(super) rule: String,
    #[arg(long)]
    pub(super) text: Option<String>,
    #[arg(long)]
    pub(super) rationale: Option<String>,
    #[arg(long, requires = "avoid_language")]
    pub(super) avoid_example: Option<String>,
    #[arg(long, requires = "avoid_example")]
    pub(super) avoid_language: Option<String>,
    #[arg(long, requires = "prefer_language")]
    pub(super) prefer_example: Option<String>,
    #[arg(long, requires = "prefer_example")]
    pub(super) prefer_language: Option<String>,
    #[arg(long, conflicts_with = "clear_reference")]
    pub(super) reference: Option<String>,
    #[arg(long, conflicts_with = "reference")]
    pub(super) clear_reference: bool,
}

#[derive(Args)]
#[command(arg_required_else_help = true)]
pub(super) struct ReviewArgs {
    #[command(subcommand)]
    pub(super) command: ReviewAction,
}

#[derive(Subcommand)]
pub(super) enum ReviewAction {
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
pub(super) struct SignoffArgs {
    #[command(subcommand)]
    pub(super) command: SignoffAction,
}

#[derive(Subcommand)]
pub(super) enum SignoffAction {
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
pub(super) struct SignoffIncludeArgs {
    #[command(subcommand)]
    pub(super) command: SignoffIncludeAction,
}

#[derive(Subcommand)]
pub(super) enum SignoffIncludeAction {
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
