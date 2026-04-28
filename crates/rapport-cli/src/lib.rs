//! Argument parsing primitives for rapport CLIs.

mod args;
mod parser;

pub use args::{
    Argument, FileSystem, InMemoryFileSystem, ParseError, RealFileSystem, RepositoryPath,
    ValidatedArgument, parse_arg, parse_validated,
};
pub use parser::{HelpTarget, Invocation, Parser};
