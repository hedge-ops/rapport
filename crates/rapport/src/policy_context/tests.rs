use crate::run_with_environment;
use claims::assert_ok;
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
    assert_eq!(listed, include_str!("../testdata/context-list.md"));

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
    assert_eq!(ownership, include_str!("../testdata/ownership-first.md"));
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
    assert_eq!(ownership, include_str!("../testdata/ownership-next.md"));
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
    assert_eq!(boundary, include_str!("../testdata/boundary-added.md"));
    let boundaries = succeeds(&mut fs, &["context", "boundary", "list", "app"]);
    assert_eq!(boundaries, include_str!("../testdata/boundary-list.md"));

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
    assert_eq!(effective, include_str!("../testdata/context-effective.md"));
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
    assert_eq!(declared, include_str!("../testdata/context-declared.md"));

    let removed = succeeds(&mut fs, &["context", "remove", "app"]);
    assert_eq!(removed, include_str!("../testdata/context-removed.md"));
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
    assert_eq!(github, include_str!("../testdata/context-github.md"));
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
    assert_eq!(agents, include_str!("../testdata/context-agents.md"));

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

    let (code, _, _) = run(&mut fs, &["context", "validate"]);

    assert_eq!(code, ExitCode::from(2));
    let error = claims::assert_err!(super::repository::Repository::load(
        &mut fs,
        rapport_files::Utf8Path::new("/repo")
    ));
    assert!(matches!(error, super::Error::InvalidEntryId(id) if id == "ROOT"));
}

#[test]
fn component_declarations_should_remain_direct_to_their_context() {
    let mut fs = InMemoryFileSystem::default();
    fs.add_directory("/repo/.git");
    assert_ok!(fs.write_string(
        "/repo/context.toml",
        r#"version = 1
id = "ROOT"
purpose = "Repository architecture."

[generated_outputs.root_output]
tool = "root_generate"
target = "root"

[ruleset]
includes = []
"#,
    ));
    assert_ok!(fs.write_string(
        "/repo/app/context.toml",
        r#"version = 1
id = "APP"
purpose = "Application architecture."

[ruleset]
includes = []
"#,
    ));

    let repository = assert_ok!(super::repository::Repository::load(
        &mut fs,
        rapport_files::Utf8Path::new("/repo")
    ));
    let root = assert_ok!(repository.at(rapport_files::Utf8Path::new(".")));
    let app = assert_ok!(repository.at(rapport_files::Utf8Path::new("app")));

    assert_eq!(root.context().generated_outputs().len(), 1);
    assert!(app.context().generated_outputs().is_empty());
    assert!(app.context().generated_inputs().is_empty());
    assert!(app.context().components().is_empty());
    assert!(app.context().kustomizations().is_empty());
}
