//! Root Rapport command-line grammar.
//!
//! This module owns top-level command routing types; each workflow module owns
//! its detailed grammar and execution.

use crate::{policy_context, shared_ruleset, work_ledger};
use clap::{Parser, Subcommand};

const ROOT_ABOUT: &str = "repository rapport for human-directed agent work";
const ROOT_LONG_ABOUT: &str = "\
Rapport keeps human-directed agent work grounded in repository-owned rules, \
build conventions, Git/GitHub integration, and local state.";
const ROOT_AFTER_HELP: &str = "\
First loop:
  prime -> doctor -> work -> develop -> build -> review -> integrate

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
    /// Configure repository-owned GitHub integration policy.
    Github(crate::github::Cli),
    /// Record Rapport usage in repository agent instructions.
    Init,
    /// Define and compose shared repository standards.
    Ruleset(shared_ruleset::Cli),
    /// Manage active local work state.
    Work(work_ledger::Cli),
    /// Manage the ordered sequence of development Action Tasks.
    Develop(work_ledger::DevelopCli),
    /// Manage folder-local structured project context.
    #[command(
        about = "Manage folder-local structured project context.",
        long_about = CONTEXT_LONG_ABOUT,
        after_help = CONTEXT_AFTER_HELP,
        after_long_help = CONTEXT_AFTER_HELP
    )]
    Context(policy_context::Cli),
    /// Validate active work with existing repository Just conventions.
    Build(work_ledger::BuildCli),
    /// Request or record an independent adversarial review of active work.
    Review(work_ledger::ReviewCli),
    /// Turn validated local work into Git/GitHub integration state.
    Integrate(work_ledger::IntegrateCli),
}
