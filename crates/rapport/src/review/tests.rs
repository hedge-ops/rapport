//! Component review acceptance coverage.
//!
//! Verifies schema compatibility, sourced inheritance, and failures through the CLI.

use crate::policy_context::{Error, review_policy_for_paths};
use crate::run_with_environment;
use claims::{assert_err, assert_ok, assert_some};
use clap::Parser;
use pretty_assertions::assert_eq;
use rapport_files::Utf8Path;
use rapport_files::{FileSystem, InMemoryFileSystem, Utf8PathBuf};
use rstest::rstest;
use std::process::ExitCode;

fn run(fs: &mut InMemoryFileSystem, args: &[&str]) -> (ExitCode, String, String) {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run_with_environment(
        args.iter().map(|arg| (*arg).to_owned()),
        fs,
        Utf8PathBuf::from("/repo"),
        &mut out,
        &mut err,
    );
    (
        code,
        String::from_utf8_lossy(&out).into_owned(),
        String::from_utf8_lossy(&err).into_owned(),
    )
}

fn rule(table: &str, id: &str, text: &str) -> String {
    format!(
        r#"
[{table}.{id}]
text = "{text}"
rationale = "One owner preserves ordering and consistency."
[{table}.{id}.avoid]
language = "text"
text = "A UI handler directly updates synchronization checkpoints."
[{table}.{id}.prefer]
language = "text"
text = "The UI requests an operation; synchronization coordinates its state."
"#
    )
}

fn repository() -> InMemoryFileSystem {
    let mut fs = InMemoryFileSystem::default();
    fs.add_directory("/repo/.git");
    assert_ok!(fs.write_string(
        "/repo/context.toml",
        "namespace = 'ROOT'\npurpose = 'Repository architecture.'\n[ruleset]\nincludes = ['TEAM']\n"
    ));
    assert_ok!(fs.write_string(
        "/repo/.rapport/rules/custom/team-standards.toml",
        format!(
            "version = 1\nid = 'TEAM'\npurpose = 'Team standards.'\n{}",
            rule("rules", "TEAM_001", "Preserve domain ownership.")
        )
    ));
    assert_ok!(fs.write_string(
        "/repo/app/core/workspace_sync/context.toml",
        format!(
            r#"
type = "crate"
namespace = "SYNC"
purpose = "Coordinates encrypted synchronization and transfer state."
[ownership.SYNC_OWNERSHIP_001]
text = "Owns synchronization scheduling and transfer coordination."
[boundaries.SYNC_BOUNDARY_001]
text = "Document invariants and mutations belong to the domain component."
[ruleset]
includes = ["TEAM", "TEAM"]
{}"#,
            rule(
                "ruleset.rules",
                "SYNC_001",
                "Keep synchronization state transitions within this component."
            )
        )
    ));
    fs
}

#[test]
fn review_should_resolve_architecture_and_deduplicate_inherited_packs_without_work() {
    let mut fs = repository();
    let (code, out, err) = run(&mut fs, &["review", "app/core/workspace_sync"]);
    assert_eq!(
        code,
        ExitCode::SUCCESS,
        "expecting a stateless component review: {err}"
    );
    assert_eq!(out, include_str!("../testdata/review-component.md"));
    assert!(!fs.exists("/repo/.rapport/work.toml"));
    assert!(!fs.exists("/repo/.rapport/tasks"));
}

#[test]
fn review_should_render_direct_declarations_and_generated_producer_sources() {
    let mut fs = InMemoryFileSystem::default();
    fs.add_directory("/repo/.git");
    assert_ok!(fs.write_string(
        "/repo/context.toml",
        r#"version = 1
id = "ROOT"
purpose = "Repository architecture."
components = ["app/core/shared", "app/core/view"]

[ruleset]
includes = []
"#,
    ));
    assert_ok!(fs.write_string(
        "/repo/app/core/shared/context.toml",
        r#"version = 1
namespace = "SHARED"
purpose = "Produces shared view inputs."

[generated_outputs.facet_swift]
tool = "facet_generate"
target = "swift"

[ruleset]
includes = []
"#,
    ));
    assert_ok!(fs.write_string(
        "/repo/app/core/view/context.toml",
        r#"version = 1
namespace = "VIEW"
purpose = "Owns the view."
kustomizations = ["."]

[generated_inputs.app]
component = "app/core/shared"
output = "facet_swift"

[ruleset]
includes = []
"#,
    ));

    let (code, out, err) = run(&mut fs, &["review", "app/core/view"]);

    assert_eq!(
        code,
        ExitCode::SUCCESS,
        "expecting a dependency-aware review: {err}"
    );
    let expected = format!("{}\n", include_str!("../testdata/review-dependencies.md"));
    assert_eq!(out, expected);
}

