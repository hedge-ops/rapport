use super::{LifecycleStep, Phase, lifecycle_step};

pub(super) fn name() -> &'static str {
    "Cargo"
}

pub(super) fn markers() -> &'static [&'static str] {
    &["Cargo.toml"]
}

pub(super) fn primary_program() -> &'static str {
    "cargo"
}

pub(super) fn fix() -> Vec<LifecycleStep> {
    vec![cargo_step(Phase::Format, ["fmt"])]
}

pub(super) fn lint() -> Vec<LifecycleStep> {
    vec![
        cargo_step(Phase::Format, ["fmt", "--", "--check"]),
        cargo_step(
            Phase::Lint,
            ["clippy", "--all-targets", "--", "-D", "warnings"],
        ),
    ]
}

pub(super) fn build() -> Vec<LifecycleStep> {
    vec![cargo_step(Phase::Build, ["check"])]
}

pub(super) fn test() -> Vec<LifecycleStep> {
    vec![cargo_step(Phase::Test, ["test"])]
}

pub(super) fn audit() -> Vec<LifecycleStep> {
    vec![
        cargo_step(Phase::ReleaseBuild, ["build", "--release"]),
        cargo_step(Phase::Docs, ["doc", "--no-deps"]),
    ]
}

fn cargo_step<const N: usize>(phase: Phase, args: [&'static str; N]) -> LifecycleStep {
    lifecycle_step(phase, "cargo", args)
}
