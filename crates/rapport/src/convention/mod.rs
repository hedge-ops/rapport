mod cargo;
pub(crate) mod swift;

pub(crate) use swift::FormatterResolutionError;

use crate::{CommandRunner, CommandSpec, Verb};
use rapport_cli::{FileSystem, Utf8Path, Utf8PathBuf};
use std::io;

static PROJECT_CONVENTIONS: &[ProjectConvention] = &[
    ProjectConvention::Cargo,
    ProjectConvention::SwiftPackageManager,
];

pub(crate) fn describe_expected_markers() -> String {
    let entries = PROJECT_CONVENTIONS
        .iter()
        .map(|convention| format!("`{}` for {}", convention.marker(), convention.name()))
        .collect::<Vec<_>>();
    entries.join(" or ")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectConvention {
    Cargo,
    SwiftPackageManager,
}

impl ProjectConvention {
    fn name(self) -> &'static str {
        match self {
            Self::Cargo => cargo::name(),
            Self::SwiftPackageManager => swift::name(),
        }
    }

    fn marker(self) -> &'static str {
        match self {
            Self::Cargo => cargo::marker(),
            Self::SwiftPackageManager => swift::marker(),
        }
    }

    fn primary_program(self) -> &'static str {
        match self {
            Self::Cargo => cargo::primary_program(),
            Self::SwiftPackageManager => swift::primary_program(),
        }
    }

    fn direct_formatter_program(self) -> Option<&'static str> {
        match self {
            Self::Cargo => None,
            Self::SwiftPackageManager => Some(swift::direct_formatter_program()),
        }
    }

    fn toolchain_install_hint(self) -> Option<&'static str> {
        match self {
            Self::Cargo => None,
            Self::SwiftPackageManager => Some(swift::toolchain_install_hint()),
        }
    }

    fn formatter_install_hint(self) -> Option<&'static str> {
        match self {
            Self::Cargo => None,
            Self::SwiftPackageManager => Some(swift::formatter_install_hint()),
        }
    }

    fn validate_manifest(self, project: &Project, files: &impl FileSystem) -> Result<(), String> {
        match self {
            Self::Cargo => Ok(()),
            Self::SwiftPackageManager => swift::validate_manifest(project, files),
        }
    }

    fn fix(
        self,
        project: &Project,
        runner: &dyn CommandRunner,
    ) -> Result<Vec<LifecycleStep>, FormatterResolutionError> {
        match self {
            Self::Cargo => Ok(cargo::fix()),
            Self::SwiftPackageManager => swift::fix(project, runner),
        }
    }

    fn lint(
        self,
        project: &Project,
        runner: &dyn CommandRunner,
    ) -> Result<Vec<LifecycleStep>, FormatterResolutionError> {
        match self {
            Self::Cargo => Ok(cargo::lint()),
            Self::SwiftPackageManager => swift::lint(project, runner),
        }
    }

    fn build(self) -> Vec<LifecycleStep> {
        match self {
            Self::Cargo => cargo::build(),
            Self::SwiftPackageManager => swift::build(),
        }
    }

    fn test(self) -> Vec<LifecycleStep> {
        match self {
            Self::Cargo => cargo::test(),
            Self::SwiftPackageManager => swift::test(),
        }
    }

    fn audit(self) -> Vec<LifecycleStep> {
        match self {
            Self::Cargo => cargo::audit(),
            Self::SwiftPackageManager => swift::audit(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Project {
    convention: ProjectConvention,
    pub(crate) root: Utf8PathBuf,
}

impl Project {
    pub(crate) fn discover(
        start: &Utf8Path,
        files: &impl FileSystem,
    ) -> Result<Self, DiscoveryError> {
        let mut current = absolute_path(start)?;
        let mut nearest_project = None;

        loop {
            for convention in PROJECT_CONVENTIONS {
                if nearest_project.is_none() && files.is_file(current.join(convention.marker())) {
                    nearest_project = Some(Self {
                        convention: *convention,
                        root: current.clone(),
                    });
                }
            }
            if files.exists(current.join(".git")) {
                return nearest_project.ok_or_else(|| DiscoveryError::NoSupportedProject {
                    start: start.to_owned(),
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

    pub(crate) fn marker(&self) -> &'static str {
        self.convention.marker()
    }

    pub(crate) fn manifest_path(&self) -> Utf8PathBuf {
        self.root.join(self.marker())
    }

    pub(crate) fn is_swift_package_manager(&self) -> bool {
        self.convention == ProjectConvention::SwiftPackageManager
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
    ) -> Result<Vec<LifecycleStep>, FormatterResolutionError> {
        match verb {
            Verb::Fix => self.convention.fix(self, runner),
            Verb::Lint => self.convention.lint(self, runner),
            Verb::Build => Ok(self.convention.build()),
            Verb::Test => Ok(self.convention.test()),
            Verb::Validate => self.validate_steps(runner),
            Verb::Audit => {
                let mut steps = self.validate_steps(runner)?;
                steps.extend(self.convention.audit());
                Ok(steps)
            }
        }
    }

    pub(crate) fn validate_manifest(&self, files: &impl FileSystem) -> Result<(), String> {
        self.convention.validate_manifest(self, files)
    }

    fn validate_steps(
        &self,
        runner: &dyn CommandRunner,
    ) -> Result<Vec<LifecycleStep>, FormatterResolutionError> {
        let mut steps = self.convention.lint(self, runner)?;
        steps.extend(self.convention.build());
        steps.extend(self.convention.test());
        Ok(steps)
    }
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
    #[display("format")]
    Format,
    #[display("lint")]
    Lint,
    #[display("build")]
    Build,
    #[display("test")]
    Test,
    #[display("release build")]
    ReleaseBuild,
    #[display("docs")]
    Docs,
}

#[derive(Debug, Clone)]
pub(crate) struct LifecycleStep {
    pub(crate) phase: Phase,
    pub(crate) spec: CommandSpec,
}

impl LifecycleStep {
    fn new(phase: Phase, spec: CommandSpec) -> Self {
        Self { phase, spec }
    }
}

fn lifecycle_step<I, S>(phase: Phase, program: impl Into<String>, args: I) -> LifecycleStep
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    LifecycleStep::new(phase, CommandSpec::new(program, args))
}
