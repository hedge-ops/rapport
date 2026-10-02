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
            inputs: vec!["source.txt".to_owned()],
            outputs: vec!["output.txt".to_owned()],
            destination: "output.txt".to_owned(),
            candidates: vec![],
            environment: vec![],
            package: None,
            locales: None,
            format_config: None,
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
