use super::{LifecycleStep, declarative::ConventionDefinition};
use crate::Verb;
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