#[test]
fn context_validate_should_reject_missing_generated_producer() {
    let mut fs = repository();
    assert_ok!(fs.write_string(
        "/repo/app/core/workspace_sync/context.toml",
        r#"version = 1
namespace = "SYNC"
purpose = "Coordinates synchronization."

[generated_inputs.app]
component = "app/core/missing"
output = "facet_swift"

[ruleset]
includes = []
"#,
    ));

    let error = review_error(&mut fs, "app/core/workspace_sync");

    assert!(matches!(
        error,
        Error::MissingGeneratedProducer {
            path,
            input,
            component,
        } if path == Utf8Path::new("/repo/app/core/workspace_sync/context.toml")
            && input == "app"
            && component == "app/core/missing"
    ));
}

#[test]
fn context_validate_should_reject_unknown_generated_output() {
    let mut fs = repository();
    assert_ok!(fs.write_string(
        "/repo/app/core/shared/context.toml",
        r#"version = 1
namespace = "SHARED"
purpose = "Produces shared inputs."

[generated_outputs.facet_swift]
tool = "facet_generate"
target = "swift"

[ruleset]
includes = []
"#,
    ));
    assert_ok!(fs.write_string(
        "/repo/app/core/workspace_sync/context.toml",
        r#"version = 1
namespace = "SYNC"
purpose = "Coordinates synchronization."

[generated_inputs.app]
component = "app/core/shared"
output = "missing_output"

[ruleset]
includes = []
"#,
    ));

    let error = review_error(&mut fs, "app/core/workspace_sync");

    assert!(matches!(
        error,
        Error::UnknownGeneratedOutput {
            path,
            input,
            component,
            output,
            producer_path,
        } if path == Utf8Path::new("/repo/app/core/workspace_sync/context.toml")
            && input == "app"
            && component == "app/core/shared"
            && output == "missing_output"
            && producer_path == Utf8Path::new("/repo/app/core/shared/context.toml")
    ));
}

#[test]
fn context_validate_should_reject_generated_dependency_cycles() {
    let mut fs = repository();
    assert_ok!(fs.write_string(
        "/repo/app/core/first/context.toml",
        r#"version = 1
namespace = "FIRST"
purpose = "First producer."

[generated_outputs.first_output]
tool = "first_generate"
target = "first"

[generated_inputs.second]
component = "app/core/second"
output = "second_output"

[ruleset]
includes = []
"#,
    ));
    assert_ok!(fs.write_string(
        "/repo/app/core/second/context.toml",
        r#"version = 1
namespace = "SECOND"
purpose = "Second producer."

[generated_outputs.second_output]
tool = "second_generate"
target = "second"

[generated_inputs.first]
component = "app/core/first"
output = "first_output"

[ruleset]
includes = []
"#,
    ));

    let error = review_error(&mut fs, ".");

    assert!(matches!(
        error,
        Error::GeneratedDependencyCycle(cycle)
            if cycle == [
                "/repo/app/core/first/context.toml",
                "/repo/app/core/second/context.toml",
                "/repo/app/core/first/context.toml",
            ]
    ));
}

#[test]
fn context_show_should_accept_namespace_in_unrelated_buildkite_context() {
    let mut fs = repository();
    assert_ok!(fs.write_string(
        "/repo/.buildkite/context.toml",
        "namespace = 'PIPELINE'\ntype = 'group'\npurpose = 'Pipeline configuration.'"
    ));
    let (code, out, err) = run(&mut fs, &["context", "show", "app/core/workspace_sync"]);
    assert_eq!(
        code,
        ExitCode::SUCCESS,
        "expecting namespace parsing across the repository: {err}"
    );
    assert_eq!(out, include_str!("../testdata/context-sync.md"));
}

