//! Agent orientation for architecture and reviews.
//!
//! Prints the complete bundled Markdown guide without rendering or transformation.

use crate::context::CommandContext;
use rapport_files::FileSystem;
use std::io::Write;
use std::process::ExitCode;

pub(crate) const RULE_AUTHORING_HELP: &str = "Rule authoring: run `rapport prime` for the authoritative Effective-series guide and a complete TOML example. Write --text as a short title: Prefer|Avoid|Always|Never [best practice stated]. Write --rationale as the first paragraph explaining why, including the failure, scope, and tradeoffs. Pair realistic bad and good behavior in the target language or tool syntax. Choose the title verb as justified by the rule.";

pub fn run<F, O, E>(context: &mut CommandContext<'_, F, O, E>) -> ExitCode
where
    F: FileSystem,
    O: Write,
    E: Write,
{
    let _ = write!(context.out, "{}", include_str!("../prime.md"));
    ExitCode::SUCCESS
}
