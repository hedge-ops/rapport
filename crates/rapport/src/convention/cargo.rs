use super::{
    DoctorCheck, LifecycleStep, Phase, Project, declarative::ConventionDefinition, lifecycle_step,
};
use crate::{CommandRunner, CommandSpec, Verb};
use rapport_cli::FileSystem;
use serde::Deserialize;
use std::sync::LazyLock;

static DEFINITION: LazyLock<ConventionDefinition> =
    LazyLock::new(|| ConventionDefinition::parse("cargo", include_str!("definitions/cargo.toml")));

fn definition() -> &'static ConventionDefinition {
    &DEFINITION
}

pub(super) fn name() -> &'static str {
    definition().name()
}

pub(super) fn markers() -> Vec<&'static str> {
    definition().markers()
}

pub(super) fn primary_program() -> &'static str {
    definition().primary_program()
}

pub(super) fn validate_manifest(project: &Project, files: &impl FileSystem) -> Result<(), String> {
    cargo_plan(project, files).map(|_| ())
}

pub(super) fn fix(
    project: &Project,
    files: &impl FileSystem,
) -> Result<Vec<LifecycleStep>, String> {
    let plan = cargo_plan(project, files)?;
    Ok(vec![lifecycle_step(
        Phase::Format,
        "cargo",
        plan.format_args("fmt"),
    )])
}

pub(super) fn lint(
    project: &Project,
    files: &impl FileSystem,
) -> Result<Vec<LifecycleStep>, String> {
    let plan = cargo_plan(project, files)?;
    Ok(vec![
        lifecycle_step(
            Phase::Format,
            "cargo",
            plan.format_args_with_rustfmt("fmt", ["--check"]),
        ),
        lifecycle_step(
            Phase::Lint,
            "cargo",
            plan.compile_args("clippy", ["--all-targets"]),
        ),
    ])
}

pub(super) fn build(
    project: &Project,
    files: &impl FileSystem,
) -> Result<Vec<LifecycleStep>, String> {
    let plan = cargo_plan(project, files)?;
    Ok(vec![lifecycle_step(
        Phase::Build,
        "cargo",
        plan.compile_args("check", std::iter::empty::<&str>()),
    )])
}

pub(super) fn test(
    project: &Project,
    runner: &dyn CommandRunner,
    files: &impl FileSystem,
) -> Result<Vec<LifecycleStep>, String> {
    let plan = cargo_plan(project, files)?;
    let args = if cargo_nextest_is_available(project, runner) {
        plan.nextest_args()
    } else {
        plan.compile_args("test", std::iter::empty::<&str>())
    };
    Ok(vec![lifecycle_step(Phase::Test, "cargo", args)])
}

pub(super) fn audit(
    project: &Project,
    files: &impl FileSystem,
) -> Result<Vec<LifecycleStep>, String> {
    let plan = cargo_plan(project, files)?;
    Ok(vec![
        lifecycle_step(
            Phase::ReleaseBuild,
            "cargo",
            plan.compile_args("build", ["--release"]),
        ),
        lifecycle_step(
            Phase::Docs,
            "cargo",
            plan.compile_args("doc", ["--no-deps"]),
        ),
    ])
}

const FORMAT_VERBS: [Verb; 4] = [Verb::Fix, Verb::Lint, Verb::Validate, Verb::Audit];
const LINT_VALIDATE_AUDIT: [Verb; 3] = [Verb::Lint, Verb::Validate, Verb::Audit];
const TEST_VALIDATE_AUDIT: [Verb; 3] = [Verb::Test, Verb::Validate, Verb::Audit];

pub(super) fn doctor_checks(
    project: &Project,
    runner: &dyn CommandRunner,
    files: &impl FileSystem,
) -> (Vec<DoctorCheck>, Vec<DoctorCheck>) {
    let tools = vec![
        super::tool_check(
            project,
            runner,
            "cargo",
            "cargo",
            ["--version"],
            &super::ALL_VERBS,
            Some(
                "Install Rust from https://www.rust-lang.org/tools/install and make sure `cargo` is on PATH.",
            ),
        ),
        super::tool_check(
            project,
            runner,
            "cargo fmt",
            "cargo",
            ["fmt", "--version"],
            &FORMAT_VERBS,
            Some(
                "Install rustfmt with your Rust toolchain, for example `rustup component add rustfmt`.",
            ),
        ),
        super::tool_check(
            project,
            runner,
            "cargo clippy",
            "cargo",
            ["clippy", "--version"],
            &LINT_VALIDATE_AUDIT,
            Some(
                "Install Clippy with your Rust toolchain, for example `rustup component add clippy`.",
            ),
        ),
        nextest_check(project, runner),
    ];
    let configuration = vec![super::file_check(
        files,
        &project.root.join("Cargo.toml"),
        "Cargo.toml",
        &super::ALL_VERBS,
        "Add a `Cargo.toml` manifest at the Cargo project root.",
    )];
    (tools, configuration)
}

fn nextest_check(project: &Project, runner: &dyn CommandRunner) -> DoctorCheck {
    let spec = CommandSpec::new("cargo", ["nextest", "--version"]);
    let probe = super::format_command(&spec);
    match runner.run(&spec, &project.root) {
        Ok(outcome) if outcome.success => DoctorCheck::pass(
            "cargo nextest",
            "usable on PATH",
            &TEST_VALIDATE_AUDIT,
            Some(probe),
        ),
        Ok(_) => DoctorCheck::warn(
            "cargo nextest",
            "not usable; rapport will fall back to cargo test",
            &TEST_VALIDATE_AUDIT,
            Some(probe),
            nextest_install_hint(),
        ),
        Err(err) => DoctorCheck::warn(
            "cargo nextest",
            format!("failed to invoke probe: {err}; rapport will fall back to cargo test"),
            &TEST_VALIDATE_AUDIT,
            Some(probe),
            nextest_install_hint(),
        ),
    }
}

