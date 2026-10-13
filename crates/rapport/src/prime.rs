//! Agent orientation for architecture and reviews.
//!
//! Prints the complete bundled Markdown guide without rendering or transformation.

use crate::context::CommandContext;
use rapport_files::FileSystem;
use std::io::Write;
use std::process::ExitCode;

pub(crate) const RULE_AUTHORING_HELP: &str = "Rule authoring: run `rapport prime` for the authoritative Effective-series guide, linked style references, and a complete TOML example. Write --text as a concise, specific recommendation in natural language; choose the verb that expresses the judgment and reserve always or never for justified invariants. Explain why in --rationale, with scope or tradeoffs where useful. Pair concrete examples showing the discouraged and recommended approaches in the target language or tool syntax.";

pub fn run<F, O, E>(context: &mut CommandContext<'_, F, O, E>) -> ExitCode
where
    F: FileSystem,
    O: Write,
    E: Write,
{
    let _ = write!(context.out, "{}", include_str!("../prime.md"));
    ExitCode::SUCCESS
}