#[test]
fn context_update_should_preserve_namespace_type_and_identifiers() {
    let mut fs = repository();
    let (code, _, err) = run(
        &mut fs,
        &[
            "context",
            "ownership",
            "add",
            "app/core/workspace_sync",
            "--text",
            "Owns retries.",
        ],
    );
    assert_eq!(
        code,
        ExitCode::SUCCESS,
        "expecting mutation to derive the next identifier: {err}"
    );
    let contents = assert_ok!(fs.read_to_string("/repo/app/core/workspace_sync/context.toml"));
    let document: toml::Value = assert_ok!(toml::from_str(&contents));
    assert_eq!(document["namespace"].as_str(), Some("SYNC"));
    assert_eq!(document["type"].as_str(), Some("crate"));
    assert_eq!(
        assert_some!(document["ownership"].as_table())
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["SYNC_OWNERSHIP_001", "SYNC_OWNERSHIP_002"]
    );
    assert_eq!(
        assert_some!(document["ruleset"]["rules"].as_table())
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["SYNC_001"]
    );
    assert_eq!(
        run(&mut fs, &["review", "app/core/workspace_sync"]).0,
        ExitCode::SUCCESS
    );
}

#[rstest]
#[case::missing_include(
    "namespace = 'ROOT'\npurpose = 'Root.'\n[ruleset]\nincludes = ['MISSING']",
    Failure::MissingInclude
)]
#[case::unknown_field(
    "namespace = 'ROOT'\npurpose = 'Root.'\nunsupported = true",
    Failure::Decode
)]
#[case::ambiguous_identity(
    "namespace = 'ROOT'\nid = 'ROOT'\npurpose = 'Root.'",
    Failure::Identity
)]
#[case::unknown_version(
    "version = 99\nnamespace = 'ROOT'\npurpose = 'Root.'",
    Failure::Version
)]
fn review_should_fail_explicitly_without_partial_prompt(
    #[case] contents: &str,
    #[case] failure: Failure,
) {
    let mut fs = repository();
    assert_ok!(fs.write_string("/repo/context.toml", contents));
    let (code, out, err) = run(&mut fs, &["review", "."]);
    assert_eq!(code, ExitCode::from(2), "{err}");
    assert!(out.is_empty());
    let error = review_error(&mut fs, ".");
    match (failure, error) {
        (Failure::MissingInclude, Error::UnresolvedInclude { path, included }) => {
            assert_eq!(path, Utf8Path::new("/repo/context.toml"));
            assert_eq!(included, "MISSING");
        }
        (Failure::Decode, Error::Decode { path, .. })
        | (Failure::Identity, Error::SchemaIdentity { path }) => {
            assert_eq!(path, Utf8Path::new("/repo/context.toml"));
        }
        (Failure::Version, Error::SchemaVersion { path, version }) => {
            assert_eq!(path, Utf8Path::new("/repo/context.toml"));
            assert_eq!(version, 99);
        }
        (_, error) => panic!("unexpected failure: {error:?}"),
    }
}

#[test]
fn review_should_reject_conflicting_local_and_included_identifiers() {
    let mut fs = repository();
    assert_ok!(fs.write_string(
        "/repo/other/context.toml",
        format!(
            "namespace = 'TEAM'\npurpose = 'Other component.'\n{}",
            rule("ruleset.rules", "TEAM_001", "Contradict domain ownership.")
        )
    ));
    let (code, out, err) = run(&mut fs, &["review", "other"]);
    assert_eq!(code, ExitCode::from(2), "{err}");
    assert!(out.is_empty());
    assert_conflict(review_error(&mut fs, "other"));
}

#[test]
fn review_should_report_missing_context() {
    let mut fs = InMemoryFileSystem::default();
    fs.add_directory("/repo");
    let (code, out, err) = run(&mut fs, &["review", "."]);
    assert_eq!(code, ExitCode::from(2), "{err}");
    assert!(out.is_empty());
    assert!(
        matches!(review_error(&mut fs, "."), Error::MissingContext(path) if path == Utf8Path::new("/repo"))
    );
}

