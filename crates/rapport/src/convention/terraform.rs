use super::{
    DoctorCheck, DoctorStatus, LifecycleStep, Project, ToolResolutionError,
    declarative::ConventionDefinition,
};
use crate::{CommandRunner, CommandSpec, Verb};
use rapport_cli::{FileSystem, Utf8Path};
use std::io;
use std::sync::LazyLock;

const TFLINT: &str = "tflint";

static DEFINITION: LazyLock<ConventionDefinition> = LazyLock::new(|| {
    ConventionDefinition::parse("terraform", include_str!("definitions/terraform.toml"))
});

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

pub(super) fn toolchain_install_hint() -> &'static str {
    definition()
        .toolchain_install_hint()
        .unwrap_or("Install Terraform and make sure `terraform` is on PATH.")
}

pub(crate) fn tflint_install_hint() -> &'static str {
    tflint_tool().install_hint()
}

pub(super) fn matching_marker(root: &Utf8Path, files: &impl FileSystem) -> Option<&'static str> {
    if is_generated_or_cache_path(root) {
        return None;
    }
    contains_terraform_file(root, files)
        .ok()
        .and_then(|found| found.then_some(markers()[0]))
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
    definition().steps(crate::Verb::Fix)
}

pub(super) fn lint(
    project: &Project,
    runner: &dyn CommandRunner,
) -> Result<Vec<LifecycleStep>, ToolResolutionError> {
    let mut steps = definition().steps(crate::Verb::Lint);
    if should_run_tflint(project, runner)? {
        let tool = tflint_tool();
        steps.push(super::lifecycle_step(
            super::Phase::Lint,
            tool.program(),
            tool.run_args(),
        ));
    }
    Ok(steps)
}

pub(super) fn build() -> Vec<LifecycleStep> {
    definition().steps(crate::Verb::Build)
}

pub(super) fn test() -> Vec<LifecycleStep> {
    definition().steps(crate::Verb::Test)
}

pub(super) fn audit() -> Vec<LifecycleStep> {
    definition().steps(crate::Verb::Audit)
}

const TERRAFORM_VERBS: [Verb; 5] = [
    Verb::Fix,
    Verb::Lint,
    Verb::Build,
    Verb::Validate,
    Verb::Audit,
];

pub(super) fn doctor_checks(
    project: &Project,
    runner: &dyn CommandRunner,
    files: &impl FileSystem,
) -> (Vec<DoctorCheck>, Vec<DoctorCheck>) {
    let tools = vec![
        super::tool_check(
            project,
            runner,
            "terraform",
            primary_program(),
            ["--version"],
            &TERRAFORM_VERBS,
            Some(toolchain_install_hint()),
        ),
        tflint_check(project, runner, files),
    ];
    let configuration = vec![
        terraform_files_check(project, files),
        super::convention_check(
            validate_manifest(project, files),
            "Terraform convention",
            &TERRAFORM_VERBS,
            "Add at least one `*.tf` file at the Terraform target root.",
        ),
    ];
    (tools, configuration)
}

fn tflint_check(
    project: &Project,
    runner: &dyn CommandRunner,
    files: &impl FileSystem,
) -> DoctorCheck {
    let affects = &[Verb::Lint, Verb::Validate, Verb::Audit];
    let required = files.is_file(project.root.join(".tflint.hcl"));
    let check = super::tool_check(
        project,
        runner,
        "tflint",
        TFLINT,
        ["--version"],
        affects,
        Some(tflint_install_hint()),
    );

    if required || check.status == DoctorStatus::Pass {
        return check;
    }

    DoctorCheck::warn(
        "tflint",
        "not configured and not found; Terraform lint will skip TFLint",
        affects,
        check.probe,
        "Add `.tflint.hcl` and install TFLint when Terraform linting should include TFLint.",
    )
}

fn terraform_files_check(project: &Project, files: &impl FileSystem) -> DoctorCheck {
    match files.read_dir(&project.root) {
        Ok(entries) => {
            let found = entries.into_iter().any(|entry| {
                files.is_file(&entry)
                    && entry.extension().is_some_and(|extension| extension == "tf")
            });
            if found {
                DoctorCheck::pass("Terraform `*.tf` files", "present", &TERRAFORM_VERBS, None)
            } else {
                DoctorCheck::fail(
                    "Terraform `*.tf` files",
                    "missing",
                    &TERRAFORM_VERBS,
                    None,
                    "Add at least one `*.tf` file at the Terraform target root.",
                )
            }
        }
        Err(err) => DoctorCheck::fail(
            "Terraform `*.tf` files",
            format!("failed to inspect target: {err}"),
            &TERRAFORM_VERBS,
            None,
            "Make the Terraform target directory readable.",
        ),
    }
}

pub(super) fn is_generated_or_cache_path(path: &Utf8Path) -> bool {
    path.components()
        .any(|component| should_skip_directory(component.as_str()))
}

pub(super) fn should_skip_directory(name: &str) -> bool {
    definition().should_skip_directory(name)
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
    let tool = tflint_tool();
    let required = tool
        .required_config()
        .is_some_and(|config| project.root.join(config).exists());
    match runner.run(
        &CommandSpec::new(tool.program(), tool.version_args()),
        &project.root,
    ) {
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

fn tflint_tool() -> &'static super::declarative::ToolDefinition {
    match definition().tool(TFLINT) {
        Some(tool) => tool,
        None => panic!("terraform convention definition must include tflint tool metadata"),
    }
}
