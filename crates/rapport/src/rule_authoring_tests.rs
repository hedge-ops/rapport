//! Acceptance scenarios for authoring shared preferences and component invariants.

use crate::cli::Cli;
use crate::run_with_environment;
use claims::{assert_ok, assert_some};
use clap::CommandFactory;
use pretty_assertions::assert_eq;
use rapport_files::{FileSystem, InMemoryFileSystem, Utf8PathBuf};
use rstest::rstest;
use std::process::ExitCode;

fn shared_example() -> &'static str {
    let (_, example) = assert_some!(include_str!("../prime.md").split_once("```toml\n"));
    let (example, _) = assert_some!(example.split_once("\n```"));
    example
}

fn succeeds(fs: &mut InMemoryFileSystem, args: &[&str]) {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run_with_environment(
        args.iter().map(|arg| (*arg).to_owned()),
        fs,
        Utf8PathBuf::from("/repo"),
        &mut out,
        &mut err,
    );
    assert_eq!(
        code,
        ExitCode::SUCCESS,
        "expecting authoring to succeed: {err:?}"
    );
    assert_eq!(err, Vec::<u8>::new());
}

#[test]
fn prime_example_should_validate_as_a_complete_shared_ruleset() {
    let mut fs = InMemoryFileSystem::default();
    fs.add_directory("/repo/.git");
    assert_ok!(fs.write_string("/repo/.rapport/rules/api.toml", shared_example()));
    succeeds(
        &mut fs,
        &["context", "init", ".", "--purpose", "API policy."],
    );
    succeeds(
        &mut fs,
        &[
            "context",
            "ruleset",
            "compose",
            "add",
            ".",
            "--ruleset",
            "API",
        ],
    );
    succeeds(&mut fs, &["context", "validate"]);
}

/// When asked for an API error preference or a token logging invariant, authored fields survive CLI persistence and validation.
#[rstest]
#[case::shared_preference(false)]
#[case::component_absolute(true)]
fn rule_add_should_preserve_effective_style_examples(#[case] local: bool) {
    let document: toml::Value = assert_ok!(toml::from_str(if local {
        include_str!("testdata/authoring-local-absolute.toml")
    } else {
        shared_example()
    }));
    let rules = if local {
        &document["ruleset"]["rules"]
    } else {
        &document["rules"]
    };
    let rule = &rules["API_001"];
    let mut fs = InMemoryFileSystem::default();
    fs.add_directory("/repo/.git");
    fs.add_directory("/repo/api");
    succeeds(
        &mut fs,
        &["context", "init", ".", "--purpose", "Repository policy."],
    );
    let mut args = if local {
        succeeds(
            &mut fs,
            &[
                "context",
                "init",
                "api",
                "--namespace",
                "API",
                "--purpose",
                "Owns API authentication diagnostics.",
            ],
        );
        vec!["context", "ruleset", "rule", "add", "api"]
    } else {
        succeeds(
            &mut fs,
            &[
                "ruleset",
                "init",
                "API",
                "--purpose",
                "Error handling at Rust API boundaries.",
            ],
        );
        vec!["ruleset", "rule", "add", "API"]
    };
    args.extend([
        "--id",
        "API_001",
        "--text",
        assert_some!(rule["text"].as_str()),
        "--rationale",
        assert_some!(rule["rationale"].as_str()),
        "--avoid-example",
        assert_some!(rule["avoid"]["text"].as_str()),
        "--avoid-language",
        assert_some!(rule["avoid"]["language"].as_str()),
        "--prefer-example",
        assert_some!(rule["prefer"]["text"].as_str()),
        "--prefer-language",
        assert_some!(rule["prefer"]["language"].as_str()),
    ]);
    succeeds(&mut fs, &args);
    if !local {
        succeeds(
            &mut fs,
            &[
                "context",
                "ruleset",
                "compose",
                "add",
                ".",
                "--ruleset",
                "API",
            ],
        );
    }
    succeeds(&mut fs, &["context", "validate"]);
    let saved = assert_ok!(fs.read_to_string(if local {
        "/repo/api/context.toml"
    } else {
        "/repo/.rapport/rules/api.toml"
    }));
    let saved: toml::Value = assert_ok!(toml::from_str(&saved));
    let saved_rules = if local {
        &saved["ruleset"]["rules"]
    } else {
        &saved["rules"]
    };
    assert_eq!(&saved_rules["API_001"], rule);
}

#[rstest]
#[case::shared(vec!["ruleset", "rule"])]
#[case::component(vec!["context", "ruleset", "rule"])]
fn rule_help_should_route_authors_to_prime(#[case] path: Vec<&str>) {
    let root = Cli::command();
    let mut command = &root;
    for name in path {
        command = assert_some!(command.find_subcommand(name));
    }
    for command in [
        command,
        assert_some!(command.find_subcommand("add")),
        assert_some!(command.find_subcommand("update")),
    ] {
        assert_eq!(
            assert_some!(command.get_after_help()).to_string(),
            crate::prime::RULE_AUTHORING_HELP,
        );
    }
}
