use crate::{Clock, CommandOutcome, CommandRunner, CommandSpec, run_with_environment};
use claims::assert_ok;
use rapport_files::{FileSystem, InMemoryFileSystem, Utf8Path, Utf8PathBuf};
use std::io;
use std::process::ExitCode;

#[derive(Debug)]
struct FixedClock;

impl Clock for FixedClock {
    fn now_rfc3339(&self) -> String {
        "2026-07-12T12:00:00Z".to_owned()
    }
}

#[derive(Debug)]
struct JustRunner;

impl CommandRunner for JustRunner {
    fn run(&self, spec: &CommandSpec, _cwd: &Utf8Path) -> io::Result<CommandOutcome> {
        assert_eq!(spec, &CommandSpec::new("just", ["--summary"]));
        Ok(CommandOutcome {
            success: true,
            stdout: "ci test".to_owned(),
            stderr: String::new(),
        })
    }
}

fn run(fs: &mut InMemoryFileSystem, args: &[&str]) -> (ExitCode, String, String) {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run_with_environment(
        args.iter().map(|argument| (*argument).to_owned()),
        &JustRunner,
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

fn succeeds(fs: &mut InMemoryFileSystem, args: &[&str]) -> String {
    let (code, out, err) = run(fs, args);
    assert_eq!(code, ExitCode::SUCCESS, "{args:?}: {err}");
    assert!(err.is_empty(), "{args:?}: {err}");
    out
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "the sequential acceptance test makes the complete Phase 2 lifecycle auditable"
)]
fn phase_two_context_policy_lifecycle() {
    let mut fs = InMemoryFileSystem::default();
    fs.add_directory("/repo/.git");
    fs.add_directory("/repo/app");
    fs.add_directory("/repo/other");

    succeeds(
        &mut fs,
        &["context", "init", ".", "--purpose", "Repository policy."],
    );
    succeeds(
        &mut fs,
        &["context", "init", "app", "--purpose", "Application policy."],
    );
    let listed = succeeds(&mut fs, &["context", "list"]);
    assert!(listed.contains("`ROOT`"));
    assert!(listed.contains("`APP`"));

    let ownership = succeeds(
        &mut fs,
        &[
            "context",
            "ownership",
            "add",
            "app",
            "--text",
            "Application behavior.",
        ],
    );
    assert!(ownership.contains("`APP_OWNERSHIP_001`"));
    succeeds(
        &mut fs,
        &[
            "context",
            "ownership",
            "remove",
            "app",
            "--id",
            "APP_OWNERSHIP_001",
        ],
    );
    let ownership = succeeds(
        &mut fs,
        &[
            "context",
            "ownership",
            "add",
            "app",
            "--text",
            "Application behavior.",
        ],
    );
    assert!(ownership.contains("`APP_OWNERSHIP_002`"));
    succeeds(
        &mut fs,
        &[
            "context",
            "ownership",
            "update",
            "app",
            "--id",
            "APP_OWNERSHIP_002",
            "--text",
            "User-facing application behavior.",
        ],
    );

    let boundary = succeeds(
        &mut fs,
        &[
            "context",
            "boundary",
            "add",
            "app",
            "--text",
            "Repository automation belongs at root.",
            "--owner",
            "ROOT",
        ],
    );
    assert!(boundary.contains("`APP_BOUNDARY_001`"));
    let boundaries = succeeds(&mut fs, &["context", "boundary", "list", "app"]);
    assert!(boundaries.contains("owner ROOT"));

    succeeds(
        &mut fs,
        &["ruleset", "init", "TEAM", "--purpose", "Team policy."],
    );
    succeeds(
        &mut fs,
        &[
            "context",
            "ruleset",
            "compose",
            "add",
            "app",
            "--ruleset",
            "TEAM",
        ],
    );
    succeeds(
        &mut fs,
        &[
            "context",
            "ruleset",
            "rule",
            "add",
            "app",
            "--id",
            "APP_RULE_001",
            "--text",
            "Keep UI policy in the app.",
            "--rationale",
            "The app owns user interaction.",
            "--avoid-example",
            "root UI policy",
            "--avoid-language",
            "text",
            "--prefer-example",
            "app UI policy",
            "--prefer-language",
            "text",
        ],
    );

    succeeds(
        &mut fs,
        &["context", "review", "set", ".", "--minimum-grade", "A-"],
    );
    succeeds(
        &mut fs,
        &["context", "review", "set", "app", "--minimum-grade", "A"],
    );
    let before_rejected_grade = assert_ok!(fs.read_to_string("/repo/app/context.toml"));
    let (code, _, err) = run(
        &mut fs,
        &["context", "review", "set", "app", "--minimum-grade", "B"],
    );
    assert_eq!(code, ExitCode::from(2));
    assert!(err.contains("cannot lower inherited grade"));
    assert_eq!(
        assert_ok!(fs.read_to_string("/repo/app/context.toml")),
        before_rejected_grade
    );

    succeeds(
        &mut fs,
        &[
            "context",
            "signoff",
            "add",
            "app",
            "--target",
            "ci",
            "--stage",
            "1",
            "--resource-group",
            "mac-display",
            "--include",
            "../other",
        ],
    );
    let workflow_path = "/repo/.github/workflows/rapport-app-signoff-ci.yml";
    let workflow = assert_ok!(fs.read_to_string(workflow_path));
    assert!(workflow.contains("name: \"Rapport App Signoff ci\""));
    assert!(workflow.contains("working-directory: \"app\""));
    assert!(workflow.contains("run: just ci"));
    assert!(workflow.contains("- \"other\""));

    let effective = succeeds(&mut fs, &["context", "show", "app"]);
    assert!(effective.contains("`APP_RULE`"));
    assert_eq!(effective.matches("`TEAM`").count(), 1);
    assert!(effective.contains("declared by APP (direct, direct composition)"));
    assert!(effective.contains("effective review minimum` — A"));

    let additional_trigger = succeeds(&mut fs, &["context", "show", "other"]);
    assert!(additional_trigger.contains("`APP_SIGNOFF_CI`"));
    assert!(additional_trigger.contains("trigger other"));
    assert!(!additional_trigger.contains("Application policy."));

    succeeds(&mut fs, &["context", "doctor", "app"]);
    assert_ok!(fs.write_string(workflow_path, "drift"));
    let (code, _, err) = run(&mut fs, &["context", "doctor", "app"]);
    assert_eq!(code, ExitCode::from(2));
    assert!(err.contains("missing or drifted"));
    succeeds(
        &mut fs,
        &[
            "context",
            "signoff",
            "repair",
            "app",
            "--signoff",
            "APP_SIGNOFF_CI",
        ],
    );
    succeeds(&mut fs, &["context", "doctor", "app"]);

    succeeds(
        &mut fs,
        &[
            "context",
            "signoff",
            "include",
            "remove",
            "app",
            "--signoff",
            "APP_SIGNOFF_CI",
            "--path",
            "../other",
        ],
    );
    succeeds(
        &mut fs,
        &[
            "context",
            "signoff",
            "remove",
            "app",
            "--signoff",
            "APP_SIGNOFF_CI",
        ],
    );
    assert!(!fs.is_file(workflow_path));

    succeeds(
        &mut fs,
        &[
            "context",
            "update",
            "app",
            "--purpose",
            "Updated application policy.",
        ],
    );
    let declared = succeeds(&mut fs, &["context", "show", "app", "--declared"]);
    assert!(declared.contains("Updated application policy."));
    assert!(!declared.contains("Repository policy."));

    let removed = succeeds(&mut fs, &["context", "remove", "app"]);
    assert!(removed.contains("context` — APP"));
    assert!(!fs.is_file("/repo/app/context.toml"));
}

#[test]
fn malformed_persisted_entry_identity_is_rejected() {
    let mut fs = InMemoryFileSystem::default();
    fs.add_directory("/repo/.git");
    fs.add_file_with_contents(
        "/repo/context.toml",
        r#"version = 1
id = "ROOT"
purpose = "Repository policy."
next_ownership = 1
next_boundary = 1

[ownership.ROOT_OWNERSHIP_001]
text = "Already allocated."

[ruleset]
includes = []
"#,
    );

    let (code, _, err) = run(&mut fs, &["context", "doctor"]);

    assert_eq!(code, ExitCode::from(2));
    assert!(err.contains("entry ID is invalid"));
}

#[test]
fn stale_included_path_can_be_removed_after_doctor_reports_it() {
    let mut fs = InMemoryFileSystem::default();
    fs.add_directory("/repo/.git");
    fs.add_directory("/repo/app");
    fs.add_file_with_contents(
        "/repo/app/context.toml",
        r#"version = 1
id = "APP"
purpose = "Application policy."
next_ownership = 1
next_boundary = 1

[ruleset]
includes = []

[[signoffs]]
id = "APP_SIGNOFF_CI"
target = "ci"
stage = 0
include = ["gone.txt"]
"#,
    );

    let (doctor_code, _, doctor_error) = run(&mut fs, &["context", "doctor", "app"]);
    assert_eq!(doctor_code, ExitCode::from(2));
    assert!(doctor_error.contains("included signoff path"));

    succeeds(
        &mut fs,
        &[
            "context",
            "signoff",
            "include",
            "remove",
            "app",
            "--signoff",
            "APP_SIGNOFF_CI",
            "--path",
            "../gone.txt",
        ],
    );
    let context = assert_ok!(fs.read_to_string("/repo/app/context.toml"));
    assert!(!context.contains("gone.txt"));
}
