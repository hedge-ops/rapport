use crate::run_with_environment;
use rapport_files::{FileSystem, InMemoryFileSystem, Utf8PathBuf};
use std::process::ExitCode;

fn run(fs: &mut InMemoryFileSystem, args: &[&str]) -> (ExitCode, String, String) {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run_with_environment(
        args.iter().map(|argument| (*argument).to_owned()),
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

fn succeeds(fs: &mut InMemoryFileSystem, args: &[&str]) -> String {
    let (code, out, err) = run(fs, args);
    assert_eq!(code, ExitCode::SUCCESS, "{args:?}: {err}");
    assert!(err.is_empty(), "{args:?}: {err}");
    out
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "the sequential acceptance test covers architecture CRUD and benchmark composition"
)]
/// When component declarations change, architecture and benchmarks remain inspectable.
fn context_commands_should_manage_architecture_and_benchmarks() {
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

    let effective = succeeds(&mut fs, &["context", "show", "app"]);
    assert!(effective.contains("`APP_RULE`"));
    assert_eq!(effective.matches("`TEAM`").count(), 1);
    assert!(effective.contains("declared by APP (direct, direct composition)"));
    succeeds(&mut fs, &["context", "validate", "app"]);

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
/// Hidden repository directories can own architecture context.
fn context_init_should_support_hidden_directories() {
    let mut fs = InMemoryFileSystem::default();
    fs.add_directory("/repo/.git");
    fs.add_directory("/repo/.github");
    fs.add_directory("/repo/.agents/skills");

    let github = succeeds(
        &mut fs,
        &[
            "context",
            "init",
            ".github",
            "--purpose",
            "GitHub automation.",
        ],
    );
    assert!(github.contains("`context` — DOT_GITHUB"));
    let agents = succeeds(
        &mut fs,
        &[
            "context",
            "init",
            ".agents/skills",
            "--purpose",
            "Agent skills.",
        ],
    );
    assert!(agents.contains("`context` — DOT_AGENTS_SKILLS"));

    succeeds(&mut fs, &["context", "validate", ".agents/skills"]);
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

    let (code, _, err) = run(&mut fs, &["context", "validate"]);

    assert_eq!(code, ExitCode::from(2));
    assert!(err.contains("entry ID is invalid"));
}
