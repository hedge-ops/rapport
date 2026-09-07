//! Stateless component review entry point.
//!
//! Owns prompt generation from explicitly selected repository paths without lifecycle state.

use crate::{CommandContext, policy_context};
use clap::Args;
use rapport_files::{FileSystem, Utf8PathBuf};
use std::io::Write;
use std::process::ExitCode;

#[derive(Debug, Args)]
pub(crate) struct Cli {
    /// Repository-relative component or file paths (default: repository root).
    #[arg(value_name = "PATH", num_args = 1..)]
    paths: Vec<Utf8PathBuf>,
}

pub(crate) fn run<F, O, E>(cli: &Cli, context: &mut CommandContext<'_, F, O, E>) -> ExitCode
where
    F: FileSystem,
    O: Write,
    E: Write,
{
    let paths = if cli.paths.is_empty() {
        vec![Utf8PathBuf::from(".")]
    } else {
        cli.paths.clone()
    };
    match policy_context::review_policy_for_paths(
        context.fs,
        &context.repo_root,
        paths.iter().map(Utf8PathBuf::as_path),
    ) {
        Ok(policy) => {
            let _ = writeln!(
                context.out,
                "# Component Review\n\nReview the selected components and the changes supplied by the caller. Inspect the code before drawing conclusions. Use the architecture below to assess responsibilities and boundaries, and evaluate every applicable benchmark.\n\nReport actionable findings with severity, code path and line, benchmark or architecture identifier, source context path, evidence, and a concrete correction. Distinguish verified issues from questions and state any review limitations. If no issues are found, say so. This prompt does not contain a diff or execute a review.\n\n{}",
                policy.markdown
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            let _ = writeln!(context.err, "# rapport review\n\n{error}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests;
