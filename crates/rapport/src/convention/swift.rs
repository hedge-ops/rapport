use super::{
    DoctorCheck, LifecycleAction, LifecycleStep, Phase, Project, ToolResolutionError,
    lifecycle_step,
};
use crate::{CommandRunner, CommandSpec, Verb};
use rapport_cli::FileSystem;
use std::io;

const PRIMARY_PROGRAM: &str = "swift";
const DIRECT_FORMATTER_PROGRAM: &str = "swift-format";
const SWIFTFORMAT_PROGRAM: &str = "swiftformat";
const SWIFTLINT_PROGRAM: &str = "swiftlint";
const APPLE_FORMATTER_CONFIG: &str = ".swift-format";
const SWIFTFORMAT_CONFIG: &str = ".swiftformat";
const SWIFTLINT_CONFIGS: &[&str] = &[".swiftlint.yml", ".swiftlint.yaml"];

const SWIFT_BUILD_VERBS: [Verb; 4] = [Verb::Build, Verb::Test, Verb::Validate, Verb::Audit];
const STYLE_VERBS: [Verb; 4] = [Verb::Fix, Verb::Lint, Verb::Validate, Verb::Audit];

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

pub(super) fn swiftformat_program() -> &'static str {
    SWIFTFORMAT_PROGRAM
}

pub(super) fn linter_program() -> &'static str {
    SWIFTLINT_PROGRAM
}

pub(super) fn toolchain_install_hint() -> &'static str {
    "Install a Swift toolchain from https://www.swift.org/install/ and make sure `swift` is on PATH."
}

pub(super) fn formatter_install_hint() -> &'static str {
    "Install Swift 6+ for `swift format`, install `swift-format`, or install SwiftFormat (`swiftformat`) for `.swiftformat`."
}

pub(super) fn apple_formatter_install_hint() -> &'static str {
    "Install Swift 6+ for `swift format`, or install `swift-format` on PATH."
}

pub(super) fn swiftformat_install_hint() -> &'static str {
    "Install SwiftFormat from https://github.com/nicklockwood/SwiftFormat and make sure `swiftformat` is on PATH."
}

pub(super) fn linter_install_hint() -> &'static str {
    "Install SwiftLint from https://github.com/realm/SwiftLint and make sure `swiftlint` is on PATH."
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
    files: &impl FileSystem,
) -> Result<Vec<LifecycleStep>, ToolResolutionError> {
    let Some(config) = detect_formatter_config(project, files) else {
        return Ok(Vec::new());
    };
    let formatter = resolve_formatter(project, runner, config)?;
    Ok(vec![format_step(
        formatter,
        config,
        &format_inputs(project),
    )])
}

