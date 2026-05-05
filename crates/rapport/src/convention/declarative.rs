use super::{LifecycleStep, Phase, lifecycle_step, message_step};
use crate::Verb;
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ConventionDefinition {
    name: String,
    markers: Vec<String>,
    primary_program: String,
    toolchain_install_hint: Option<String>,
    #[serde(default)]
    skip_directories: Vec<String>,
    verbs: VerbDefinitions,
    #[serde(default)]
    tools: BTreeMap<String, ToolDefinition>,
}

impl ConventionDefinition {
    pub(super) fn parse(name: &str, contents: &str) -> Self {
        match toml_edit::de::from_str(contents) {
            Ok(definition) => definition,
            Err(err) => panic!("failed to parse convention definition {name}: {err}"),
        }
    }

    pub(super) fn name(&'static self) -> &'static str {
        &self.name
    }

    pub(super) fn markers(&'static self) -> Vec<&'static str> {
        self.markers.iter().map(String::as_str).collect()
    }

    pub(super) fn primary_program(&'static self) -> &'static str {
        &self.primary_program
    }

    pub(super) fn toolchain_install_hint(&'static self) -> Option<&'static str> {
        self.toolchain_install_hint.as_deref()
    }

    pub(super) fn steps(&'static self, verb: Verb) -> Vec<LifecycleStep> {
        self.verb_definition(verb)
            .steps
            .iter()
            .map(StepDefinition::lifecycle_step)
            .collect()
    }

    pub(super) fn should_skip_directory(&self, name: &str) -> bool {
        self.skip_directories
            .iter()
            .any(|candidate| candidate == name)
    }

    pub(super) fn tool(&'static self, name: &str) -> Option<&'static ToolDefinition> {
        self.tools.get(name)
    }

    fn verb_definition(&self, verb: Verb) -> &VerbDefinition {
        match verb {
            Verb::Fix => &self.verbs.fix,
            Verb::Lint => &self.verbs.lint,
            Verb::Build => &self.verbs.build,
            Verb::Test => &self.verbs.test,
            Verb::Validate => panic!("validate is composed by rapport, not declared in TOML"),
            Verb::Audit => &self.verbs.audit,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ToolDefinition {
    program: String,
    version_args: Vec<String>,
    run_args: Vec<String>,
    required_config: Option<String>,
    install_hint: String,
}

impl ToolDefinition {
    pub(super) fn program(&'static self) -> &'static str {
        &self.program
    }

    pub(super) fn version_args(&self) -> Vec<String> {
        self.version_args.clone()
    }

    pub(super) fn run_args(&self) -> Vec<String> {
        self.run_args.clone()
    }

    pub(super) fn required_config(&self) -> Option<&str> {
        self.required_config.as_deref()
    }

    pub(super) fn install_hint(&'static self) -> &'static str {
        &self.install_hint
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VerbDefinitions {
    #[serde(default)]
    fix: VerbDefinition,
    #[serde(default)]
    lint: VerbDefinition,
    #[serde(default)]
    build: VerbDefinition,
    #[serde(default)]
    test: VerbDefinition,
    #[serde(default)]
    audit: VerbDefinition,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct VerbDefinition {
    #[serde(default)]
    steps: Vec<StepDefinition>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StepDefinition {
    phase: PhaseDefinition,
    program: Option<String>,
    args: Option<Vec<String>>,
    message: Option<String>,
}

impl StepDefinition {
    fn lifecycle_step(&self) -> LifecycleStep {
        let phase = self.phase.lifecycle_phase();
        match (&self.program, &self.message) {
            (Some(program), None) => lifecycle_step(
                phase,
                program.clone(),
                self.args.clone().unwrap_or_default(),
            ),
            (None, Some(message)) => message_step(phase, message.clone()),
            (Some(_), Some(_)) => panic!("convention step must not set both program and message"),
            (None, None) => panic!("convention step must set program or message"),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum PhaseDefinition {
    Fix,
    Format,
    Lint,
    Build,
    Test,
    Validate,
    Audit,
    #[serde(rename = "release build")]
    ReleaseBuild,
    Docs,
}

impl PhaseDefinition {
    fn lifecycle_phase(&self) -> Phase {
        match self {
            Self::Fix => Phase::Fix,
            Self::Format => Phase::Format,
            Self::Lint => Phase::Lint,
            Self::Build => Phase::Build,
            Self::Test => Phase::Test,
            Self::Validate => Phase::Validate,
            Self::Audit => Phase::Audit,
            Self::ReleaseBuild => Phase::ReleaseBuild,
            Self::Docs => Phase::Docs,
        }
    }
}
