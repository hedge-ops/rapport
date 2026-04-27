//! Argument parsing primitives for rapport CLIs.

mod args;

pub use args::{
    Argument, FileSystem, HelpTarget, InMemoryFileSystem, Invocation, ParseError, Parser,
    RealFileSystem, RepositoryPath, ValidatedArgument, parse_arg, parse_validated,
};
