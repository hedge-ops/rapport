mod bun;
mod cargo;
mod declarative;
mod fastlane;
mod gradle;
mod kustomize;
pub(crate) mod swift;
pub(crate) mod terraform;
mod zola;

use crate::{CommandRunner, CommandSpec, Verb};
use rapport_cli::{FileSystem, Utf8Path, Utf8PathBuf};
use std::io;

static PROJECT_CONVENTIONS: &[ProjectConvention] = &[
    ProjectConvention::Cargo,
    ProjectConvention::Zola,
    ProjectConvention::Bun,
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
    Zola,
    Bun,
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
            Self::Zola => zola::name(),
            Self::Bun => bun::name(),
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
            Self::Zola => zola::markers().to_vec(),
            Self::Bun => bun::markers().to_vec(),
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
            Self::Zola => zola::primary_program(),
            Self::Bun => bun::primary_program(),
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
            Self::Cargo
            | Self::Zola
            | Self::Bun
            | Self::Fastlane
            | Self::Gradle
            | Self::Kustomize
            | Self::Terraform => None,
        }
    }

    fn toolchain_install_hint(self) -> Option<&'static str> {
        match self {
            Self::Cargo => None,
            Self::Zola => Some(zola::toolchain_install_hint()),
            Self::Bun => Some(bun::toolchain_install_hint()),
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
            Self::Cargo
            | Self::Zola
            | Self::Bun
            | Self::Fastlane
            | Self::Gradle
            | Self::Kustomize
            | Self::Terraform => None,
        }
    }

    fn validate_manifest(self, project: &Project, files: &impl FileSystem) -> Result<(), String> {
        match self {
            Self::Cargo => Ok(()),
            Self::Zola => zola::validate_manifest(project, files),
            Self::Bun => bun::validate_manifest(project, files),
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
        files: &impl FileSystem,
    ) -> Result<Vec<LifecycleStep>, ToolResolutionError> {
        match self {
            Self::Cargo => Ok(cargo::fix()),
            Self::Zola => zola::fix(project, files).map_err(ToolResolutionError::Convention),
            Self::Bun => bun::fix(project, files).map_err(ToolResolutionError::Convention),
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
        files: &impl FileSystem,
    ) -> Result<Vec<LifecycleStep>, ToolResolutionError> {
        match self {
            Self::Cargo => Ok(cargo::lint()),
            Self::Zola => zola::lint(project, files).map_err(ToolResolutionError::Convention),
            Self::Bun => bun::lint(project, files).map_err(ToolResolutionError::Convention),
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
        files: &impl FileSystem,
    ) -> Result<Vec<LifecycleStep>, ToolResolutionError> {
        match self {
            Self::Cargo => Ok(cargo::build()),
            Self::Zola => zola::build(project, files).map_err(ToolResolutionError::Convention),
            Self::Bun => bun::build(project, files).map_err(ToolResolutionError::Convention),
            Self::SwiftPackageManager => Ok(swift::build()),
            Self::Fastlane => Ok(fastlane::build()),
            Self::Gradle => Ok(gradle::build()),
            Self::Kustomize => kustomize::build(project, runner),
            Self::Terraform => Ok(terraform::build()),
        }
    }

    fn test(
        self,
        project: &Project,
        files: &impl FileSystem,
    ) -> Result<Vec<LifecycleStep>, ToolResolutionError> {
        match self {
            Self::Cargo => Ok(cargo::test()),
            Self::Zola => zola::test(project, files).map_err(ToolResolutionError::Convention),
            Self::Bun => bun::test(project, files).map_err(ToolResolutionError::Convention),
            Self::SwiftPackageManager => Ok(swift::test()),
            Self::Fastlane => Ok(fastlane::test()),
            Self::Gradle => Ok(gradle::test()),
            Self::Kustomize => Ok(kustomize::test()),
            Self::Terraform => Ok(terraform::test()),
        }
    }

    fn validate(
        self,
        project: &Project,
        runner: &dyn CommandRunner,
        files: &impl FileSystem,
    ) -> Result<Vec<LifecycleStep>, ToolResolutionError> {
        match self {
            Self::Gradle => Ok(gradle::validate()),
            Self::Zola => zola::validate(project, files).map_err(ToolResolutionError::Convention),
            Self::Bun => bun::validate(project, files).map_err(ToolResolutionError::Convention),
            _ => project.validate_steps(runner, files),
        }
    }

    fn audit(
        self,
        project: &Project,
        runner: &dyn CommandRunner,
        files: &impl FileSystem,
    ) -> Result<Vec<LifecycleStep>, ToolResolutionError> {
        match self {
            Self::Fastlane => Ok(fastlane::audit()),
            Self::Gradle => Ok(gradle::audit()),
            Self::Zola => zola::audit(project, files).map_err(ToolResolutionError::Convention),
            Self::Bun => bun::audit(project, files).map_err(ToolResolutionError::Convention),
            Self::Cargo | Self::SwiftPackageManager | Self::Kustomize | Self::Terraform => {
                let mut steps = project.validate_steps(runner, files)?;
                steps.extend(match self {
                    Self::Cargo => cargo::audit(),
                    Self::SwiftPackageManager => swift::audit(),
                    Self::Kustomize => kustomize::audit(),
                    Self::Terraform => terraform::audit(),
                    Self::Zola | Self::Bun | Self::Fastlane | Self::Gradle => unreachable!(),
                });
                Ok(steps)
            }
        }
    }

    fn matching_marker(self, root: &Utf8Path, files: &impl FileSystem) -> Option<&'static str> {
        match self {
            Self::Zola => zola::matching_marker(root, files),
            Self::Bun => bun::matching_marker(root, files),
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

    fn should_skip_discovery_directory(self, name: &str) -> bool {
        match self {
            Self::Zola => zola::should_skip_directory(name),
            Self::Bun => bun::should_skip_directory(name),
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
        let root = absolute_path(start)?;
        let upward = discover_upward(&root, start, files)?;

        let mut scoped_projects = Vec::new();
        discover_nested_targets(&root, files, &[], &mut scoped_projects)?;
        if !scoped_projects.is_empty() {
            return Ok(scoped_projects);
        }

        if let Some(project) = upward.nearest_project {
            return Ok(vec![project]);
        }

        Err(DiscoveryError::NoSupportedProject {
            start: start.to_owned(),
            git_root: upward.git_root,
        })
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

    fn is_bun_scriptless_container(&self, files: &impl FileSystem) -> bool {
        self.convention == ProjectConvention::Bun && bun::has_no_standard_scripts(&self.root, files)
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

    pub(crate) fn auxiliary_toolchain_install_hint(&self, program: &str) -> Option<&'static str> {
        if self.convention == ProjectConvention::Zola && program == bun::primary_program() {
            Some(bun::toolchain_install_hint())
        } else {
            None
        }
    }

    pub(crate) fn lifecycle_steps(
        &self,
        verb: Verb,
        runner: &dyn CommandRunner,
        fs: &impl FileSystem,
    ) -> Result<Vec<LifecycleStep>, ToolResolutionError> {
        match verb {
            Verb::Fix => self.convention.fix(self, runner, fs),
            Verb::Lint => self.convention.lint(self, runner, fs),
            Verb::Build => self.convention.build(self, runner, fs),
            Verb::Test => self.convention.test(self, fs),
            Verb::Validate => self.convention.validate(self, runner, fs),
            Verb::Audit => self.convention.audit(self, runner, fs),
        }
    }

    pub(crate) fn validate_manifest(&self, files: &impl FileSystem) -> Result<(), String> {
        self.convention.validate_manifest(self, files)
    }

    fn validate_steps(
        &self,
        runner: &dyn CommandRunner,
        files: &impl FileSystem,
    ) -> Result<Vec<LifecycleStep>, ToolResolutionError> {
        if self.convention == ProjectConvention::Fastlane {
            return Ok(fastlane::validate());
        }
        let mut steps = self.convention.lint(self, runner, files)?;
        steps.extend(self.convention.build(self, runner, files)?);
        steps.extend(self.convention.test(self, files)?);
        Ok(steps)
    }

    pub(crate) fn curate_failure_output(&self, output: &str) -> String {
        match self.convention {
            ProjectConvention::Gradle => gradle::curate_failure_output(output),
            ProjectConvention::Zola => zola::curate_failure_output(output),
            ProjectConvention::Bun => bun::curate_failure_output(output),
            _ => output.to_owned(),
        }
    }
}

#[derive(Debug)]
struct UpwardDiscovery {
    nearest_project: Option<Project>,
    git_root: Utf8PathBuf,
}

fn discover_upward(
    root: &Utf8Path,
    start: &Utf8Path,
    files: &impl FileSystem,
) -> Result<UpwardDiscovery, DiscoveryError> {
    let mut current = root.to_owned();
    let mut nearest_project = None;

    loop {
        if nearest_project.is_none() {
            nearest_project = matching_projects_at(&current, files).into_iter().next();
        }
        if files.exists(current.join(".git")) {
            return Ok(UpwardDiscovery {
                nearest_project,
                git_root: current,
            });
        }
        if !current.pop() {
            return Err(DiscoveryError::OutsideGitRepository {
                start: start.to_owned(),
            });
        }
    }
}

fn discover_nested_targets(
    root: &Utf8Path,
    files: &impl FileSystem,
    covered_conventions: &[ProjectConvention],
    projects: &mut Vec<Project>,
) -> Result<(), DiscoveryError> {
    if should_skip_discovery_directory(root) {
        return Ok(());
    }

    let direct_projects = matching_projects_at(root, files)
        .into_iter()
        .filter(|project| !covered_conventions.contains(&project.convention))
        .collect::<Vec<_>>();

    let mut descendant_covered = covered_conventions.to_vec();
    for project in &direct_projects {
        if !project.is_bun_scriptless_container(files) {
            descendant_covered.push(project.convention);
        }
    }

    let mut child_projects = Vec::new();
    discover_nested_children(root, files, &descendant_covered, &mut child_projects)?;

    for project in direct_projects {
        if project.is_bun_scriptless_container(files) && !child_projects.is_empty() {
            continue;
        }
        projects.push(project);
    }
    projects.extend(child_projects);
    Ok(())
}

fn discover_nested_children(
    root: &Utf8Path,
    files: &impl FileSystem,
    covered_conventions: &[ProjectConvention],
    projects: &mut Vec<Project>,
) -> Result<(), DiscoveryError> {
    for entry in files
        .read_dir(root)
        .map_err(|err| DiscoveryError::UnreadableDirectory {
            path: root.to_owned(),
            err,
        })?
    {
        if files.is_dir(&entry) {
            discover_nested_targets(&entry, files, covered_conventions, projects)?;
        }
    }
    Ok(())
}

fn matching_projects_at(root: &Utf8Path, files: &impl FileSystem) -> Vec<Project> {
    let mut projects = PROJECT_CONVENTIONS
        .iter()
        .filter_map(|convention| {
            convention
                .matching_marker(root, files)
                .map(|marker| Project {
                    convention: *convention,
                    marker,
                    root: root.to_owned(),
                })
        })
        .collect::<Vec<_>>();

    if projects
        .iter()
        .any(|project| project.convention == ProjectConvention::Zola)
    {
        projects.retain(|project| project.convention != ProjectConvention::Bun);
    }

    projects
}

fn should_skip_discovery_directory(root: &Utf8Path) -> bool {
    let Some(name) = root.file_name() else {
        return false;
    };
    DEFAULT_SKIP_DISCOVERY_DIRECTORIES.contains(&name)
        || PROJECT_CONVENTIONS
            .iter()
            .any(|convention| convention.should_skip_discovery_directory(name))
}

const DEFAULT_SKIP_DISCOVERY_DIRECTORIES: &[&str] = &[
    ".git",
    ".build",
    ".cache",
    ".gradle",
    ".next",
    ".swiftpm",
    ".terraform",
    ".terraform-cache",
    ".terragrunt-cache",
    ".turbo",
    "DerivedData",
    "Pods",
    "build",
    "coverage",
    "dist",
    "node_modules",
    "target",
    "vendor",
];

#[derive(Debug)]
pub(crate) enum ToolResolutionError {
    Convention(String),
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
