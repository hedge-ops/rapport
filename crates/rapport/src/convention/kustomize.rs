use super::{LifecycleStep, Phase, Project, ToolResolutionError, lifecycle_step, message_step};
use crate::{CommandRunner, CommandSpec};
use rapport_cli::FileSystem;
use std::io;

const KUBECTL: &str = "kubectl";
const KUSTOMIZE: &str = "kustomize";
const KUBECONFORM: &str = "kubeconform";
const MARKERS: [&str; 2] = ["kustomization.yaml", "kustomization.yml"];

pub(super) fn name() -> &'static str {
    "Kustomize"
}

pub(super) fn markers() -> &'static [&'static str] {
    &MARKERS
}

pub(super) fn primary_program() -> &'static str {
    KUSTOMIZE
}

pub(super) fn renderer_install_hint() -> &'static str {
    "Install standalone Kustomize, or kubectl with Kustomize support, and make sure `kustomize` or `kubectl` is on PATH."
}

pub(super) fn validate_manifest(project: &Project, files: &impl FileSystem) -> Result<(), String> {
    let marker = project.marker();
    let manifest = project.manifest_path();
    files
        .read_to_string(&manifest)
        .map(|_| ())
        .map_err(|err| format!("Failed to read `{marker}`: {err}"))
}

pub(super) fn fix() -> Vec<LifecycleStep> {
    vec![message_step(
        Phase::Fix,
        "Kustomize has no autofix; leaving manifests unchanged.",
    )]
}

pub(super) fn lint(
    project: &Project,
    runner: &dyn CommandRunner,
) -> Result<Vec<LifecycleStep>, ToolResolutionError> {
    let renderer = resolve_renderer(project, runner)?;
    resolve_validator(project, runner)?;
    Ok(vec![lint_step(renderer)])
}

pub(super) fn build(
    project: &Project,
    runner: &dyn CommandRunner,
) -> Result<Vec<LifecycleStep>, ToolResolutionError> {
    let renderer = resolve_renderer(project, runner)?;
    Ok(vec![render_step(renderer)])
}

pub(super) fn test() -> Vec<LifecycleStep> {
    vec![message_step(
        Phase::Test,
        "No Kubernetes tests configured for this Kustomize target.",
    )]
}

pub(super) fn audit() -> Vec<LifecycleStep> {
    Vec::new()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Renderer {
    Kubectl,
    Standalone,
}

fn resolve_renderer(
    project: &Project,
    runner: &(impl CommandRunner + ?Sized),
) -> Result<Renderer, ToolResolutionError> {
    if probe(project, runner, KUSTOMIZE, ["version"])? {
        return Ok(Renderer::Standalone);
    }
    if probe(project, runner, KUBECTL, ["version", "--client"])? {
        return Ok(Renderer::Kubectl);
    }
    Err(ToolResolutionError::MissingKustomizeRenderer)
}

fn resolve_validator(
    project: &Project,
    runner: &(impl CommandRunner + ?Sized),
) -> Result<(), ToolResolutionError> {
    if probe(project, runner, KUBECONFORM, ["-v"])? {
        Ok(())
    } else {
        Err(ToolResolutionError::MissingKubernetesValidator)
    }
}

fn probe<const N: usize>(
    project: &Project,
    runner: &(impl CommandRunner + ?Sized),
    program: &'static str,
    args: [&'static str; N],
) -> Result<bool, ToolResolutionError> {
    match runner.run(&CommandSpec::new(program, args), &project.root) {
        Ok(outcome) => Ok(outcome.success),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(ToolResolutionError::ProbeInvoke { program, err }),
    }
}

fn render_step(renderer: Renderer) -> LifecycleStep {
    match renderer {
        Renderer::Standalone => lifecycle_step(Phase::Build, KUSTOMIZE, ["build", "."]),
        Renderer::Kubectl => lifecycle_step(Phase::Build, KUBECTL, ["kustomize", "."]),
    }
}

fn lint_step(renderer: Renderer) -> LifecycleStep {
    let build_command = match renderer {
        Renderer::Standalone => "kustomize build .",
        Renderer::Kubectl => "kubectl kustomize .",
    };
    let script = format!(
        "set -e; rendered=\"$({build_command})\"; printf '%s\\n' \"$rendered\" | kubeconform -strict -summary -ignore-missing-schemas -"
    );
    lifecycle_step(Phase::Lint, "/bin/sh", ["-c".to_owned(), script])
}
