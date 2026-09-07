//! Root Rapport command-line grammar.
//!
//! This module owns top-level command routing types; each workflow module owns
//! its detailed grammar and execution.

use crate::{policy_context, shared_ruleset, work_ledger};
use clap::{Parser, Subcommand};

const ROOT_ABOUT: &str = "Repository architecture and review benchmarks";
const ROOT_LONG_ABOUT: &str = "Rapport turns repository-owned architecture and standards in context.toml into sourced component review prompts.";
const ROOT_AFTER_HELP: &str = "Start here:
  rapport context init <path> --purpose <text>
  rapport context show <path>
  rapport review <path>

Review requires no Work, build, integration, GitHub, or agent service.
Legacy lifecycle commands are deprecated; see docs/lifecycle-migration.md.";
const CONTEXT_LONG_ABOUT: &str = "\
Folder context answers what a project area is about before agents plan, code, test, build, \
review, or integrate. Ownership records what belongs in the folder. Boundaries describe \
neighboring responsibilities and where work should move instead. Rules are numbered, reviewable benchmarks for judging local work.";
const CONTEXT_AFTER_HELP: &str = "Edit repository-owned context.toml directly or use rapport context commands. Architecture and numbered benchmarks inherit from ancestors. Generate a complete sourced prompt with rapport review <path>.";

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
    #[command(hide = true)]
    Doctor,
    /// Configure repository-owned GitHub integration policy.
    #[command(hide = true)]
    Github(crate::github::Cli),
    /// Record Rapport usage in repository agent instructions.
    Init,
    /// Define and compose shared repository standards.
    Ruleset(shared_ruleset::Cli),
    /// Manage active local work state.
    #[command(hide = true)]
    Work(work_ledger::Cli),
    /// Manage the ordered sequence of development Action Tasks.
    #[command(hide = true)]
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
    #[command(hide = true)]
    Build(work_ledger::BuildCli),
    /// Generate a Markdown component review prompt from repository context.
    Review(crate::review::Cli),
    /// Turn validated local work into Git/GitHub integration state.
    #[command(hide = true)]
    Integrate(work_ledger::IntegrateCli),
}
