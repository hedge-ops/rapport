use crate::ParseError;

pub trait Parser: Sized + std::fmt::Debug {
    /// Lightweight verb identifier used to dispatch parsing and help.
    type Verb: std::fmt::Debug;

    /// Resolve a verb name (e.g. `"build"`) to a [`Self::Verb`].
    ///
    /// # Errors
    ///
    /// Returns [`ParseError::UnknownVerb`] when `name` is not a known verb.
    fn parse_verb(name: &str) -> Result<Self::Verb, ParseError>;

    /// Build the parsed command from a verb and the remaining argv.
    ///
    /// # Errors
    ///
    /// Returns a [`ParseError`] when an expected argument is missing,
    /// or an argument value fails to parse or validate.
    fn from_argv(verb: Self::Verb, rest: &[String]) -> Result<Self, ParseError>;

    /// Parse full CLI argv into an [`Invocation`]. Detects `-h`, `--help`,
    /// and the `help` subcommand before dispatching to argument parsing,
    /// so help requests do not require otherwise-mandatory arguments.
    ///
    /// # Errors
    ///
    /// Returns [`ParseError::NoVerb`] when `argv` is empty, or any error
    /// produced by [`Parser::parse_verb`] or [`Parser::from_argv`].
    fn parse<I>(argv: I) -> Result<Invocation<Self>, ParseError>
    where
        I: IntoIterator<Item = String>,
    {
        let argv: Vec<String> = argv.into_iter().collect();
        match argv.as_slice() {
            [] => Err(ParseError::NoVerb),
            [a] if is_help_flag(a) || a == "help" => Ok(Invocation::Help(HelpTarget::Top)),
            [first, verb_name] if first == "help" => {
                let verb = Self::parse_verb(verb_name)?;
                Ok(Invocation::Help(HelpTarget::Verb(verb)))
            }
            [name, rest @ ..] => {
                let verb = Self::parse_verb(name)?;
                if rest.iter().any(|a| is_help_flag(a)) {
                    Ok(Invocation::Help(HelpTarget::Verb(verb)))
                } else {
                    Self::from_argv(verb, rest).map(Invocation::Run)
                }
            }
        }
    }
}

/// What to print when the user asks for help.
#[derive(Debug)]
pub enum HelpTarget<V> {
    /// Top-level help (`rapport`, `rapport --help`, `rapport help`).
    Top,
    /// Help for a specific verb (`rapport build --help`, `rapport help build`).
    Verb(V),
}

/// The result of parsing argv: either a command to run, or a help request.
#[derive(Debug)]
pub enum Invocation<C: Parser> {
    Run(C),
    Help(HelpTarget<C::Verb>),
}

fn is_help_flag(s: &str) -> bool {
    s == "-h" || s == "--help"
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum TestVerb {
        Foo,
        Bar,
    }

    #[derive(Debug, PartialEq, Eq)]
    enum TestCommand {
        Foo,
        Bar { value: String },
    }

    impl Parser for TestCommand {
        type Verb = TestVerb;

        fn parse_verb(name: &str) -> Result<TestVerb, ParseError> {
            match name {
                "foo" => Ok(TestVerb::Foo),
                "bar" => Ok(TestVerb::Bar),
                _ => Err(ParseError::UnknownVerb(name.into())),
            }
        }

        fn from_argv(verb: TestVerb, rest: &[String]) -> Result<Self, ParseError> {
            match verb {
                TestVerb::Foo => Ok(Self::Foo),
                TestVerb::Bar => {
                    let [v] = rest else {
                        return Err(ParseError::MissingArg {
                            verb: "bar".into(),
                            expected: "value",
                        });
                    };
                    Ok(Self::Bar { value: v.clone() })
                }
            }
        }
    }

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn empty_argv_returns_no_verb() {
        let err = TestCommand::parse(argv(&[])).expect_err("expected error");
        assert!(matches!(err, ParseError::NoVerb));
    }

    #[rstest]
    #[case::dash_h(vec!["-h"])]
    #[case::double_dash_help(vec!["--help"])]
    #[case::help_word(vec!["help"])]
    fn top_level_help_routes_to_top(#[case] args: Vec<&str>) {
        let inv = TestCommand::parse(argv(&args)).expect("parse should succeed");
        assert!(matches!(inv, Invocation::Help(HelpTarget::Top)));
    }

    #[rstest]
    #[case::help_word_then_verb(vec!["help", "foo"])]
    #[case::verb_then_dash_h(vec!["foo", "-h"])]
    #[case::verb_then_double_dash_help(vec!["foo", "--help"])]
    #[case::verb_then_arg_then_help(vec!["foo", "extra", "--help"])]
    fn verb_help_routes_to_verb_target(#[case] args: Vec<&str>) {
        let inv = TestCommand::parse(argv(&args)).expect("parse should succeed");
        assert!(matches!(
            inv,
            Invocation::Help(HelpTarget::Verb(TestVerb::Foo))
        ));
    }

    #[test]
    fn unknown_verb_returns_unknown_verb_error() {
        let err = TestCommand::parse(argv(&["bogus"])).expect_err("expected error");
        match err {
            ParseError::UnknownVerb(v) => assert_eq!(v, "bogus"),
            other => panic!("expected UnknownVerb, got {other:?}"),
        }
    }

    #[test]
    fn help_with_unknown_verb_returns_unknown_verb_error() {
        let err = TestCommand::parse(argv(&["help", "bogus"])).expect_err("expected error");
        match err {
            ParseError::UnknownVerb(v) => assert_eq!(v, "bogus"),
            other => panic!("expected UnknownVerb, got {other:?}"),
        }
    }

    #[test]
    fn run_path_dispatches_to_from_argv_no_args() {
        let inv = TestCommand::parse(argv(&["foo"])).expect("parse should succeed");
        assert!(matches!(inv, Invocation::Run(TestCommand::Foo)));
    }

    #[test]
    fn run_path_passes_remaining_args_to_from_argv() {
        let inv = TestCommand::parse(argv(&["bar", "hello"])).expect("parse should succeed");
        match inv {
            Invocation::Run(TestCommand::Bar { value }) => assert_eq!(value, "hello"),
            other => panic!("expected Run(Bar), got {other:?}"),
        }
    }

    #[test]
    fn from_argv_errors_propagate() {
        let err = TestCommand::parse(argv(&["bar"])).expect_err("expected error");
        match err {
            ParseError::MissingArg { verb, expected } => {
                assert_eq!(verb, "bar");
                assert_eq!(expected, "value");
            }
            other => panic!("expected MissingArg, got {other:?}"),
        }
    }
}
