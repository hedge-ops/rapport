//! Order explicit component operations after their generated inputs.

use crate::policy_context::ComponentContext;
use rapport_files::{Utf8Path, Utf8PathBuf};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum Target {
    Check,
    Build,
    Test,
    Validate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    Check,
    Build,
    Test,
}
impl Target {
    pub(crate) fn steps(self) -> &'static [Step] {
        match self {
            Self::Check => &[Step::Check],
            Self::Build => &[Step::Build],
            Self::Test => &[Step::Test],
            Self::Validate => &[Step::Check, Step::Build, Step::Test],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Artifact {
    pub(crate) component: Utf8PathBuf,
    pub(crate) output: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Operation {
    Generate(Artifact),
    Run {
        component: Utf8PathBuf,
        target: Target,
    },
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("no direct component context at `{0}`")]
    MissingComponent(Utf8PathBuf),
    #[error("component `{component}` does not declare output `{output}`")]
    MissingOutput {
        component: Utf8PathBuf,
        output: String,
    },
    #[error("component membership cycle at `{0}`")]
    MembershipCycle(Utf8PathBuf),
    #[error("group `{0}` has no declared component members")]
    EmptyGroup(Utf8PathBuf),
}

/// Plan all selections before allowing any tool execution.
pub(crate) fn plan(
    contexts: &BTreeMap<Utf8PathBuf, ComponentContext>,
    paths: &[Utf8PathBuf],
    target: Target,
) -> Result<Vec<Operation>, Error> {
    let mut planner = Planner::new(contexts);
    for path in paths {
        planner.component(path, target)?;
    }
    Ok(planner.operations)
}

pub(crate) fn generate(
    contexts: &BTreeMap<Utf8PathBuf, ComponentContext>,
    artifact: Artifact,
) -> Result<Vec<Operation>, Error> {
    let mut planner = Planner::new(contexts);
    planner.generate(artifact)?;
    Ok(planner.operations)
}

struct Planner<'a> {
    contexts: &'a BTreeMap<Utf8PathBuf, ComponentContext>,
    artifacts: BTreeSet<Artifact>,
    components: BTreeSet<Utf8PathBuf>,
    visiting: BTreeSet<Utf8PathBuf>,
    operations: Vec<Operation>,
}

impl<'a> Planner<'a> {
    fn new(contexts: &'a BTreeMap<Utf8PathBuf, ComponentContext>) -> Self {
        Self {
            contexts,
            artifacts: BTreeSet::new(),
            components: BTreeSet::new(),
            visiting: BTreeSet::new(),
            operations: Vec::new(),
        }
    }

    fn context(&self, path: &Utf8Path) -> Result<&'a ComponentContext, Error> {
        self.contexts
            .get(path)
            .ok_or_else(|| Error::MissingComponent(path.to_path_buf()))
    }

    fn inputs(&mut self, path: &Utf8Path) -> Result<(), Error> {
        let inputs = self.context(path)?.generated_inputs();
        for input in inputs.values() {
            self.generate(Artifact {
                component: input.component().as_path().to_path_buf(),
                output: input.output().as_str().to_owned(),
            })?;
        }
        Ok(())
    }

    fn generate(&mut self, artifact: Artifact) -> Result<(), Error> {
        if self.artifacts.contains(&artifact) {
            return Ok(());
        }
        let producer = self.context(&artifact.component)?;
        if !producer
            .generated_outputs()
            .keys()
            .any(|name| name.as_str() == artifact.output)
        {
            return Err(Error::MissingOutput {
                component: artifact.component,
                output: artifact.output,
            });
        }
        // Context loading already rejects generated dependency cycles.
        self.inputs(&artifact.component)?;
        self.artifacts.insert(artifact.clone());
        self.operations.push(Operation::Generate(artifact));
        Ok(())
    }

    fn component(&mut self, path: &Utf8Path, target: Target) -> Result<(), Error> {
        if self.components.contains(path) {
            return Ok(());
        }
        if !self.visiting.insert(path.to_path_buf()) {
            return Err(Error::MembershipCycle(path.to_path_buf()));
        }
        let context = self.context(path)?;
        self.inputs(path)?;
        if target == Target::Validate
            && let Some(execution) = context.execution()
        {
            for (name, generator) in &execution.generators {
                if generator.committed {
                    self.generate(Artifact {
                        component: path.to_path_buf(),
                        output: name.clone(),
                    })?;
                }
            }
        }
        if context.component_type() == Some("group") {
            if context.components().is_empty() {
                return Err(Error::EmptyGroup(path.to_path_buf()));
            }
            for member in context.components() {
                self.component(member.as_path(), target)?;
            }
        } else {
            self.operations.push(Operation::Run {
                component: path.to_path_buf(),
                target,
            });
        }
        self.visiting.remove(path);
        self.components.insert(path.to_path_buf());
        Ok(())
    }
}
