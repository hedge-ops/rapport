//! Argument parsing primitives for rapport CLIs.

mod args;
pub mod files;
mod parser;

pub use args::{
    Argument, ParseError, RepositoryPath, ValidatedArgument, parse_arg, parse_validated,
};
pub use files::{FileSystem, InMemoryFileSystem, RealFileSystem};
pub use parser::{HelpTarget, Invocation, Parser};
