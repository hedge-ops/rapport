use super::{DoctorCheck, LifecycleStep, Project, declarative::ConventionDefinition};
use crate::{CommandRunner, Verb};
use rapport_cli::FileSystem;
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

pub(super) fn fix() -> Vec<LifecycleStep> {
    definition().steps(Verb::Fix)
}

pub(super) fn lint() -> Vec<LifecycleStep> {
    definition().steps(Verb::Lint)
}

pub(super) fn build() -> Vec<LifecycleStep> {
    definition().steps(Verb::Build)
}

pub(super) fn test() -> Vec<LifecycleStep> {
    definition().steps(Verb::Test)
}

pub(super) fn audit() -> Vec<LifecycleStep> {
    definition().steps(Verb::Audit)
}

const FORMAT_VERBS: [Verb; 4] = [Verb::Fix, Verb::Lint, Verb::Validate, Verb::Audit];
const LINT_VALIDATE_AUDIT: [Verb; 3] = [Verb::Lint, Verb::Validate, Verb::Audit];

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
