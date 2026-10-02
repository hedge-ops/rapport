//! Component dependency ordering and freshness behavior.

use super::*;
use claims::{assert_err, assert_ok};
use pretty_assertions::assert_eq;
use rapport_command::CommandOutcome;
use rapport_files::{InMemoryFileSystem, RealFileSystem};
use std::sync::atomic::{AtomicUsize, Ordering};

fn declarations() -> BTreeMap<Utf8PathBuf, policy_context::ComponentContext> {
    let mut fs = InMemoryFileSystem::default();
    fs.add_directory("/repo/.git");
    for (path, text) in [
        (
            "producer",
            "namespace = 'PRODUCER'\npurpose = 'Produces types'\ntype = 'crate'\n[generated_outputs.types]\ntool = 'facet_generate'\ntarget = 'swift'",
        ),
        (
            "first",
            "namespace = 'FIRST'\npurpose = 'Consumes types'\ntype = 'swift_package'\n[generated_inputs.types]\ncomponent = 'producer'\noutput = 'types'",
        ),
        (
            "second",
            "namespace = 'SECOND'\npurpose = 'Consumes types'\ntype = 'swift_package'\n[generated_inputs.types]\ncomponent = 'producer'\noutput = 'types'",
        ),
    ] {
        assert_ok!(fs.write_string(format!("/repo/{path}/context.toml"), text));
    }
    assert_ok!(policy_context::declarations(
        &mut fs,
        Utf8Path::new("/repo")
    ))
}

/// When two consumers share a producer, generation precedes both and runs once.
#[test]
fn shared_producer_is_generated_once_before_consumers() {
    let plan = assert_ok!(graph::plan(
        &declarations(),
        &["first".into(), "second".into()],
        Target::Build
    ));
    assert_eq!(
        plan,
        vec![
            Operation::Generate(Artifact {
                component: "producer".into(),
                output: "types".to_owned()
            }),
            Operation::Run {
                component: "first".into(),
                target: Target::Build
            },
            Operation::Run {
                component: "second".into(),
                target: Target::Build
            },
        ]
    );
}

#[test]
fn missing_explicit_component_is_not_replaced_by_parent() {
    let error = assert_err!(graph::plan(
        &declarations(),
        &["first/child".into()],
        Target::Build
    ));
    assert!(
        matches!(error, graph::Error::MissingComponent(path) if path == Utf8Path::new("first/child"))
    );
}

