mod cargo;
mod declarative;
mod fastlane;
mod gradle;
mod kustomize;
pub(crate) mod swift;
pub(crate) mod terraform;

use crate::{CommandRunner, CommandSpec, Verb};
use rapport_cli::{FileSystem, Utf8Path, Utf8PathBuf};
use std::io;

static PROJECT_CONVENTIONS: &[ProjectConvention] = &[
    ProjectConvention::Cargo,
    ProjectConvention::SwiftPackageManager,
    ProjectConvention::Fastlane,
    ProjectConvention::Gradle,
    ProjectConvention::Kustomize,
    ProjectConvention::Terraform,
];

pub(crate) fn describe_expected_markers() -> String {
    let entries = PROJECT_CONVENTIONS
        .iter()
        .flat_map(|convention| {
            convention
                .markers()
                .into_iter()
                .map(move |marker| format!("`{marker}` for {}", convention.name()))
        })
        .collect::<Vec<_>>();
    entries.join(" or ")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectConvention {
    Cargo,
    SwiftPackageManager,
    Fastlane,
    Gradle,
    Kustomize,
    Terraform,
}

impl ProjectConvention {
    fn name(self) -> &'static str {
        match self {
            Self::Cargo => cargo::name(),
            Self::SwiftPackageManager => swift::name(),
            Self::Fastlane => fastlane::name(),
            Self::Gradle => gradle::name(),
            Self::Kustomize => kustomize::name(),
            Self::Terraform => terraform::name(),
        }
    }

    fn markers(self) -> Vec<&'static str> {
        match self {
            Self::Cargo => cargo::markers(),
            Self::SwiftPackageManager => swift::markers().to_vec(),
            Self::Fastlane => fastlane::markers().to_vec(),
            Self::Gradle => gradle::markers(),
            Self::Kustomize => kustomize::markers().to_vec(),
            Self::Terraform => terraform::markers(),
        }
    }

    fn primary_program(self) -> &'static str {
        match self {
            Self::Cargo => cargo::primary_program(),
            Self::SwiftPackageManager => swift::primary_program(),
            Self::Fastlane => fastlane::primary_program(),
            Self::Gradle => gradle::primary_program(),
            Self::Kustomize => kustomize::primary_program(),
            Self::Terraform => terraform::primary_program(),
        }
    }

    fn direct_formatter_program(self) -> Option<&'static str> {
        match self {
            Self::SwiftPackageManager => Some(swift::direct_formatter_program()),
            Self::Cargo | Self::Fastlane | Self::Gradle | Self::Kustomize | Self::Terraform => None,
        }
    }

    fn toolchain_install_hint(self) -> Option<&'static str> {
        match self {
            Self::Cargo => None,
            Self::SwiftPackageManager => Some(swift::toolchain_install_hint()),
            Self::Fastlane => Some(fastlane::toolchain_install_hint()),
            Self::Gradle => Some(gradle::toolchain_install_hint()),
            Self::Kustomize => Some(kustomize::renderer_install_hint()),
            Self::Terraform => Some(terraform::toolchain_install_hint()),
        }
    }

    fn formatter_install_hint(self) -> Option<&'static str> {
        match self {
            Self::SwiftPackageManager => Some(swift::formatter_install_hint()),
            Self::Cargo | Self::Fastlane | Self::Gradle | Self::Kustomize | Self::Terraform => None,
        }
    }

    fn validate_manifest(self, project: &Project, files: &impl FileSystem) -> Result<(), String> {
        match self {
            Self::Cargo => Ok(()),
            Self::SwiftPackageManager => swift::validate_manifest(project, files),
            Self::Fastlane => fastlane::validate_manifest(project, files),
            Self::Gradle => gradle::validate_manifest(project, files),
            Self::Kustomize => kustomize::validate_manifest(project, files),
            Self::Terraform => terraform::validate_manifest(project, files),
        }
    }

    fn fix(
        self,
        project: &Project,
        runner: &dyn CommandRunner,
    ) -> Result<Vec<LifecycleStep>, ToolResolutionError> {
        match self {
            Self::Cargo => Ok(cargo::fix()),
            Self::SwiftPackageManager => swift::fix(project, runner),
            Self::Fastlane => Ok(fastlane::fix()),
            Self::Gradle => Ok(gradle::fix()),
            Self::Kustomize => Ok(kustomize::fix()),
            Self::Terraform => Ok(terraform::fix()),
        }
    }

    fn lint(
        self,
        project: &Project,
        runner: &dyn CommandRunner,
    ) -> Result<Vec<LifecycleStep>, ToolResolutionError> {
        match self {
            Self::Cargo => Ok(cargo::lint()),
            Self::SwiftPackageManager => swift::lint(project, runner),
            Self::Fastlane => Ok(fastlane::lint()),
            Self::Gradle => Ok(gradle::lint()),
            Self::Kustomize => kustomize::lint(project, runner),
            Self::Terraform => terraform::lint(project, runner),
        }
    }

    fn build(
        self,
        project: &Project,
        runner: &dyn CommandRunner,
    ) -> Result<Vec<LifecycleStep>, ToolResolutionError> {
        match self {
            Self::Cargo => Ok(cargo::build()),
            Self::SwiftPackageManager => Ok(swift::build()),
            Self::Fastlane => Ok(fastlane::build()),
            Self::Gradle => Ok(gradle::build()),
            Self::Kustomize => kustomize::build(project, runner),
            Self::Terraform => Ok(terraform::build()),
        }
    }

    fn test(self) -> Vec<LifecycleStep> {
        match self {
            Self::Cargo => cargo::test(),
            Self::SwiftPackageManager => swift::test(),
            Self::Fastlane => fastlane::test(),
            Self::Gradle => gradle::test(),
            Self::Kustomize => kustomize::test(),
            Self::Terraform => terraform::test(),
        }
    }

    fn validate(
        self,
        project: &Project,
        runner: &dyn CommandRunner,
    ) -> Result<Vec<LifecycleStep>, ToolResolutionError> {
        match self {
            Self::Gradle => Ok(gradle::validate()),
            _ => project.validate_steps(runner),
        }
    }

    fn audit(
        self,
        project: &Project,
        runner: &dyn CommandRunner,
    ) -> Result<Vec<LifecycleStep>, ToolResolutionError> {
        match self {
            Self::Fastlane => Ok(fastlane::audit()),
            Self::Gradle => Ok(gradle::audit()),
            Self::Cargo | Self::SwiftPackageManager | Self::Kustomize | Self::Terraform => {
                let mut steps = project.validate_steps(runner)?;
                steps.extend(match self {
                    Self::Cargo => cargo::audit(),
                    Self::SwiftPackageManager => swift::audit(),
                    Self::Kustomize => kustomize::audit(),
                    Self::Terraform => terraform::audit(),
                    Self::Fastlane | Self::Gradle => unreachable!(),
                });
                Ok(steps)
            }
        }
    }

    fn matching_marker(self, root: &Utf8Path, files: &impl FileSystem) -> Option<&'static str> {
        match self {
            Self::Terraform => terraform::matching_marker(root, files),
            Self::Cargo
            | Self::SwiftPackageManager
            | Self::Fastlane
            | Self::Gradle
            | Self::Kustomize => self
                .markers()
                .into_iter()
                .find(|marker| files.is_file(root.join(marker))),
        }
    }

    fn discovers_nested_targets(self) -> bool {
        matches!(self, Self::Kustomize | Self::Terraform)
    }

    fn should_skip_discovery_directory(self, name: &str) -> bool {
        match self {
            Self::Terraform => terraform::should_skip_directory(name),
            Self::Cargo
            | Self::SwiftPackageManager
            | Self::Fastlane
            | Self::Gradle
            | Self::Kustomize => false,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Project {
    convention: ProjectConvention,
    marker: &'static str,
    pub(crate) root: Utf8PathBuf,
}

impl Project {
    pub(crate) fn discover_all(
        start: &Utf8Path,
        files: &impl FileSystem,
    ) -> Result<Vec<Self>, DiscoveryError> {
        let mut current = absolute_path(start)?;
        let mut nearest_project = None;

        loop {
            for convention in PROJECT_CONVENTIONS {
                if nearest_project.is_none()
                    && let Some(marker) = convention.matching_marker(&current, files)
                {
                    nearest_project = Some(Self {
                        convention: *convention,
                        marker,
                        root: current.clone(),
                    });
                }
            }
            if files.exists(current.join(".git")) {
                if let Some(project) = nearest_project {
                    return Ok(vec![project]);
                }

                let root = absolute_path(start)?;
                let mut projects = Vec::new();
                discover_nested_targets(&root, files, &mut projects)?;
                return if projects.is_empty() {
                    Err(DiscoveryError::NoSupportedProject {
                        start: start.to_owned(),
                        git_root: current,
                    })
                } else {
                    Ok(projects)
                };
            }
            if !current.pop() {
                return Err(DiscoveryError::OutsideGitRepository {
                    start: start.to_owned(),
                });
            }
        }
    }

    pub(crate) fn marker(&self) -> &'static str {
        self.marker
    }

    pub(crate) fn manifest_path(&self) -> Utf8PathBuf {
        self.root.join(self.marker())
    }

    pub(crate) fn is_swift_package_manager(&self) -> bool {
        self.convention == ProjectConvention::SwiftPackageManager
    }

    pub(crate) fn is_gradle(&self) -> bool {
        self.convention == ProjectConvention::Gradle
    }

    pub(crate) fn should_report_failure_context(&self) -> bool {
        self.convention != ProjectConvention::Cargo
    }

    pub(crate) fn label(&self) -> String {
        format!("{} project: {}", self.convention.name(), self.root)
    }

    pub(crate) fn toolchain_install_hint(&self) -> Option<&'static str> {
        self.convention.toolchain_install_hint()
    }

    pub(crate) fn formatter_install_hint(&self) -> Option<&'static str> {
        self.convention.formatter_install_hint()
    }

    pub(crate) fn primary_program(&self) -> &'static str {
        self.convention.primary_program()
    }

    pub(crate) fn direct_formatter_program(&self) -> Option<&'static str> {
        self.convention.direct_formatter_program()
    }

    pub(crate) fn lifecycle_steps(
        &self,
        verb: Verb,
        runner: &dyn CommandRunner,
    ) -> Result<Vec<LifecycleStep>, ToolResolutionError> {
        match verb {
            Verb::Fix => self.convention.fix(self, runner),
            Verb::Lint => self.convention.lint(self, runner),
            Verb::Build => self.convention.build(self, runner),
            Verb::Test => Ok(self.convention.test()),
            Verb::Validate => self.convention.validate(self, runner),
            Verb::Audit => self.convention.audit(self, runner),
        }
    }

    pub(crate) fn validate_manifest(&self, files: &impl FileSystem) -> Result<(), String> {
        self.convention.validate_manifest(self, files)
    }

    fn validate_steps(
        &self,
        runner: &dyn CommandRunner,
    ) -> Result<Vec<LifecycleStep>, ToolResolutionError> {
        if self.convention == ProjectConvention::Fastlane {
            return Ok(fastlane::validate());
        }
        let mut steps = self.convention.lint(self, runner)?;
        steps.extend(self.convention.build(self, runner)?);
        steps.extend(self.convention.test());
        Ok(steps)
    }

    pub(crate) fn curate_failure_output(&self, output: &str) -> String {
        match self.convention {
            ProjectConvention::Gradle => gradle::curate_failure_output(output),
            _ => output.to_owned(),
        }
    }
}