#[test]
fn review_should_resolve_installed_catalog_packs_transitively() {
    let mut fs = repository();
    for pack in ["RUST_CRATE", "CRUX_APP"] {
        let (code, _, err) = run(&mut fs, &["ruleset", "catalog", "install", pack]);
        assert_eq!(
            code,
            ExitCode::SUCCESS,
            "expecting installed catalog pack: {err}"
        );
    }
    assert_ok!(fs.write_string(
        "/repo/context.toml",
        "namespace = 'ROOT'\npurpose = 'Root.'\n[ruleset]\nincludes = ['RUST_CRATE', 'CRUX_APP']"
    ));
    let (code, _, err) = run(&mut fs, &["review", "app/core/workspace_sync"]);
    assert_eq!(
        code,
        ExitCode::SUCCESS,
        "expecting transitive catalog resolution: {err}"
    );
    let shared = assert_ok!(crate::shared_ruleset::SharedRulesets::load(
        &mut fs,
        Utf8Path::new("/repo")
    ));
    for (pack, dependency) in [("RUST_CRATE", "RUST_CODING"), ("CRUX_APP", "CRUX_MODEL")] {
        let pack_id = assert_ok!(crate::shared_ruleset::RulesetId::parse(pack));
        let dependency_id = assert_ok!(crate::shared_ruleset::RulesetId::parse(dependency));
        assert!(
            assert_ok!(shared.require(&pack_id))
                .transitive()
                .contains(&dependency_id)
        );
        assert_eq!(
            assert_some!(assert_ok!(shared.require(&dependency_id)).rules().first())
                .id()
                .as_str(),
            format!("{dependency}_001")
        );
    }
}

#[test]
fn context_init_should_create_namespaced_architecture() {
    let mut fs = InMemoryFileSystem::default();
    fs.add_directory("/repo/app");
    let (code, _, err) = run(
        &mut fs,
        &[
            "context",
            "init",
            "app",
            "--namespace",
            "APP",
            "--type",
            "swift_package",
            "--purpose",
            "Owns the application.",
        ],
    );
    assert_eq!(
        code,
        ExitCode::SUCCESS,
        "expecting authored architecture: {err}"
    );
    let (code, out, err) = run(&mut fs, &["review", "app"]);
    assert_eq!(
        code,
        ExitCode::SUCCESS,
        "expecting immediate review without lifecycle setup: {err}"
    );
    assert_eq!(out, include_str!("../testdata/review-new-component.md"));
    let contents = assert_ok!(fs.read_to_string("/repo/app/context.toml"));
    let document: toml::Value = assert_ok!(toml::from_str(&contents));
    assert_eq!(document["namespace"].as_str(), Some("APP"));
}

#[test]
fn review_should_ignore_invalid_work_state_and_accept_multiple_paths() {
    let mut fs = repository();
    let invalid_work = "this is not TOML";
    assert_ok!(fs.write_string("/repo/.rapport/work.toml", invalid_work));
    let (code, out, err) = run(&mut fs, &["review", ".", "app/core/workspace_sync"]);
    assert_eq!(
        code,
        ExitCode::SUCCESS,
        "expecting work state not to affect review: {err}"
    );
    assert_eq!(out, include_str!("../testdata/review-multiple.md"));
    assert_eq!(
        assert_ok!(fs.read_to_string("/repo/.rapport/work.toml")),
        invalid_work
    );
}

#[test]
fn review_should_merge_identical_standards_and_preserve_both_sources() {
    let mut fs = repository();
    assert_ok!(fs.write_string(
        "/repo/other/context.toml",
        format!(
            "namespace = 'TEAM'\npurpose = 'Other component.'\n{}",
            rule("ruleset.rules", "TEAM_001", "Preserve domain ownership.")
        )
    ));
    let (code, out, err) = run(&mut fs, &["review", "other"]);
    assert_eq!(
        code,
        ExitCode::SUCCESS,
        "expecting identical standards to merge: {err}"
    );
    assert_eq!(out, include_str!("../testdata/review-merged-sources.md"));
}

