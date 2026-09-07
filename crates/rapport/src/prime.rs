//! Agent orientation for architecture and reviews.
//!
//! Prints the complete bundled Markdown guide without rendering or transformation.

use crate::context::CommandContext;
use rapport_files::FileSystem;
use std::io::Write;
use std::process::ExitCode;

pub fn run<F, O, E>(context: &mut CommandContext<'_, F, O, E>) -> ExitCode
where
    F: FileSystem,
    O: Write,
    E: Write,
{
    let _ = write!(context.out, "{}", include_str!("../prime.md"));
    ExitCode::SUCCESS
}