struct Workspace(Utf8PathBuf);
impl Workspace {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "rapport-generation-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let path = assert_ok!(Utf8PathBuf::from_path_buf(path));
        assert_ok!(std::fs::create_dir_all(path.join(".git")));
        assert_ok!(std::fs::write(path.join("source.txt"), "first"));
        Self(path)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct GeneratorRunner {
    root: Utf8PathBuf,
    calls: AtomicUsize,
    version: String,
    fail: bool,
}
impl Runner for GeneratorRunner {
    fn run(&self, spec: &CommandSpec) -> std::io::Result<CommandOutcome> {
        let version = spec.arguments() == ["--version"];
        if !version {
            self.calls.fetch_add(1, Ordering::SeqCst);
            std::fs::write(
                self.root.join("output.txt"),
                std::fs::read(self.root.join("source.txt"))?,
            )?;
        }
        Ok(CommandOutcome::new(
            version || !self.fail,
            Some(i32::from(!version && self.fail)),
            self.version.as_bytes().to_vec(),
            vec![],
            std::time::Duration::ZERO,
        ))
    }
}

fn prepared(root: &Utf8Path, committed: bool) -> Vec<Prepared> {
    vec![Prepared {
        operation: Operation::Generate(Artifact {
            component: ".".into(),
            output: "types".to_owned(),
        }),
        commands: vec![CommandSpec::new("fake-generator").current_dir(root)],
        generator: Some(declaration::Generator {
            inputs: assert_ok!(vec!["source.txt".to_owned()].try_into()),
            outputs: assert_ok!(
                vec![declaration::Output {
                    path: "output.txt".to_owned(),
                    candidate: None
                }]
                .try_into()
            ),
            environment: vec![],
            adapter: declaration::Adapter::Facet {
                language: declaration::FacetLanguage::Swift,
                destination: "output.txt".to_owned(),
            },
            committed,
        }),
    }]
}

fn execute_generated(
    workspace: &Workspace,
    runner: &GeneratorRunner,
    force: bool,
    committed: bool,
    explicit: bool,
) -> Result<(), Error> {
    let mut fs = RealFileSystem;
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut context = CommandContext::new(workspace.0.clone(), &mut fs, &mut out, &mut err);
    run_with_runner(
        &mut context,
        prepared(&workspace.0, committed),
        force,
        false,
        explicit,
        runner,
    )
}

/// Reuse is valid only while sources, tool identity, and output bytes remain current.
#[test]
fn generation_reuses_only_current_intact_outputs() {
    let workspace = Workspace::new();
    let mut runner = GeneratorRunner {
        root: workspace.0.clone(),
        calls: AtomicUsize::new(0),
        version: "v1".to_owned(),
        fail: false,
    };
    assert_ok!(execute_generated(&workspace, &runner, false, false, false));
    assert_ok!(execute_generated(&workspace, &runner, false, false, false));
    assert_eq!(runner.calls.load(Ordering::SeqCst), 1);
    assert_ok!(std::fs::write(workspace.0.join("source.txt"), "second"));
    assert_ok!(execute_generated(&workspace, &runner, false, false, false));
    assert_ok!(std::fs::write(workspace.0.join("output.txt"), "corrupted"));
    assert_ok!(execute_generated(&workspace, &runner, false, false, false));
    runner.version = "v2".to_owned();
    assert_ok!(execute_generated(&workspace, &runner, false, false, false));
    assert_ok!(execute_generated(&workspace, &runner, true, false, false));
    assert_eq!(runner.calls.load(Ordering::SeqCst), 5);
}

#[test]
fn strict_validation_does_not_rewrite_committed_output() {
    let workspace = Workspace::new();
    let runner = GeneratorRunner {
        root: workspace.0.clone(),
        calls: AtomicUsize::new(0),
        version: "v1".to_owned(),
        fail: false,
    };
    let error = assert_err!(execute_generated(&workspace, &runner, false, true, false));
    assert!(matches!(error, Error::Stale(_)));
    assert_eq!(runner.calls.load(Ordering::SeqCst), 0);
    assert_ok!(execute_generated(&workspace, &runner, false, true, true));
    assert_ok!(execute_generated(&workspace, &runner, false, true, false));
    assert_eq!(runner.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn failed_generation_does_not_leave_a_reusable_receipt() {
    let workspace = Workspace::new();
    let mut runner = GeneratorRunner {
        root: workspace.0.clone(),
        calls: AtomicUsize::new(0),
        version: "v1".to_owned(),
        fail: false,
    };
    assert_ok!(execute_generated(&workspace, &runner, false, false, false));
    runner.fail = true;
    assert!(matches!(
        assert_err!(execute_generated(&workspace, &runner, true, false, false)),
        Error::Failed { .. }
    ));
    runner.fail = false;
    assert_ok!(execute_generated(&workspace, &runner, false, false, false));
    assert_eq!(runner.calls.load(Ordering::SeqCst), 3);
}

#[test]
fn dry_run_does_not_invoke_tools_or_write_outputs() {
    let workspace = Workspace::new();
    let runner = GeneratorRunner {
        root: workspace.0.clone(),
        calls: AtomicUsize::new(0),
        version: "v1".to_owned(),
        fail: false,
    };
    let mut fs = RealFileSystem;
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut context = CommandContext::new(workspace.0.clone(), &mut fs, &mut out, &mut err);
    assert_ok!(run_with_runner(
        &mut context,
        prepared(&workspace.0, false),
        false,
        true,
        false,
        &runner
    ));
    assert_eq!(runner.calls.load(Ordering::SeqCst), 0);
    assert!(!workspace.0.join("output.txt").exists());
    assert!(!workspace.0.join("target").exists());
}

#[rstest::rstest]
#[case::facet_apple("kind = 'facet'\nlanguage = 'apple'\ndestination = 'out'")]
#[case::boltffi_swift("kind = 'boltffi'\nplatform = 'swift'")]
#[case::facet_missing_destination("kind = 'facet'\nlanguage = 'swift'")]
#[case::boltffi_irrelevant_destination(
    "kind = 'boltffi'\nplatform = 'android'\ndestination = 'out'"
)]
#[case::empty_commands("kind = 'command'\ncommands = []")]
#[case::empty_program("kind = 'command'\ncommands = [{program = '', cwd = '.'}]")]
#[case::just("kind = 'command'\ncommands = [{program = 'just', cwd = '.'}]")]
#[case::absolute_just(
    "kind = 'command'\ncommands = [{program = '/usr/local/bin/just', cwd = '.'}]"
)]
fn adapter_should_reject_invalid_declarations(#[case] text: &str) {
    assert_err!(toml::from_str::<declaration::Adapter>(text));
}