pub(super) fn lint(
    project: &Project,
    runner: &dyn CommandRunner,
    files: &impl FileSystem,
) -> Result<Vec<LifecycleStep>, ToolResolutionError> {
    let mut steps = Vec::new();
    if let Some(config) = detect_formatter_config(project, files) {
        let formatter = resolve_formatter(project, runner, config)?;
        steps.push(formatter_lint_step(
            formatter,
            config,
            &format_inputs(project),
        ));
    }
    if let Some(config) = detect_linter_config(project, files) {
        resolve_linter(project, runner, config)?;
        steps.push(linter_step(config));
    }
    Ok(steps)
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
    let mut tools = vec![super::tool_check(
        project,
        runner,
        "swift",
        primary_program(),
        ["--version"],
        &SWIFT_BUILD_VERBS,
        Some(toolchain_install_hint()),
    )];
    let formatter_config = detect_formatter_config(project, files);
    if let Some(config) = formatter_config {
        tools.push(formatter_check(project, runner, config));
    }
    let linter_config = detect_linter_config(project, files);
    if let Some(config) = linter_config {
        tools.push(linter_check(project, runner, config));
    }

    let mut configuration = vec![
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
    if let Some(config) = formatter_config {
        configuration.push(super::file_check(
            files,
            &project.root.join(config.file_name()),
            format!("Swift formatter config `{}`", config.file_name()).as_str(),
            &STYLE_VERBS,
            "Add a Swift formatter config at the package root.",
        ));
    }
    if let Some(config) = linter_config {
        configuration.push(super::file_check(
            files,
            &project.root.join(config.file_name()),
            format!("SwiftLint config `{}`", config.file_name()).as_str(),
            &STYLE_VERBS,
            "Add a SwiftLint config at the package root.",
        ));
    }
    (tools, configuration)
}

fn formatter_check(
    project: &Project,
    runner: &dyn CommandRunner,
    config: FormatterConfig,
) -> DoctorCheck {
    match config {
        FormatterConfig::AppleSwiftFormat => super::alternative_tool_check(
            project,
            runner,
            "Swift formatter",
            &[
                ("swift", &["format", "--version"], "swift format"),
                ("swift-format", &["--version"], "swift-format"),
            ],
            &STYLE_VERBS,
            apple_formatter_install_hint(),
        ),
        FormatterConfig::SwiftFormat => super::tool_check(
            project,
            runner,
            "SwiftFormat",
            SWIFTFORMAT_PROGRAM,
            ["--version"],
            &STYLE_VERBS,
            Some(swiftformat_install_hint()),
        ),
    }
}

fn linter_check(
    project: &Project,
    runner: &dyn CommandRunner,
    config: LinterConfig,
) -> DoctorCheck {
    super::tool_check(
        project,
        runner,
        format!("SwiftLint `{}`", config.file_name()).as_str(),
        SWIFTLINT_PROGRAM,
        ["version"],
        &STYLE_VERBS,
        Some(linter_install_hint()),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SwiftFormatter {
    Driver,
    Direct,
    SwiftFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FormatterConfig {
    AppleSwiftFormat,
    SwiftFormat,
}

impl FormatterConfig {
    fn file_name(self) -> &'static str {
        match self {
            Self::AppleSwiftFormat => APPLE_FORMATTER_CONFIG,
            Self::SwiftFormat => SWIFTFORMAT_CONFIG,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinterConfig {
    SwiftLintYaml,
    SwiftLintYml,
}

impl LinterConfig {
    fn file_name(self) -> &'static str {
        match self {
            Self::SwiftLintYaml => ".swiftlint.yaml",
            Self::SwiftLintYml => ".swiftlint.yml",
        }
    }
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
    config: FormatterConfig,
) -> Result<SwiftFormatter, ToolResolutionError> {
    if config == FormatterConfig::SwiftFormat {
        let probe = CommandSpec::new(SWIFTFORMAT_PROGRAM, ["--version"]);
        return match runner.run(&probe, &project.root) {
            Ok(outcome) if outcome.success => Ok(SwiftFormatter::SwiftFormat),
            Ok(_) => Err(missing_formatter(config)),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Err(missing_formatter(config)),
            Err(err) => Err(ToolResolutionError::ProbeInvoke {
                program: SWIFTFORMAT_PROGRAM,
                err,
            }),
        };
    }

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
        Ok(_) => Err(missing_formatter(config)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Err(missing_formatter(config)),
        Err(err) => Err(ToolResolutionError::ProbeInvoke {
            program: DIRECT_FORMATTER_PROGRAM,
            err,
        }),
    }
}

fn resolve_linter(
    project: &Project,
    runner: &(impl CommandRunner + ?Sized),
    config: LinterConfig,
) -> Result<(), ToolResolutionError> {
    let probe = CommandSpec::new(SWIFTLINT_PROGRAM, ["version"]);
    match runner.run(&probe, &project.root) {
        Ok(outcome) if outcome.success => Ok(()),
        Ok(_) => Err(missing_linter(config)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Err(missing_linter(config)),
        Err(err) => Err(ToolResolutionError::ProbeInvoke {
            program: SWIFTLINT_PROGRAM,
            err,
        }),
    }
}

fn missing_formatter(config: FormatterConfig) -> ToolResolutionError {
    match config {
        FormatterConfig::AppleSwiftFormat => ToolResolutionError::MissingFormatter {
            config: APPLE_FORMATTER_CONFIG,
            install_hint: apple_formatter_install_hint(),
            first_probe: "swift format --version",
            second_probe: Some("swift-format --version"),
        },
        FormatterConfig::SwiftFormat => ToolResolutionError::MissingFormatter {
            config: SWIFTFORMAT_CONFIG,
            install_hint: swiftformat_install_hint(),
            first_probe: "swiftformat --version",
            second_probe: None,
        },
    }
}

fn missing_linter(config: LinterConfig) -> ToolResolutionError {
    ToolResolutionError::MissingLinter {
        config: config.file_name(),
        install_hint: linter_install_hint(),
        probe: "swiftlint version",
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

fn detect_formatter_config(project: &Project, files: &impl FileSystem) -> Option<FormatterConfig> {
    if files.is_file(project.root.join(APPLE_FORMATTER_CONFIG)) {
        Some(FormatterConfig::AppleSwiftFormat)
    } else if files.is_file(project.root.join(SWIFTFORMAT_CONFIG)) {
        Some(FormatterConfig::SwiftFormat)
    } else {
        None
    }
}

fn detect_linter_config(project: &Project, files: &impl FileSystem) -> Option<LinterConfig> {
    for config in SWIFTLINT_CONFIGS {
        if files.is_file(project.root.join(config)) {
            return Some(match *config {
                ".swiftlint.yaml" => LinterConfig::SwiftLintYaml,
                ".swiftlint.yml" => LinterConfig::SwiftLintYml,
                _ => unreachable!(),
            });
        }
    }
    None
}

fn format_step(
    formatter: SwiftFormatter,
    config: FormatterConfig,
    inputs: &[String],
) -> LifecycleStep {
    formatter_step(
        Phase::Format,
        formatter,
        config,
        &["format", "format", "--in-place"],
        &["format", "--in-place"],
        &["--config"],
        inputs,
    )
}

fn formatter_lint_step(
    formatter: SwiftFormatter,
    config: FormatterConfig,
    inputs: &[String],
) -> LifecycleStep {
    formatter_step(
        Phase::Lint,
        formatter,
        config,
        &["format", "lint", "--strict"],
        &["lint", "--strict"],
        &["--lint", "--config"],
        inputs,
    )
}

fn formatter_step(
    phase: Phase,
    formatter: SwiftFormatter,
    config: FormatterConfig,
    driver_args: &'static [&'static str],
    direct_args: &'static [&'static str],
    swiftformat_args: &'static [&'static str],
    inputs: &[String],
) -> LifecycleStep {
    let spec = match formatter {
        SwiftFormatter::Driver => {
            apple_formatter_spec(PRIMARY_PROGRAM, driver_args, config, inputs)
        }
        SwiftFormatter::Direct => {
            apple_formatter_spec(DIRECT_FORMATTER_PROGRAM, direct_args, config, inputs)
        }
        SwiftFormatter::SwiftFormat => swiftformat_spec(swiftformat_args, config, inputs),
    };
    LifecycleStep {
        phase,
        action: LifecycleAction::Command(spec),
    }
}

fn apple_formatter_spec(
    program: &'static str,
    base_args: &'static [&'static str],
    config: FormatterConfig,
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
    args.extend(["--configuration".to_owned(), config.file_name().to_owned()]);
    args.extend(inputs.iter().cloned());
    CommandSpec::new(program, args)
}

fn swiftformat_spec(
    base_args: &'static [&'static str],
    config: FormatterConfig,
    inputs: &[String],
) -> CommandSpec {
    let mut args = base_args
        .iter()
        .map(|arg| (*arg).to_owned())
        .collect::<Vec<_>>();
    args.push(config.file_name().to_owned());
    args.extend(inputs.iter().cloned());
    CommandSpec::new(SWIFTFORMAT_PROGRAM, args)
}

fn linter_step(config: LinterConfig) -> LifecycleStep {
    LifecycleStep {
        phase: Phase::Lint,
        action: LifecycleAction::Command(CommandSpec::new(
            SWIFTLINT_PROGRAM,
            ["lint", "--strict", "--config", config.file_name()],
        )),
    }
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