#[test]
fn review_should_reject_duplicate_namespaces() {
    let mut fs = repository();
    assert_ok!(fs.write_string(
        "/repo/other/context.toml",
        "namespace = 'SYNC'\npurpose = 'Duplicate identity.'"
    ));
    let (code, out, err) = run(&mut fs, &["review", "other"]);
    assert_eq!(code, ExitCode::from(2), "{err}");
    assert!(out.is_empty());
    assert!(matches!(review_error(&mut fs, "other"), Error::DuplicateContext(id) if id == "SYNC"));
}

#[test]
fn review_should_reject_persisted_include_cycles() {
    let mut fs = repository();
    assert_ok!(fs.write_string(
        "/repo/.rapport/rules/custom/team-standards.toml",
        "version = 1\nid = 'TEAM'\npurpose = 'Team.'\nincludes = ['OTHER']"
    ));
    assert_ok!(fs.write_string(
        "/repo/.rapport/rules/other.toml",
        "version = 1\nid = 'OTHER'\npurpose = 'Other.'\nincludes = ['TEAM']"
    ));
    let (code, out, err) = run(&mut fs, &["review", "."]);
    assert_eq!(code, ExitCode::from(2), "{err}");
    assert!(out.is_empty());
    let Error::Ruleset(crate::shared_ruleset::Error::IncludeCycle(cycle)) =
        review_error(&mut fs, ".")
    else {
        panic!("expected include cycle");
    };
    assert_eq!(cycle, ["OTHER", "TEAM", "OTHER"]);
}

#[test]
fn review_should_default_to_root_and_treat_former_commands_as_paths() {
    let mut fs = repository();
    let (code, out, err) = run(&mut fs, &["review"]);
    assert_eq!(
        code,
        ExitCode::SUCCESS,
        "expecting default root review: {err}"
    );
    assert_eq!(out, include_str!("../testdata/review-root.md"));
    let (code, out, err) = run(&mut fs, &["review", "start"]);
    assert_eq!(
        code,
        ExitCode::SUCCESS,
        "expecting a path named after a legacy command: {err}"
    );
    assert_eq!(out, include_str!("../testdata/review-start-path.md"));
}

#[rstest]
#[case::work(&["work", "--help"])]
#[case::develop(&["develop", "--help"])]
#[case::build(&["build", "--help"])]
#[case::integrate(&["integrate", "--help"])]
#[case::github(&["github", "--help"])]
#[case::doctor(&["doctor", "--help"])]
#[case::context_grade(&["context", "review", "--help"])]
#[case::context_signoff(&["context", "signoff", "--help"])]
#[case::context_doctor(&["context", "doctor", "--help"])]
#[case::review_result(&["review", "complete", "--result", "result.json"])]
fn cli_should_reject_removed_lifecycle_commands(#[case] args: &[&str]) {
    let mut fs = repository();
    let (code, out, err) = run(&mut fs, args);
    assert_eq!(code, ExitCode::from(2), "{err}");
    assert!(out.is_empty());
    let error = assert_err!(crate::cli::Cli::try_parse_from(
        std::iter::once("rapport").chain(args.iter().copied())
    ));
    assert_eq!(
        error.kind(),
        if args[0] == "review" {
            clap::error::ErrorKind::UnknownArgument
        } else {
            clap::error::ErrorKind::InvalidSubcommand
        }
    );
}

#[rstest]
#[case::grade("[review]\nminimum_grade = 'A-'", "review")]
#[case::signoff("[[signoffs]]\nid = 'ROOT_SIGNOFF_CI'\ntarget = 'ci'", "signoffs")]
fn context_validate_should_reject_removed_fields(
    #[case] obsolete: &str,
    #[case] expected_field: &str,
) {
    let mut fs = repository();
    assert_ok!(fs.write_string(
        "/repo/context.toml",
        format!("namespace = 'ROOT'\npurpose = 'Root.'\n{obsolete}")
    ));
    let (code, out, err) = run(&mut fs, &["context", "validate"]);
    assert_eq!(code, ExitCode::from(2), "{err}");
    assert!(out.is_empty());
    let Error::LifecycleField { path, field } = review_error(&mut fs, ".") else {
        panic!("expected retired lifecycle field");
    };
    assert_eq!(path, Utf8Path::new("/repo/context.toml"));
    assert_eq!(field, expected_field);
}