fn discover_nested_targets(
    root: &Utf8Path,
    files: &impl FileSystem,
    projects: &mut Vec<Project>,
) -> Result<(), DiscoveryError> {
    if should_skip_discovery_directory(root) {
        return Ok(());
    }

    for convention in PROJECT_CONVENTIONS {
        if convention.discovers_nested_targets()
            && let Some(marker) = convention.matching_marker(root, files)
        {
            projects.push(Project {
                convention: *convention,
                marker,
                root: root.to_owned(),
            });
            return Ok(());
        }
    }

    for entry in files
        .read_dir(root)
        .map_err(|err| DiscoveryError::UnreadableDirectory {
            path: root.to_owned(),
            err,
        })?
    {
        if files.is_dir(&entry) {
            discover_nested_targets(&entry, files, projects)?;
        }
    }
    Ok(())
}

fn should_skip_discovery_directory(root: &Utf8Path) -> bool {
    let Some(name) = root.file_name() else {
        return false;
    };
    name == ".git"
        || PROJECT_CONVENTIONS
            .iter()
            .any(|convention| convention.should_skip_discovery_directory(name))
}

#[derive(Debug)]
pub(crate) enum ToolResolutionError {
    MissingSwift(io::Error),
    MissingFormatter,
    MissingKustomizeRenderer,
    MissingKubernetesValidator,
    MissingTflint,
    ProbeInvoke {
        program: &'static str,
        err: io::Error,
    },
}