#[rstest::rstest]
#[case::swift(declaration::FacetLanguage::Swift, "swift")]
#[case::kotlin(declaration::FacetLanguage::Kotlin, "kotlin")]
#[case::csharp(declaration::FacetLanguage::Csharp, "csharp")]
fn facet_should_generate_the_selected_language(
    #[case] language: declaration::FacetLanguage,
    #[case] argument: &str,
) {
    let workspace = Workspace::new();
    let commands = assert_ok!(adapters::generation(
        &workspace.0,
        Utf8Path::new("producer"),
        &declaration::Adapter::Facet {
            language,
            destination: "generated".to_owned()
        },
    ));
    assert_eq!(
        commands,
        vec![
            CommandSpec::new("cargo")
                .current_dir(&workspace.0)
                .args(["run", "--manifest-path"])
                .arg(workspace.0.join("producer/Cargo.toml").as_str())
                .args([
                    "--bin",
                    "codegen",
                    "--features",
                    "codegen,facet_typegen",
                    "--",
                    "--language",
                    argument,
                    "--output-dir"
                ])
                .arg(workspace.0.join("generated").as_str())
        ]
    );
}

#[test]
fn boltffi_should_package_android_without_a_destination_setting() {
    let commands = assert_ok!(adapters::generation(
        Utf8Path::new("/repo"),
        Utf8Path::new("producer"),
        &declaration::Adapter::Boltffi {
            platform: declaration::BoltFfiPlatform::Android
        },
    ));
    assert_eq!(
        commands,
        vec![
            CommandSpec::new("boltffi")
                .args(["pack", "android"])
                .current_dir("/repo/producer")
        ]
    );
}

#[test]
fn command_should_preserve_literal_arguments_order_and_working_directories() {
    let workspace = Workspace::new();
    let adapter: declaration::Adapter = assert_ok!(toml::from_str(
        r#"
kind = "command"
commands = [
  { program = "cargo", args = ["run", "--package", "generate-strings", "--", "swift", "--locales", "app/core/strings"], cwd = "." },
  { program = "swiftformat", args = ["generated/a file.swift", "$(literal)"], cwd = "consumer" },
]
"#
    ));
    assert_eq!(
        assert_ok!(adapters::generation(
            &workspace.0,
            Utf8Path::new("producer"),
            &adapter
        )),
        vec![
            CommandSpec::new("cargo")
                .args([
                    "run",
                    "--package",
                    "generate-strings",
                    "--",
                    "swift",
                    "--locales",
                    "app/core/strings"
                ])
                .current_dir(workspace.0.join(".")),
            CommandSpec::new("swiftformat")
                .args(["generated/a file.swift", "$(literal)"])
                .current_dir(workspace.0.join("consumer")),
        ]
    );
    assert_eq!(
        assert_ok!(toml::from_str::<declaration::Adapter>(&assert_ok!(
            toml::to_string(&adapter)
        ))),
        adapter
    );
}

#[test]
fn command_should_reject_a_working_directory_outside_the_repository() {
    let workspace = Workspace::new();
    let adapter = assert_ok!(toml::from_str::<declaration::Adapter>(
        "kind = 'command'\ncommands = [{program = 'cargo', cwd = '../outside'}]"
    ));
    assert!(matches!(
        assert_err!(adapters::generation(
            &workspace.0,
            Utf8Path::new("."),
            &adapter
        )),
        Error::Path(_)
    ));
}