#[test]
fn context_validate_should_ignore_work_and_generated_workflows() {
    let mut fs = repository();
    for path in [
        "/repo/.rapport/work.toml",
        "/repo/.github/workflows/rapport-signoff.yml",
    ] {
        assert_ok!(fs.write_string(path, "deliberately invalid legacy state"));
    }
    let (code, out, err) = run(&mut fs, &["context", "validate"]);
    assert_eq!(
        code,
        ExitCode::SUCCESS,
        "expecting only architecture validation: {err}"
    );
    assert_eq!(out, include_str!("../testdata/context-validate-two.md"));
    let (_, shown, _) = run(&mut fs, &["context", "show", "app/core/workspace_sync"]);
    assert_eq!(shown, include_str!("../testdata/context-sync.md"));
    let (code, _, err) = run(&mut fs, &["context", "remove", "app/core/workspace_sync"]);
    assert_eq!(
        code,
        ExitCode::SUCCESS,
        "expecting architecture removal: {err}"
    );
    for path in [
        "/repo/.rapport/work.toml",
        "/repo/.github/workflows/rapport-signoff.yml",
    ] {
        assert_eq!(
            assert_ok!(fs.read_to_string(path)),
            "deliberately invalid legacy state"
        );
    }
}

#[test]
fn context_validate_should_find_effective_conflicts_in_descendants() {
    let mut fs = repository();
    assert_ok!(fs.write_string(
        "/repo/other/context.toml",
        format!(
            "namespace = 'TEAM'\npurpose = 'Other component.'\n{}",
            rule(
                "ruleset.rules",
                "TEAM_001",
                "Contradict inherited standards."
            )
        )
    ));
    let (code, out, err) = run(&mut fs, &["context", "validate"]);
    assert_eq!(code, ExitCode::from(2), "{err}");
    assert!(out.is_empty());
    assert_conflict(review_error(&mut fs, "other"));
}

#[test]
fn context_validate_should_accept_components_without_a_root_context() {
    let mut fs = repository();
    assert_ok!(fs.remove_file("/repo/context.toml"));
    let (code, out, err) = run(&mut fs, &["context", "validate"]);
    assert_eq!(
        code,
        ExitCode::SUCCESS,
        "expecting all declared components to be validated: {err}"
    );
    assert_eq!(out, include_str!("../testdata/context-validate-one.md"));
}

fn review_error(fs: &mut InMemoryFileSystem, path: &str) -> Error {
    match review_policy_for_paths(fs, Utf8Path::new("/repo"), [Utf8Path::new(path)]) {
        Ok(_) => panic!("expected review resolution to fail"),
        Err(error) => error,
    }
}

#[derive(Debug)]
enum Failure {
    MissingInclude,
    Decode,
    Identity,
    Version,
}

fn assert_conflict(error: Error) {
    let Error::Ruleset(crate::shared_ruleset::Error::ConflictingRule {
        rule,
        first,
        second,
    }) = error
    else {
        panic!("expected benchmark conflict: {error:?}");
    };
    assert_eq!(rule, "TEAM_001");
    assert_eq!(first, "other/context.toml");
    assert_eq!(second, ".rapport/rules/custom/team-standards.toml");
}

#[test]
fn review_should_render_transitive_benchmarks_once_with_their_defining_sources() {
    let mut fs = repository();
    assert_ok!(fs.write_string(
        "/repo/.rapport/rules/foundation.toml",
        format!(
            "version = 1\nid = 'FOUNDATION'\npurpose = 'Foundation standards.'\n{}",
            rule("rules", "FOUNDATION_001", "Keep state changes explicit.")
        )
    ));
    let team_path = "/repo/.rapport/rules/custom/team-standards.toml";
    let team = assert_ok!(fs.read_to_string(team_path));
    assert_ok!(fs.write_string(team_path, format!("includes = ['FOUNDATION']\n{team}")));
    assert_ok!(fs.write_string("/repo/context.toml", "namespace = 'ROOT'\npurpose = 'Repository architecture.'\n[ruleset]\nincludes = ['TEAM', 'FOUNDATION']\n"));
    let (code, out, err) = run(&mut fs, &["review", "."]);
    assert_eq!(code, ExitCode::SUCCESS, "{err}");
    assert_eq!(out, include_str!("../testdata/review-transitive.md"));
}
