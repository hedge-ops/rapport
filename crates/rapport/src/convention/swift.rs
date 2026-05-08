use super::{
    DoctorCheck, LifecycleAction, LifecycleStep, Phase, Project, ToolResolutionError,
    lifecycle_step,
};
use crate::{CommandRunner, CommandSpec, Verb};
use rapport_cli::FileSystem;
use std::io;

const PRIMARY_PROGRAM: &str = "swift";
const DIRECT_FORMATTER_PROGRAM: &str = "swift-format";

pub(super) fn name() -> &'static str {
    "SwiftPM"
}

pub(super) fn markers() -> &'static [&'static str] {
    &["Package.swift"]
}

pub(super) fn primary_program() -> &'static str {
    PRIMARY_PROGRAM
}

pub(super) fn direct_formatter_program() -> &'static str {
    DIRECT_FORMATTER_PROGRAM
}

pub(super) fn toolchain_install_hint() -> &'static str {
    "Install a Swift toolchain from https://www.swift.org/install/ and make sure `swift` is on PATH."
}

pub(super) fn formatter_install_hint() -> &'static str {
    "Install Swift 6+ for `swift format`, or install `swift-format` on PATH."
}

pub(super) fn validate_manifest(project: &Project, files: &impl FileSystem) -> Result<(), String> {
    let marker = project.marker();
    let manifest = project.manifest_path();
    let contents = files
        .read_to_string(&manifest)
        .map_err(|err| format!("Failed to read `{marker}`: {err}"))?;
    parse_swift_tools_version(&contents)
        .map(|_| ())
        .map_err(|reason| format!("`{marker}` {reason}."))
}

pub(super) fn fix(
    project: &Project,
    runner: &dyn CommandRunner,
) -> Result<Vec<LifecycleStep>, ToolResolutionError> {
    let formatter = resolve_formatter(project, runner)?;
    Ok(vec![formatter_step(
        Phase::Format,
        formatter,
        &["format", "format", "--in-place"],
        &["format", "--in-place"],
        &format_inputs(project),
    )])
}

pub(super) fn lint(
    project: &Project,
    runner: &dyn CommandRunner,
) -> Result<Vec<LifecycleStep>, ToolResolutionError> {
    let formatter = resolve_formatter(project, runner)?;
    Ok(vec![formatter_step(
        Phase::Lint,
        formatter,
        &["format", "lint", "--strict"],
        &["lint", "--strict"],
        &format_inputs(project),
    )])
}

pub(super) fn build() -> Vec<LifecycleStep> {
    vec![swift_step(Phase::Build, ["build"])]
}

pub(super) fn test() -> Vec<LifecycleStep> {
    vec![swift_step(Phase::Test, ["test"])]
}

pub(super) fn audit() -> Vec<LifecycleStep> {
    vec![swift_step(Phase::ReleaseBuild, ["build", "-c", "release"])]
}

pub(super) fn doctor_checks(
    project: &Project,
    runner: &dyn CommandRunner,
    files: &impl FileSystem,
) -> (Vec<DoctorCheck>, Vec<DoctorCheck>) {
    let tools = vec![
        super::tool_check(
            project,
            runner,
            "swift",
            primary_program(),
            ["--version"],
            &super::ALL_VERBS,
            Some(toolchain_install_hint()),
        ),
        formatter_check(project, runner),
    ];
    let configuration = vec![
        super::file_check(
            files,
            &project.root.join("Package.swift"),
            "Package.swift",
            &super::ALL_VERBS,
            "Add a SwiftPM `Package.swift` manifest at the package root.",
        ),
        super::convention_check(
            validate_manifest(project, files),
            "Swift tools version",
            &super::ALL_VERBS,
            "Start `Package.swift` with a valid `// swift-tools-version:` declaration.",
        ),
    ];
    (tools, configuration)
}

