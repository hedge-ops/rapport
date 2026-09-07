//! Component review acceptance coverage.
//!
//! Verifies schema compatibility, sourced inheritance, and failures through the CLI.

use crate::{Clock, CommandOutcome, CommandRunner, CommandSpec, run_with_environment};
use claims::assert_ok;
use pretty_assertions::assert_eq;
use rapport_files::{FileSystem, InMemoryFileSystem, Utf8Path, Utf8PathBuf};
use rstest::rstest;
use std::{io, process::ExitCode};

struct FixedClock;
impl Clock for FixedClock {
    fn now_rfc3339(&self) -> String {
        "2026-09-07T00:00:00Z".to_owned()
    }
}
struct NoCommands;
impl CommandRunner for NoCommands {
    fn run(&self, _: &CommandSpec, _: &Utf8Path) -> io::Result<CommandOutcome> {
        panic!("component review must not execute external commands")
    }
}

fn run(fs: &mut InMemoryFileSystem, args: &[&str]) -> (ExitCode, String, String) {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run_with_environment(
        args.iter().map(|arg| (*arg).to_owned()),
        &NoCommands,
        fs,
        &FixedClock,
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
    for expected in [
        "Repository architecture.",
        "SYNC_OWNERSHIP_001",
        "SYNC_BOUNDARY_001",
        "`SYNC_001`",
        "Rationale:",
        "Avoid (text):",
        "Prefer (text):",
        "app/core/workspace_sync/context.toml",
        ".rapport/rules/custom/team-standards.toml",
        "Type: `crate`",
    ] {
        assert!(
            out.contains(expected),
            "expecting sourced architecture and standards: {expected}"
        );
    }
    assert_eq!(out.matches("`TEAM_001`").count(), 1);
    assert!(!fs.exists("/repo/.rapport/work.toml"));
    assert!(!fs.exists("/repo/.rapport/tasks"));
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
    assert!(out.contains("`SYNC_001`"));
    assert!(out.contains("`type` — `crate`"));
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
    for expected in [
        "namespace = \"SYNC\"",
        "type = \"crate\"",
        "SYNC_OWNERSHIP_002",
        "ruleset.rules.SYNC_001",
    ] {
        assert!(
            contents.contains(expected),
            "expecting schema metadata to survive mutation: {expected}"
        );
    }
    assert_eq!(
        run(&mut fs, &["review", "app/core/workspace_sync"]).0,
        ExitCode::SUCCESS
    );
}

#[rstest]
#[case::missing_include(
    "namespace = 'ROOT'\npurpose = 'Root.'\n[ruleset]\nincludes = ['MISSING']",
    "MISSING"
)]
#[case::unknown_field(
    "namespace = 'ROOT'\npurpose = 'Root.'\nunsupported = true",
    "unsupported"
)]
#[case::ambiguous_identity("namespace = 'ROOT'\nid = 'ROOT'\npurpose = 'Root.'", "exactly one")]
#[case::unknown_version(
    "version = 99\nnamespace = 'ROOT'\npurpose = 'Root.'",
    "schema version"
)]
fn review_should_fail_explicitly_without_partial_prompt(
    #[case] contents: &str,
    #[case] diagnostic: &str,
) {
    let mut fs = repository();
    assert_ok!(fs.write_string("/repo/context.toml", contents));
    let (code, out, err) = run(&mut fs, &["review", "."]);
    assert_eq!(code, ExitCode::from(2));
    assert!(out.is_empty());
    assert!(
        err.contains(diagnostic),
        "expecting an actionable failure: {err}"
    );
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
    assert_eq!(code, ExitCode::from(2));
    assert!(out.is_empty());
    assert!(err.contains("TEAM_001"));
    assert!(err.contains("other/context.toml"));
    assert!(err.contains(".rapport/rules/custom/team-standards.toml"));
}

#[test]
fn review_should_report_missing_context_with_creation_guidance() {
    let mut fs = InMemoryFileSystem::default();
    fs.add_directory("/repo");
    let (code, out, err) = run(&mut fs, &["review", "."]);
    assert_eq!(code, ExitCode::from(2));
    assert!(out.is_empty());
    assert!(err.contains("rapport context init"));
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
    let (code, out, err) = run(&mut fs, &["review", "app/core/workspace_sync"]);
    assert_eq!(
        code,
        ExitCode::SUCCESS,
        "expecting transitive catalog resolution: {err}"
    );
    assert_eq!(out.matches("`RUST_CODING_001`").count(), 1);
    assert!(out.contains("`CRUX_MODEL_001`"));
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
    assert!(out.contains("Type: `swift_package`"));
    let contents = assert_ok!(fs.read_to_string("/repo/app/context.toml"));
    assert!(contents.contains("namespace = \"APP\""));
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
    assert_eq!(out.matches("`TEAM_001`").count(), 1);
    assert!(out.contains("`SYNC_001`"));
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
    assert_eq!(out.matches("`TEAM_001`").count(), 1);
    assert!(out.contains(".rapport/rules/custom/team-standards.toml"));
    assert!(out.contains("other/context.toml"));
}

#[test]
fn review_should_reject_duplicate_namespaces() {
    let mut fs = repository();
    assert_ok!(fs.write_string(
        "/repo/other/context.toml",
        "namespace = 'SYNC'\npurpose = 'Duplicate identity.'"
    ));
    let (code, out, err) = run(&mut fs, &["review", "other"]);
    assert_eq!(code, ExitCode::from(2));
    assert!(out.is_empty());
    assert!(err.contains("SYNC"));
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
    assert_eq!(code, ExitCode::from(2));
    assert!(out.is_empty());
    assert!(err.contains("cycle"));
    assert!(err.contains("TEAM"));
    assert!(err.contains("OTHER"));
}

#[test]
fn review_should_default_to_root_and_preserve_cli_disambiguation() {
    let mut fs = repository();
    let (code, out, err) = run(&mut fs, &["review"]);
    assert_eq!(
        code,
        ExitCode::SUCCESS,
        "expecting default root review: {err}"
    );
    assert!(out.contains("`TEAM_001`"));
    assert!(!out.contains("`SYNC_001`"));
    let (code, out, err) = run(&mut fs, &["review", "--", "start"]);
    assert_eq!(
        code,
        ExitCode::SUCCESS,
        "expecting a path named after a legacy command: {err}"
    );
    assert!(out.contains("`ROOT` — start"));
}
