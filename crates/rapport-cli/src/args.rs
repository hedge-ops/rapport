use std::fmt::Display;
use std::path::{Path, PathBuf};

pub trait Parser: Sized {
    /// Build the parsed command from a verb name and the remaining argv.
    ///
    /// # Errors
    ///
    /// Returns a [`ParseError`] when the verb is unknown, an expected
    /// argument is missing, or an argument value fails to parse.
    fn from_argv(verb: &str, rest: &[String]) -> Result<Self, ParseError>;

    /// Parses full CLI argv into a command. Splits into verb name and
    /// the remaining arguments, then dispatches to [`Parser::from_argv`].
    ///
    /// # Errors
    ///
    /// Returns [`ParseError::NoVerb`] when `argv` is empty, or any
    /// error produced by [`Parser::from_argv`].
    fn parse<I>(argv: I) -> Result<Self, ParseError>
    where
        I: IntoIterator<Item = String>,
    {
        let argv: Vec<String> = argv.into_iter().collect();
        let [verb, rest @ ..] = argv.as_slice() else {
            return Err(ParseError::NoVerb);
        };
        Self::from_argv(verb, rest)
    }
}

pub trait Argument: Sized {
    /// Parse this argument from a string.
    ///
    /// # Errors
    ///
    /// Returns a brief reason describing why the value did not parse.
    /// The [`Parser`] implementation composes the reason with the
    /// verb and value into a [`ParseError::InvalidArg`].
    fn parse(s: &str) -> Result<Self, String>;
}

#[derive(Debug)]
pub enum ParseError {
    NoVerb,
    UnknownVerb(String),
    MissingArg {
        verb: String,
        expected: &'static str,
    },
    InvalidArg {
        verb: String,
        value: String,
        reason: String,
    },
}

#[derive(Debug, Clone)]
pub struct RepositoryPath(PathBuf);

impl RepositoryPath {
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

impl Argument for RepositoryPath {
    fn parse(s: &str) -> Result<Self, String> {
        let p = PathBuf::from(s);
        if p.is_dir() {
            Ok(RepositoryPath(p))
        } else {
            Err("does not exist or is not a directory".to_string())
        }
    }
}

impl Display for RepositoryPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.display().fmt(f)
    }
}
