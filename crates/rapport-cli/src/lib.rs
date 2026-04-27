//! Argument parsing primitives for rapport CLIs.

mod args;
pub use args::{
    Argument, FileSystem, InMemoryFileSystem, ParseError, Parser, RealFileSystem, RepositoryPath,
    ValidatedArgument, parse_arg, parse_validated,
};
