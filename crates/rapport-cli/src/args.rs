use camino::{Utf8Path, Utf8PathBuf};
use std::collections::HashSet;
use std::fmt::Display;

pub trait Parser: Sized {
    /// Build the parsed command from a verb name and the remaining argv.
    ///
    /// # Errors
    ///
    /// Returns a [`ParseError`] when the verb is unknown, an expected
    /// argument is missing, or an argument value fails to parse or
    /// validate.
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
    /// Parse a string into a typed argument value.
    ///
    /// Pure — does not touch the filesystem, network, or any external
    /// state. Argument types whose validity also depends on external
    /// state should additionally implement [`ValidatedArgument`].
    ///
    /// # Errors
    ///
    /// Returns a brief reason describing why the value did not parse
    /// (e.g., wrong format, invalid characters).
    fn parse(s: &str) -> Result<Self, String>;
}

pub trait ValidatedArgument<V>: Argument {
    /// Validate the parsed value against external state supplied via
    /// the validator (a filesystem, a network client, etc.).
    ///
    /// # Errors
    ///
    /// Returns a brief reason describing why the value failed
    /// validation in the validator's context.
    fn validate(&self, validator: &V) -> Result<(), String>;
}

/// Parse a single argument value, wrapping a [`Argument::parse`]
/// failure into a [`ParseError::InvalidArg`] that carries the verb
/// context.
///
/// # Errors
///
/// Returns [`ParseError::InvalidArg`] when [`Argument::parse`] fails.
pub fn parse_arg<A: Argument>(verb: &str, value: &str) -> Result<A, ParseError> {
    A::parse(value).map_err(|reason| ParseError::InvalidArg {
        verb: verb.into(),
        value: value.into(),
        reason,
    })
}

/// Parse and validate a single argument value, wrapping failures from
/// either stage into a [`ParseError::InvalidArg`].
///
/// # Errors
///
/// Returns [`ParseError::InvalidArg`] when either [`Argument::parse`]
/// or [`ValidatedArgument::validate`] fails for `value`.
pub fn parse_validated<A, V>(verb: &str, value: &str, validator: &V) -> Result<A, ParseError>
where
    A: ValidatedArgument<V>,
{
    let parsed = parse_arg::<A>(verb, value)?;
    parsed
        .validate(validator)
        .map_err(|reason| ParseError::InvalidArg {
            verb: verb.into(),
            value: value.into(),
            reason,
        })?;
    Ok(parsed)
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

pub trait FileSystem {
    fn is_dir(&self, path: &Utf8Path) -> bool;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct RealFileSystem;

impl FileSystem for RealFileSystem {
    fn is_dir(&self, path: &Utf8Path) -> bool {
        path.is_dir()
    }
}

#[derive(Debug, Default, Clone)]
pub struct InMemoryFileSystem {
    directories: HashSet<Utf8PathBuf>,
}

impl InMemoryFileSystem {
    pub fn add_directory(&mut self, path: impl Into<Utf8PathBuf>) {
        self.directories.insert(path.into());
    }
}

impl FileSystem for InMemoryFileSystem {
    fn is_dir(&self, path: &Utf8Path) -> bool {
        self.directories.contains(path)
    }
}

#[derive(Debug, Clone)]
pub struct RepositoryPath(Utf8PathBuf);

impl RepositoryPath {
    #[must_use]
    pub fn as_path(&self) -> &Utf8Path {
        &self.0
    }
}

impl Argument for RepositoryPath {
    fn parse(s: &str) -> Result<Self, String> {
        Ok(RepositoryPath(Utf8PathBuf::from(s)))
    }
}

impl<F: FileSystem> ValidatedArgument<F> for RepositoryPath {
    fn validate(&self, fs: &F) -> Result<(), String> {
        if fs.is_dir(&self.0) {
            Ok(())
        } else {
            Err("does not exist or is not a directory".into())
        }
    }
}

impl Display for RepositoryPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::current_dir(".")]
    #[case::root_slash("/")]
    #[case::missing_path("/does/not/exist")]
    #[case::relative("relative/path")]
    #[case::with_spaces("path with spaces")]
    #[case::empty_string("")]
    fn repository_path_parse_accepts_any_string(#[case] input: &str) {
        let parsed = RepositoryPath::parse(input).unwrap();
        assert_eq!(parsed.as_path().as_str(), input);
    }

    fn fs_with(dirs: &[&str]) -> InMemoryFileSystem {
        let mut fs = InMemoryFileSystem::default();
        for d in dirs {
            fs.add_directory(*d);
        }
        fs
    }

    #[rstest]
    #[case::single_directory("/work", &["/work"])]
    #[case::nested_directory("/work/inner", &["/work", "/work/inner"])]
    fn repository_path_validate_accepts_directory_in_fs(#[case] path: &str, #[case] dirs: &[&str]) {
        let fs = fs_with(dirs);
        let p = RepositoryPath::parse(path).unwrap();
        assert!(p.validate(&fs).is_ok());
    }

    #[rstest]
    #[case::missing_path("/nope", &[])]
    #[case::sibling_only("/work/jane", &["/work/john"])]
    #[case::parent_only("/work/jane", &["/work"])]
    fn repository_path_validate_rejects_path_not_in_fs(#[case] path: &str, #[case] dirs: &[&str]) {
        let fs = fs_with(dirs);
        let p = RepositoryPath::parse(path).unwrap();
        let err = p.validate(&fs).expect_err("validation should fail");
        assert!(err.contains("does not exist or is not a directory"));
    }

    #[rstest]
    fn parse_validated_succeeds_for_known_directory() {
        let fs = fs_with(&["/work"]);
        let result: Result<RepositoryPath, _> = parse_validated("build", "/work", &fs);
        assert!(result.is_ok());
    }

    #[rstest]
    fn parse_validated_wraps_validation_failure() {
        let fs = InMemoryFileSystem::default();
        let result: Result<RepositoryPath, _> = parse_validated("build", "/nope", &fs);
        match result {
            Err(ParseError::InvalidArg {
                verb,
                value,
                reason,
            }) => {
                assert_eq!(verb, "build");
                assert_eq!(value, "/nope");
                assert!(reason.contains("does not exist"));
            }
            other => panic!("expected InvalidArg, got {other:?}"),
        }
    }
}