#[derive(Debug)]
pub(crate) enum DiscoveryError {
    NoSupportedProject {
        start: Utf8PathBuf,
        git_root: Utf8PathBuf,
    },
    NonUtf8Start {
        path: String,
    },
    OutsideGitRepository {
        start: Utf8PathBuf,
    },
    UnreadableStart {
        path: Utf8PathBuf,
        err: io::Error,
    },
    UnreadableDirectory {
        path: Utf8PathBuf,
        err: io::Error,
    },
}

fn absolute_path(path: &Utf8Path) -> Result<Utf8PathBuf, DiscoveryError> {
    let absolute = std::path::absolute(path).map_err(|err| DiscoveryError::UnreadableStart {
        path: path.to_owned(),
        err,
    })?;
    Utf8PathBuf::from_path_buf(absolute).map_err(|path| DiscoveryError::NonUtf8Start {
        path: path.display().to_string(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, derive_more::Display)]
pub(crate) enum Phase {
    #[display("fix")]
    Fix,
    #[display("format")]
    Format,
    #[display("lint")]
    Lint,
    #[display("build")]
    Build,
    #[display("test")]
    Test,
    #[display("validate")]
    Validate,
    #[display("audit")]
    Audit,
    #[display("release build")]
    ReleaseBuild,
    #[display("docs")]
    Docs,
}

#[derive(Debug, Clone)]
pub(crate) struct LifecycleStep {
    pub(crate) phase: Phase,
    pub(crate) action: LifecycleAction,
}

impl LifecycleStep {
    fn command(phase: Phase, spec: CommandSpec) -> Self {
        Self {
            phase,
            action: LifecycleAction::Command(spec),
        }
    }

    fn message(phase: Phase, message: impl Into<String>) -> Self {
        Self {
            phase,
            action: LifecycleAction::Message(message.into()),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum LifecycleAction {
    Command(CommandSpec),
    Message(String),
}

fn lifecycle_step<I, S>(phase: Phase, program: impl Into<String>, args: I) -> LifecycleStep
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    LifecycleStep::command(phase, CommandSpec::new(program, args))
}

fn message_step(phase: Phase, message: impl Into<String>) -> LifecycleStep {
    LifecycleStep::message(phase, message)
}