fn cargo_nextest_is_available(project: &Project, runner: &dyn CommandRunner) -> bool {
    let spec = CommandSpec::new("cargo", ["nextest", "--version"]);
    runner
        .run(&spec, &project.root)
        .is_ok_and(|outcome| outcome.success)
}

fn nextest_install_hint() -> &'static str {
    "Install cargo-nextest with `cargo install cargo-nextest`, or let rapport use its documented `cargo test` fallback."
}

#[derive(Debug, Clone)]
struct CargoPlan {
    scope: CargoScope,
    options: CargoOptions,
}

impl CargoPlan {
    fn format_args(&self, command: &str) -> Vec<String> {
        let mut args = vec![command.to_owned()];
        args.extend(self.scope.format_args());
        args
    }

    fn format_args_with_rustfmt<I, S>(&self, command: &str, rustfmt_args: I) -> Vec<String>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut args = self.format_args(command);
        args.push("--".into());
        args.extend(rustfmt_args.into_iter().map(Into::into));
        args
    }

    fn compile_args<I, S>(&self, command: &str, extra_args: I) -> Vec<String>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut args = vec![command.to_owned()];
        args.extend(self.scope.compile_args());
        args.extend(extra_args.into_iter().map(Into::into));
        args.extend(self.options.compile_args());
        if command == "clippy" {
            args.extend(["--".into(), "-D".into(), "warnings".into()]);
        }
        args
    }

    fn nextest_args(&self) -> Vec<String> {
        let mut args = vec!["nextest".into(), "run".into()];
        args.extend(self.scope.compile_args());
        args.extend(self.options.compile_args());
        args
    }
}

#[derive(Debug, Clone)]
enum CargoScope {
    Workspace,
    Package(String),
}

impl CargoScope {
    fn format_args(&self) -> Vec<String> {
        match self {
            Self::Workspace => vec!["--all".into()],
            Self::Package(name) => vec!["--package".into(), name.clone()],
        }
    }

    fn compile_args(&self) -> Vec<String> {
        match self {
            Self::Workspace => vec!["--workspace".into()],
            Self::Package(name) => vec!["--package".into(), name.clone()],
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct CargoOptions {
    #[serde(default)]
    features: Vec<String>,
    #[serde(default)]
    all_features: bool,
    #[serde(default)]
    no_default_features: bool,
    target: Option<String>,
}

impl CargoOptions {
    fn compile_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if self.no_default_features {
            args.push("--no-default-features".into());
        }
        if self.all_features {
            args.push("--all-features".into());
        }
        if !self.features.is_empty() {
            args.extend(["--features".into(), self.features.join(",")]);
        }
        if let Some(target) = &self.target {
            args.extend(["--target".into(), target.clone()]);
        }
        args
    }

    fn validate(&self) -> Result<(), String> {
        if self.all_features && !self.features.is_empty() {
            return Err(
                "Cargo rapport metadata must not set both `all-features` and `features`.".into(),
            );
        }
        if self
            .features
            .iter()
            .any(|feature| feature.trim().is_empty())
        {
            return Err("Cargo rapport metadata `features` entries must not be empty.".into());
        }
        if self
            .target
            .as_ref()
            .is_some_and(|target| target.trim().is_empty())
        {
            return Err("Cargo rapport metadata `target` must not be empty.".into());
        }
        Ok(())
    }
}

#[derive(Debug, Default, Deserialize)]
struct CargoManifest {
    package: Option<CargoPackage>,
    workspace: Option<CargoWorkspace>,
}

#[derive(Debug, Deserialize)]
struct CargoPackage {
    name: Option<String>,
    metadata: Option<CargoMetadata>,
}

#[derive(Debug, Deserialize)]
struct CargoWorkspace {
    metadata: Option<CargoMetadata>,
}

#[derive(Debug, Deserialize)]
struct CargoMetadata {
    rapport: Option<RapportMetadata>,
}

#[derive(Debug, Deserialize)]
struct RapportMetadata {
    cargo: Option<CargoOptions>,
}

fn cargo_plan(project: &Project, files: &impl FileSystem) -> Result<CargoPlan, String> {
    let manifest = parse_manifest(project, files)?;
    let (scope, options) = if let Some(workspace) = manifest.workspace {
        (
            CargoScope::Workspace,
            cargo_options(workspace.metadata.as_ref()),
        )
    } else if let Some(package) = manifest.package {
        let name = package
            .name
            .clone()
            .ok_or_else(|| "Cargo package manifest must include `package.name`.".to_owned())?;
        (
            CargoScope::Package(name),
            cargo_options(package.metadata.as_ref()),
        )
    } else {
        return Err("Cargo.toml must contain either `[package]` or `[workspace]`.".into());
    };
    options.validate()?;
    Ok(CargoPlan { scope, options })
}

fn parse_manifest(project: &Project, files: &impl FileSystem) -> Result<CargoManifest, String> {
    let path = project.manifest_path();
    let contents = files
        .read_to_string(&path)
        .map_err(|err| format!("Failed to read Cargo.toml: {err}"))?;
    toml_edit::de::from_str(&contents).map_err(|err| format!("Failed to parse Cargo.toml: {err}"))
}

fn cargo_options(metadata: Option<&CargoMetadata>) -> CargoOptions {
    metadata
        .and_then(|metadata| metadata.rapport.as_ref())
        .and_then(|rapport| rapport.cargo.clone())
        .unwrap_or_default()
}
