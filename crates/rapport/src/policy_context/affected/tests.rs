use super::*;
use claims::{assert_err, assert_ok};
use pretty_assertions::assert_eq;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_REPOSITORY: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: Utf8PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let sequence = NEXT_REPOSITORY.fetch_add(1, Ordering::Relaxed);
        let root = assert_ok!(Utf8PathBuf::from_path_buf(std::env::temp_dir().join(
            format!("rapport-affected-{}-{sequence}", std::process::id())
        )));
        assert_ok!(std::fs::create_dir_all(&root));
        let fixture = Self { root };
        fixture.git(&["init", "-q", "-b", "main"]);
        fixture.git(&["config", "user.name", "Rapport Test"]);
        fixture.git(&["config", "user.email", "rapport@example.invalid"]);
        fixture.context(".", "ROOT", "Root policy.", &[]);
        fixture.context("app", "APP", "Application.", &[]);
        fixture.context("other space", "OTHER", "Other component.", &[]);
        fixture.write("app/file", "original\n");
        fixture.write("other space/file", "other\n");
        fixture.write(".gitignore", "ignored/\n");
        fixture.commit();
        fixture
    }

    fn git(&self, args: &[&str]) -> String {
        let result = assert_ok!(
            Command::new("git")
                .args(args)
                .current_dir(&self.root)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env(
                    "GIT_CONFIG_GLOBAL",
                    if cfg!(windows) { "NUL" } else { "/dev/null" }
                )
                .output()
        );
        assert!(
            result.status.success(),
            "expecting fixture git operation to succeed: {args:?}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_ok!(String::from_utf8(result.stdout))
            .trim()
            .to_owned()
    }

    fn write(&self, path: &str, contents: &str) {
        let path = self.root.join(path);
        assert_ok!(std::fs::create_dir_all(assert_ok!(
            path.parent().ok_or("missing parent")
        )));
        assert_ok!(std::fs::write(path, contents));
    }

    fn context(&self, directory: &str, id: &str, purpose: &str, includes: &[&str]) {
        self.write(&format!("{directory}/context.toml"), &format!("version = 1\nid = {id:?}\npurpose = {purpose:?}\nnext_ownership = 1\nnext_boundary = 1\n[ruleset]\nincludes = {includes:?}\n"));
    }

    fn commit(&self) -> String {
        self.git(&["add", "."]);
        self.git(&["commit", "-qm", "fixture change"]);
        self.git(&["rev-parse", "HEAD"])
    }

    fn discover(
        &self,
        base: Option<&str>,
        head: Option<&str>,
        pending: bool,
    ) -> Result<Selection, Error> {
        discover(
            &Args {
                base: base.map(str::to_owned),
                head: head.map(str::to_owned),
                pending,
                json: true,
            },
            &self.root,
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn component_with(path: &str, files: &[&str], policy: &[&str]) -> Component {
    Component {
        path: path.into(),
        changed_files: files.iter().map(|s| (*s).into()).collect(),
        policy_sources: policy.iter().map(|s| (*s).into()).collect(),
    }
}

/// Pending selection covers every local source while excluding ignored files and branch commits.
#[test]
fn discover_should_select_pending_sources_and_succeed_when_empty() {
    let fixture = Fixture::new();
    assert_eq!(
        assert_ok!(fixture.discover(None, None, true)).components,
        []
    );
    fixture.write("app/file", "committed\n");
    fixture.commit();
    assert_eq!(
        assert_ok!(fixture.discover(None, None, true)).components,
        []
    );
    fixture.write("app/staged", "staged\n");
    fixture.git(&["add", "app/staged"]);
    fixture.write("app/file", "unstaged\n");
    fixture.write("other space/new file", "untracked\n");
    fixture.write("ignored/context.toml", "invalid ignored policy");
    let result = assert_ok!(fixture.discover(None, None, true));
    assert_eq!(
        result.components,
        [
            component_with("app", &["app/file", "app/staged"], &[]),
            component_with("other space", &["other space/new file"], &[])
        ]
    );
    assert_eq!(result.comparison, None);
    assert_eq!(
        result.pending,
        Pending {
            included: true,
            head: Some(fixture.git(&["rev-parse", "HEAD"]))
        }
    );
    assert_eq!(assert_ok!(render(&result, false)), "app\n'other space'\n");
}

/// A diverged base uses its merge base, and explicit heads remain isolated from dirty context.
#[test]
fn discover_should_compare_merge_base_and_selected_head() {
    let fixture = Fixture::new();
    let ancestor = fixture.git(&["rev-parse", "HEAD"]);
    fixture.git(&["checkout", "-qb", "topic"]);
    fixture.write("app/file", "topic\n");
    let topic = fixture.commit();
    fixture.git(&["checkout", "-q", "main"]);
    fixture.write("other space/file", "base-only\n");
    let base = fixture.commit();
    fixture.write("context.toml", "invalid dirty declaration");
    let result = assert_ok!(fixture.discover(Some("main"), Some("topic"), false));
    assert_eq!(
        result.comparison,
        Some(Comparison {
            base,
            head: topic,
            merge_base: ancestor
        })
    );
    assert_eq!(
        result.pending,
        Pending {
            included: false,
            head: None
        }
    );
    assert_eq!(
        result.components,
        [component_with("app", &["app/file"], &[])]
    );
    assert!(matches!(
        assert_err!(fixture.discover(Some("main"), Some("topic"), true)),
        Error::UnrelatedHead { .. }
    ));
    assert!(matches!(
        assert_err!(fixture.discover(None, None, true)),
        Error::Policy(_)
    ));
}

/// Combined scope preserves committed and local evidence without duplicate components.
#[test]
fn discover_should_combine_scopes_and_respect_nested_ownership() {
    let fixture = Fixture::new();
    fixture.context("app/child", "CHILD", "Child.", &[]);
    let base = fixture.commit();
    fixture.write("app/file", "branch\n");
    fixture.commit();
    fixture.write("app/local", "local\n");
    fixture.write("root file", "root\n");
    let result = assert_ok!(fixture.discover(Some(&base), Some("HEAD"), true));
    assert_eq!(
        result.components,
        [
            component_with(".", &["root file"], &[]),
            component_with("app", &["app/file", "app/local"], &[])
        ]
    );
}

/// Renames retain both owners, and file deletions retain ownership from the earlier tree.
#[rstest::rstest]
#[case::committed(false)]
#[case::pending(true)]
fn discover_should_cover_moves_and_deletions(#[case] pending: bool) {
    let fixture = Fixture::new();
    let base = fixture.git(&["rev-parse", "HEAD"]);
    fixture.git(&["mv", "app/file", "other space/moved file"]);
    fixture.git(&["rm", "other space/file"]);
    if !pending {
        fixture.commit();
    }
    let result =
        assert_ok!(fixture.discover(if pending { None } else { Some(&base) }, None, pending));
    assert_eq!(
        result.components,
        [
            component_with("app", &["app/file"], &[]),
            component_with(
                "other space",
                &["other space/file", "other space/moved file"],
                &[]
            )
        ]
    );
}

/// Removed declarations fail explicitly instead of substituting an ancestor component.
#[rstest::rstest]
#[case::committed(false)]
#[case::pending(true)]
fn discover_should_report_removed_components(#[case] pending: bool) {
    let fixture = Fixture::new();
    let base = fixture.git(&["rev-parse", "HEAD"]);
    fixture.git(&["rm", "app/context.toml"]);
    if !pending {
        fixture.commit();
    }
    assert!(
        matches!(assert_err!(fixture.discover(if pending { None } else { Some(&base) }, None, pending)), Error::RemovedComponent(path) if path == "app")
    );
}

/// Ancestor policy edits select descendants with policy provenance, not invented source edits.
#[test]
fn discover_should_propagate_inherited_policy_and_ignore_formatting() {
    let fixture = Fixture::new();
    fixture.context(".", "ROOT", "Updated guidance.", &[]);
    let result = assert_ok!(fixture.discover(None, None, true));
    assert_eq!(
        result.components,
        [
            component_with(".", &["context.toml"], &["context.toml"]),
            component_with("app", &[], &["context.toml"]),
            component_with("other space", &[], &["context.toml"])
        ]
    );
    fixture.git(&["restore", "context.toml"]);
    let original = assert_ok!(std::fs::read_to_string(fixture.root.join("context.toml")));
    fixture.write("context.toml", &format!("# formatting only\n{original}"));
    assert_eq!(
        assert_ok!(fixture.discover(None, None, true)).components,
        [component_with(".", &["context.toml"], &[])]
    );
}

/// Transitive shared standards select only consuming components and preserve the shared source.
#[test]
fn discover_should_propagate_shared_standards() {
    let fixture = Fixture::new();
    fixture.write(
        ".rapport/rules/shared.toml",
        "version = 1\nid = 'SHARED'\npurpose = 'Original standards.'\nincludes = []\n",
    );
    fixture.write(
        ".rapport/rules/bundle.toml",
        "version = 1\nid = 'BUNDLE'\npurpose = 'Bundle.'\nincludes = ['SHARED']\n",
    );
    fixture.context("app", "APP", "Application.", &["BUNDLE"]);
    fixture.context("app/child", "CHILD", "Child.", &[]);
    fixture.commit();
    fixture.write(
        ".rapport/rules/shared.toml",
        "version = 1\nid = 'SHARED'\npurpose = 'Updated standards.'\nincludes = []\n",
    );
    assert_eq!(
        assert_ok!(fixture.discover(None, None, true)).components,
        [
            component_with(".", &[".rapport/rules/shared.toml"], &[]),
            component_with("app", &[], &[".rapport/rules/shared.toml"]),
            component_with("app/child", &[], &[".rapport/rules/shared.toml"])
        ]
    );
}

/// Staged changes remain selected when the working file is restored to its original contents.
#[test]
fn discover_should_preserve_intermediate_index_policy() {
    let fixture = Fixture::new();
    let original = assert_ok!(std::fs::read_to_string(fixture.root.join("context.toml")));
    fixture.context(".", "ROOT", "Staged guidance.", &[]);
    fixture.git(&["add", "context.toml"]);
    fixture.write("context.toml", &original);
    assert_eq!(
        assert_ok!(fixture.discover(None, None, true)).components,
        [
            component_with(".", &["context.toml"], &["context.toml"]),
            component_with("app", &[], &["context.toml"]),
            component_with("other space", &[], &["context.toml"])
        ]
    );
}

/// New local declarations control pending ownership without altering committed-only selection.
#[test]
fn discover_should_use_local_context_for_pending_files() {
    let fixture = Fixture::new();
    fixture.context("app/new", "NEW", "New area.", &[]);
    fixture.write("app/new/file", "new source\n");
    assert_eq!(
        assert_ok!(fixture.discover(None, None, true)).components,
        [component_with(
            "app/new",
            &["app/new/context.toml", "app/new/file"],
            &[]
        )]
    );
    assert_eq!(
        assert_ok!(fixture.discover(Some("HEAD"), None, false)).components,
        []
    );
}

/// Invalid scope, refs, histories, and missing ownership must never produce partial results.
#[test]
fn discover_should_report_typed_failures() {
    let fixture = Fixture::new();
    assert!(matches!(
        assert_err!(fixture.discover(None, None, false)),
        Error::InvalidScope
    ));
    assert!(matches!(
        assert_err!(fixture.discover(None, Some("HEAD"), true)),
        Error::InvalidScope
    ));
    assert!(matches!(
        assert_err!(fixture.discover(Some("missing-ref"), None, false)),
        Error::Git(rapport_git::GitError::CommandFailed { .. })
    ));
    fixture.git(&["checkout", "--orphan", "unrelated"]);
    fixture.write("unrelated-history", "independent tree\n");
    fixture.commit();
    assert!(matches!(
        assert_err!(fixture.discover(Some("main"), None, false)),
        Error::Git(rapport_git::GitError::CommandFailed {
            operation: "find merge base",
            ..
        })
    ));
    fixture.git(&["rm", "context.toml"]);
    fixture.commit();
    fixture.write("unowned", "no context\n");
    assert!(
        matches!(assert_err!(fixture.discover(None, None, true)), Error::Unmappable(path) if path == "unowned")
    );
}

#[test]
fn render_should_preserve_complete_json_contract() {
    let result = Selection {
        schema_version: 1,
        comparison: None,
        pending: Pending {
            included: true,
            head: Some("a".repeat(40)),
        },
        components: vec![component_with("app", &["app/file"], &["context.toml"])],
    };
    assert_eq!(
        assert_ok!(render(&result, true)),
        include_str!("../../testdata/affected.json")
    );
    assert_eq!(shell_path("a'b $c"), "'a'\\''b $c'");
    assert_eq!(
        assert_ok!(render(
            &Selection {
                components: vec![],
                ..result
            },
            false
        )),
        ""
    );
}

#[rstest::rstest]
#[case::no_scope(vec![], clap::error::ErrorKind::MissingRequiredArgument)]
#[case::head_without_base(vec!["--head", "HEAD", "--pending"], clap::error::ErrorKind::MissingRequiredArgument)]
fn cli_should_reject_invalid_scope(
    #[case] options: Vec<&str>,
    #[case] expected: clap::error::ErrorKind,
) {
    use clap::Parser;
    let error = assert_err!(crate::cli::Cli::try_parse_from(
        ["rapport", "context", "affected"]
            .into_iter()
            .chain(options)
    ));
    assert_eq!(error.kind(), expected);
}

/// A failed discovery leaves stdout empty for downstream machine consumers.
#[test]
fn run_should_keep_diagnostics_off_stdout() {
    let fixture = Fixture::new();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut fs = rapport_files::RealFileSystem;
    let code = crate::run_with_environment(
        ["context", "affected", "--base", "no-such-ref", "--json"].map(str::to_owned),
        &mut fs,
        fixture.root.clone(),
        &mut out,
        &mut err,
    );
    assert_eq!(code, ExitCode::from(2));
    assert_eq!(out, Vec::<u8>::new());
    assert!(!err.is_empty(), "expecting diagnostics on stderr");
    let code = crate::run_with_environment(
        ["context", "affected", "--pending", "--json"].map(str::to_owned),
        &mut fs,
        fixture.root.clone(),
        &mut out,
        &mut Vec::new(),
    );
    assert_eq!(code, ExitCode::SUCCESS);
    assert_eq!(
        assert_ok!(serde_json::from_slice::<serde_json::Value>(&out)),
        assert_ok!(serde_json::to_value(assert_ok!(
            fixture.discover(None, None, true)
        )))
    );
}

/// An unresolved merge cannot yield complete ownership evidence.
#[test]
fn discover_should_reject_conflicts_and_non_repositories() {
    let fixture = Fixture::new();
    fixture.git(&["checkout", "-qb", "topic"]);
    fixture.write("app/file", "topic\n");
    fixture.commit();
    fixture.git(&["checkout", "-q", "main"]);
    fixture.write("app/file", "main\n");
    fixture.commit();
    let merge = assert_ok!(
        Command::new("git")
            .args(["merge", "topic"])
            .current_dir(&fixture.root)
            .output()
    );
    assert!(!merge.status.success(), "expecting a conflicting merge");
    assert!(
        matches!(assert_err!(fixture.discover(None, None, true)), Error::Unmerged(paths) if paths == BTreeSet::from([Utf8PathBuf::from("app/file")]))
    );
    assert_ok!(std::fs::remove_dir_all(fixture.root.join(".git")));
    assert!(matches!(
        assert_err!(fixture.discover(None, None, true)),
        Error::Git(rapport_git::GitError::CommandFailed {
            operation: "discover repository",
            ..
        })
    ));
}

/// Removing an include attributes inherited policy changes to the edited context, not an unchanged pack.
#[test]
fn discover_should_attribute_composition_changes_to_the_edited_source() {
    let fixture = Fixture::new();
    fixture.write(
        ".rapport/rules/shared.toml",
        "version = 1\nid = 'SHARED'\npurpose = 'Shared standards.'\nincludes = []\n",
    );
    fixture.context("app", "APP", "Application.", &["SHARED"]);
    fixture.context("app/child", "CHILD", "Child.", &[]);
    fixture.commit();
    fixture.context("app", "APP", "Application.", &[]);
    assert_eq!(
        assert_ok!(fixture.discover(None, None, true)).components,
        [
            component_with("app", &["app/context.toml"], &["app/context.toml"]),
            component_with("app/child", &[], &["app/context.toml"])
        ]
    );
}

/// A dangling source symlink is still a changed Git path with an owner.
#[cfg(unix)]
#[test]
fn discover_should_map_untracked_dangling_symlinks() {
    let fixture = Fixture::new();
    assert_ok!(std::os::unix::fs::symlink(
        "missing-target",
        fixture.root.join("app/link")
    ));
    assert_eq!(
        assert_ok!(fixture.discover(None, None, true)).components,
        [component_with("app", &["app/link"], &[])]
    );
}

/// A committed shared rule edit propagates full benchmark changes through transitive includes.
#[test]
fn discover_should_select_consumers_of_changed_rule_content() {
    let fixture = Fixture::new();
    fixture.write(
        ".rapport/rules/shared.toml",
        "version = 1\nid = 'SHARED'\npurpose = 'Shared standards.'\nincludes = []\n",
    );
    fixture.context("app", "APP", "Application.", &["SHARED"]);
    let base = fixture.commit();
    fixture.write(
        ".rapport/rules/shared.toml",
        indoc::indoc! {"
        version = 1
        id = 'SHARED'
        purpose = 'Shared standards.'
        includes = []
        [rules.SHARED_001]
        text = 'Use structured errors.'
        rationale = 'Callers need typed failures.'
        [rules.SHARED_001.avoid]
        language = 'rust'
        text = 'Result<(), String>'
        [rules.SHARED_001.prefer]
        language = 'rust'
        text = 'Result<(), Error>'
    "},
    );
    fixture.commit();
    let expected = vec![
        component_with(".", &[".rapport/rules/shared.toml"], &[]),
        component_with("app", &[], &[".rapport/rules/shared.toml"]),
    ];
    assert_eq!(
        assert_ok!(fixture.discover(Some(&base), None, false)).components,
        expected
    );
    let args = Args {
        base: Some(base),
        head: None,
        pending: false,
        json: true,
    };
    assert_eq!(
        assert_ok!(discover(&args, &fixture.root.join("app"))).components,
        expected
    );
}
