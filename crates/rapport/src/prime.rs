//! Agent orientation for architecture and reviews.
//!
//! Prints the complete bundled Markdown guide without rendering or transformation.

use crate::context::CommandContext;
use rapport_files::FileSystem;
use std::io::Write;
use std::process::ExitCode;

pub(crate) const RULE_AUTHORING_HELP: &str = "Rule authoring: run `rapport prime` for the authoritative Effective-series guide and a complete TOML example. Write --text as a short, specific recommendation; explain the failure, scope, and tradeoffs in --rationale; pair concrete examples of the same lesson in the relevant language or tool syntax. Use prefer, avoid, always, or never as justified by the rule.";

pub fn run<F, O, E>(context: &mut CommandContext<'_, F, O, E>) -> ExitCode
where
    F: FileSystem,
    O: Write,
    E: Write,
{
    let _ = write!(context.out, "{}", include_str!("../prime.md"));
    ExitCode::SUCCESS
}