#[test]
fn generator_should_keep_candidate_and_output_together_through_serialization() {
    let generator: declaration::Generator = assert_ok!(toml::from_str(
        r#"
inputs = ["src/**/*"]
outputs = [{ path = "generated/app.html", candidate = "bundle/app.html" }]
committed = true
[adapter]
kind = "command"
commands = [{program = "bun", args = ["run", "build"], cwd = "."}]
"#
    ));
    assert_eq!(
        generator.outputs.as_ref(),
        &[declaration::Output {
            path: "generated/app.html".to_owned(),
            candidate: Some("bundle/app.html".to_owned())
        }]
    );
    assert_eq!(
        assert_ok!(toml::from_str::<declaration::Generator>(&assert_ok!(
            toml::to_string(&generator)
        ))),
        generator
    );
}

#[test]
fn validate_should_expand_into_only_executable_steps() {
    assert_eq!(
        Target::Validate.steps(),
        &[graph::Step::Check, graph::Step::Build, graph::Step::Test]
    );
}

#[test]
fn adapter_should_reject_disagreement_with_output_metadata() {
    let adapter = declaration::Adapter::Boltffi {
        platform: declaration::BoltFfiPlatform::Android,
    };
    assert_ok!(adapter.validate_output("boltffi_generate", "android"));
    assert!(
        matches!(assert_err!(adapter.validate_output("facet_generate", "swift")), Error::UnsupportedGenerator { tool, target } if tool == "facet_generate" && target == "swift")
    );
}

#[rstest::rstest]
#[case::empty_inputs("inputs = []\noutputs = [{path = 'out'}]")]
#[case::empty_outputs("inputs = ['src']\noutputs = []")]
fn generator_should_require_inputs_and_outputs(#[case] fields: &str) {
    let text = format!("{fields}\n[adapter]\nkind = 'boltffi'\nplatform = 'android'");
    assert_err!(toml::from_str::<declaration::Generator>(&text));
}

/// A repository-owned generator builds a candidate without rewriting committed output during validation.
#[test]
fn command_should_share_generation_freshness_and_committed_output_checks() {
    let workspace = Workspace::new();
    assert_ok!(std::fs::write(workspace.0.join("committed.txt"), "old"));
    assert_ok!(std::fs::write(
        workspace.0.join("context.toml"),
        r#"
namespace = "ROOT"
purpose = "A repository-owned generator"
type = "crate"
[generated_outputs.preview]
tool = "preview_generator"
target = "text"
[execution.generators.preview]
inputs = ["source.txt"]
outputs = [{path = "committed.txt", candidate = "output.txt"}]
committed = true
[execution.generators.preview.adapter]
kind = "command"
commands = [{program = "fake-generator", cwd = "."}]
"#
    ));
    let git = assert_ok!(
        std::process::Command::new("git")
            .args(["init", "--quiet"])
            .arg(&workspace.0)
            .output()
    );
    assert!(git.status.success());
    let mut fs = RealFileSystem;
    let declarations = assert_ok!(policy_context::declarations(&mut fs, &workspace.0));
    let plan = assert_ok!(graph::generate(
        &declarations,
        Artifact {
            component: ".".into(),
            output: "preview".to_owned()
        }
    ));
    let runner = GeneratorRunner {
        root: workspace.0.clone(),
        calls: AtomicUsize::new(0),
        version: "v1".to_owned(),
        fail: false,
    };
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut context = CommandContext::new(workspace.0.clone(), &mut fs, &mut out, &mut err);
    let mut execute = |explicit| {
        let prepared = prepare(context.fs, &workspace.0, &declarations, plan.clone())?;
        run_with_runner(&mut context, prepared, false, false, explicit, &runner)
    };
    assert!(matches!(assert_err!(execute(false)), Error::Stale(_)));
    assert_eq!(
        assert_ok!(std::fs::read_to_string(workspace.0.join("committed.txt"))),
        "old"
    );
    assert_ok!(execute(true));
    assert_eq!(
        assert_ok!(std::fs::read_to_string(workspace.0.join("committed.txt"))),
        "first"
    );
    assert_ok!(execute(false));
    assert_eq!(runner.calls.load(Ordering::SeqCst), 2);
}