fn formatter_check(project: &Project, runner: &dyn CommandRunner) -> DoctorCheck {
    super::alternative_tool_check(
        project,
        runner,
        "Swift formatter",
        &[
            ("swift", &["format", "--version"], "swift format"),
            ("swift-format", &["--version"], "swift-format"),
        ],
        &[Verb::Fix, Verb::Lint, Verb::Validate, Verb::Audit],
        formatter_install_hint(),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SwiftFormatter {
    Driver,
    Direct,
}

pub(crate) fn parse_swift_tools_version(contents: &str) -> Result<&str, &'static str> {
    let first_line = contents.lines().next().unwrap_or_default();
    let Some(raw_version) = first_line.strip_prefix("// swift-tools-version:") else {
        return Err("must begin with a valid `// swift-tools-version:` declaration");
    };
    let version = raw_version.trim();
    if version.is_empty() || !version.split('.').all(is_numeric_version_component) {
        return Err("must begin with a valid `// swift-tools-version:` declaration");
    }
    Ok(version)
}

fn resolve_formatter(
    project: &Project,
    runner: &(impl CommandRunner + ?Sized),
) -> Result<SwiftFormatter, ToolResolutionError> {
    let driver_probe = CommandSpec::new(PRIMARY_PROGRAM, ["format", "--version"]);
    match runner.run(&driver_probe, &project.root) {
        Ok(outcome) if outcome.success => return Ok(SwiftFormatter::Driver),
        Ok(_) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Err(ToolResolutionError::MissingSwift(err));
        }
        Err(err) => {
            return Err(ToolResolutionError::ProbeInvoke {
                program: PRIMARY_PROGRAM,
                err,
            });
        }
    }

    let direct_probe = CommandSpec::new(DIRECT_FORMATTER_PROGRAM, ["--version"]);
    match runner.run(&direct_probe, &project.root) {
        Ok(outcome) if outcome.success => Ok(SwiftFormatter::Direct),
        Ok(_) => Err(ToolResolutionError::MissingFormatter),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            Err(ToolResolutionError::MissingFormatter)
        }
        Err(err) => Err(ToolResolutionError::ProbeInvoke {
            program: DIRECT_FORMATTER_PROGRAM,
            err,
        }),
    }
}

fn is_numeric_version_component(component: &str) -> bool {
    !component.is_empty() && component.chars().all(|ch| ch.is_ascii_digit())
}

fn format_inputs(project: &Project) -> Vec<String> {
    [project.marker(), "Sources", "Tests", "Plugins"]
        .into_iter()
        .filter(|relative| project.root.join(relative).exists())
        .map(ToOwned::to_owned)
        .collect()
}

fn formatter_step(
    phase: Phase,
    formatter: SwiftFormatter,
    driver_args: &'static [&'static str],
    direct_args: &'static [&'static str],
    inputs: &[String],
) -> LifecycleStep {
    let spec = match formatter {
        SwiftFormatter::Driver => formatter_spec(PRIMARY_PROGRAM, driver_args, inputs),
        SwiftFormatter::Direct => formatter_spec(DIRECT_FORMATTER_PROGRAM, direct_args, inputs),
    };
    LifecycleStep {
        phase,
        action: LifecycleAction::Command(spec),
    }
}

fn formatter_spec(
    program: &'static str,
    base_args: &'static [&'static str],
    inputs: &[String],
) -> CommandSpec {
    let mut args = base_args
        .iter()
        .map(|arg| (*arg).to_owned())
        .collect::<Vec<_>>();
    args.extend([
        "--recursive".to_owned(),
        "--no-color-diagnostics".to_owned(),
    ]);
    args.extend(inputs.iter().cloned());
    CommandSpec::new(program, args)
}

fn swift_step<const N: usize>(phase: Phase, args: [&'static str; N]) -> LifecycleStep {
    lifecycle_step(phase, PRIMARY_PROGRAM, args)
}

#[cfg(test)]
mod tests {
    use super::super::ProjectConvention;
    use super::*;
    use claims::{assert_err, assert_ok};
    use rapport_cli::{InMemoryFileSystem, Utf8PathBuf};

    fn swift_project(root: impl Into<Utf8PathBuf>) -> Project {
        Project {
            convention: ProjectConvention::SwiftPackageManager,
            marker: "Package.swift",
            root: root.into(),
        }
    }

    #[test]
    fn swift_tools_version_parser_accepts_numeric_versions() {
        assert_eq!(
            parse_swift_tools_version("// swift-tools-version: 6.0\n"),
            Ok("6.0")
        );
        assert_eq!(
            parse_swift_tools_version("// swift-tools-version:5.10.1\n"),
            Ok("5.10.1")
        );
    }

    #[test]
    fn swift_tools_version_parser_rejects_missing_or_malformed_versions() {
        assert!(parse_swift_tools_version("").is_err());
        assert!(parse_swift_tools_version("import PackageDescription\n").is_err());
        assert!(parse_swift_tools_version("// swift-tools-version:\n").is_err());
        assert!(parse_swift_tools_version("// swift-tools-version: 6.x\n").is_err());
    }

    #[test]
    fn manifest_validation_reads_through_file_system() {
        let root = Utf8PathBuf::from("/work");
        let manifest = root.join("Package.swift");
        let project = swift_project(root);
        let mut files = InMemoryFileSystem::default();
        files.add_file_with_contents(
            manifest,
            "// swift-tools-version: 6.0\nimport PackageDescription\n",
        );

        assert_ok!(project.validate_manifest(&files));
    }

    #[test]
    fn manifest_validation_reports_file_system_read_errors() {
        let root = Utf8PathBuf::from("/work");
        let project = swift_project(root);
        let files = InMemoryFileSystem::default();

        let err = assert_err!(project.validate_manifest(&files));

        assert!(err.contains("Failed to read `Package.swift`"));
        assert!(err.contains("not found"));
    }
}
