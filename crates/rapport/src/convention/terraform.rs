use super::{LifecycleStep, Phase, Project, ToolResolutionError, lifecycle_step, message_step};
use crate::{CommandRunner, CommandSpec};
use rapport_cli::{FileSystem, Utf8Path};
use std::io;

const TERRAFORM: &str = "terraform";
const TFLINT: &str = "tflint";
const TFLINT_CONFIG: &str = ".tflint.hcl";
const MARKERS: [&str; 1] = ["*.tf"];

pub(super) fn name() -> &'static str {
    "Terraform"
}

pub(super) fn markers() -> &'static [&'static str] {
    &MARKERS
}

pub(super) fn primary_program() -> &'static str {
    TERRAFORM
}

pub(super) fn toolchain_install_hint() -> &'static str {
    "Install Terraform from https://developer.hashicorp.com/terraform/install and make sure `terraform` is on PATH."
}

pub(crate) fn tflint_install_hint() -> &'static str {
    "Install TFLint from https://github.com/terraform-linters/tflint and make sure `tflint` is on PATH."
}

pub(super) fn matching_marker(root: &Utf8Path, files: &impl FileSystem) -> Option<&'static str> {
    if is_generated_or_cache_path(root) {
        return None;
    }
    contains_terraform_file(root, files)
        .ok()
        .and_then(|found| found.then_some(MARKERS[0]))
}

pub(super) fn validate_manifest(project: &Project, files: &impl FileSystem) -> Result<(), String> {
    contains_terraform_file(&project.root, files)
        .map_err(|err| format!("Failed to inspect Terraform project: {err}"))
        .and_then(|found| {
            found
                .then_some(())
                .ok_or_else(|| "Terraform project must contain at least one `*.tf` file.".into())
        })
}

pub(super) fn fix() -> Vec<LifecycleStep> {
    vec![terraform_step(Phase::Format, ["fmt", "-recursive"])]
}

pub(super) fn lint(
    project: &Project,
    runner: &dyn CommandRunner,
) -> Result<Vec<LifecycleStep>, ToolResolutionError> {
    let mut steps = vec![terraform_step(
        Phase::Format,
        ["fmt", "-check", "-recursive"],
    )];
    if should_run_tflint(project, runner)? {
        steps.push(lifecycle_step(Phase::Lint, TFLINT, ["--recursive"]));
    }
    Ok(steps)
}

pub(super) fn build() -> Vec<LifecycleStep> {
    vec![terraform_step(Phase::Build, ["validate"])]
}

pub(super) fn test() -> Vec<LifecycleStep> {
    vec![message_step(
        Phase::Test,
        "No Terraform tests configured for this project.",
    )]
}

pub(super) fn audit() -> Vec<LifecycleStep> {
    Vec::new()
}

pub(super) fn is_generated_or_cache_path(path: &Utf8Path) -> bool {
    path.components().any(|component| {
        matches!(
            component.as_str(),
            ".terraform" | ".terragrunt-cache" | ".terraform-cache"
        )
    })
}

fn contains_terraform_file(root: &Utf8Path, files: &impl FileSystem) -> io::Result<bool> {
    Ok(files.read_dir(root)?.into_iter().any(|entry| {
        files.is_file(&entry) && entry.extension().is_some_and(|extension| extension == "tf")
    }))
}

fn should_run_tflint(
    project: &Project,
    runner: &(impl CommandRunner + ?Sized),
) -> Result<bool, ToolResolutionError> {
    let required = project.root.join(TFLINT_CONFIG).exists();
    match runner.run(&CommandSpec::new(TFLINT, ["--version"]), &project.root) {
        Ok(outcome) => {
            if outcome.success {
                Ok(true)
            } else if required {
                Err(ToolResolutionError::MissingTflint)
            } else {
                Ok(false)
            }
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            if required {
                Err(ToolResolutionError::MissingTflint)
            } else {
                Ok(false)
            }
        }
        Err(err) => Err(ToolResolutionError::ProbeInvoke {
            program: TFLINT,
            err,
        }),
    }
}

fn terraform_step<const N: usize>(phase: Phase, args: [&'static str; N]) -> LifecycleStep {
    lifecycle_step(phase, TERRAFORM, args)
}
