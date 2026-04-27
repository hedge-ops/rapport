//! Argument parsing and command-running primitives for rapport CLIs.

mod args;
mod runner;

pub use args::{
    Argument, FileSystem, HelpTarget, InMemoryFileSystem, Invocation, ParseError, Parser,
    RealFileSystem, RepositoryPath, ValidatedArgument, parse_arg, parse_validated,
};
pub use runner::{CommandOutcome, CommandRunner, RealCommandRunner};
