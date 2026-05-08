mod convention;
mod runner;
mod view;

pub use runner::{CommandOutcome, CommandRunner, CommandSpec, RealCommandRunner};

use convention::{
    DiscoveryError, DoctorCheck, DoctorStatus, DoctorTargetReport, LifecycleAction, Phase,
    PrimeConventionStatus, PrimeTargetReport, Project, ToolResolutionError,
    describe_expected_markers, expected_marker_entries,
};
use nonempty::{NonEmpty, nonempty};
use rapport_cli::{
    FileSystem, HelpTarget, Invocation, ParseError, Parser as _, RealFileSystem, RepositoryPath,
    Utf8Path, parse_validated,
};
use std::fmt::Display;
use std::io::{self, Write};
use std::process::ExitCode;
use std::time::Instant;
use view::{Outcome, RunHint, ViewBuilder};

const USAGE: &str = "usage: rapport <fix|lint|build|test|validate|audit|doctor|prime> <path>";

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    strum::Display,
    strum::EnumString,
    strum::EnumIter,
    strum::AsRefStr,
)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum Verb {
    Fix,
    Lint,
    Build,
    Test,
    Validate,
    Audit,
}

impl Verb {
    fn about(self) -> &'static str {
        match self {
            Self::Fix => "Auto-fix issues (modifies code)",
            Self::Lint => "Check style and conventions (read-only)",
            Self::Build => "Verify the code compiles",
            Self::Test => "Run the test suite",
            Self::Validate => "Pre-commit check (lint + build + test)",
            Self::Audit => "Pre-release check (validate + release-mode compile)",
        }
    }

    fn hints(self, outcome: Outcome, path: &Utf8Path) -> NonEmpty<RunHint> {
        let p = path.as_str();
        let cmd = |verb: &str| RunHint::new(format!("rapport {verb} {p}"));
        match (self, outcome) {
            (Self::Fix, Outcome::Pass) | (Self::Build, Outcome::Fail) => nonempty![cmd("lint")],
            (Self::Fix | Self::Lint, Outcome::Fail) => nonempty![cmd("fix")],
            (Self::Lint, Outcome::Pass) => nonempty![cmd("build")],
            (Self::Build, Outcome::Pass) | (Self::Test, Outcome::Fail) => nonempty![cmd("test")],
            (Self::Test, Outcome::Pass) | (Self::Audit, Outcome::Fail) => {
                nonempty![cmd("validate")]
            }
            (Self::Validate, Outcome::Pass) => nonempty![cmd("audit")],
            (Self::Validate, Outcome::Fail) => {
                nonempty![cmd("lint"), cmd("build"), cmd("test")]
            }
            (Self::Audit, Outcome::Pass) => nonempty![RunHint::new("git push")],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CliVerb {
    Lifecycle(Verb),
    Doctor,
    Prime,
}

impl CliVerb {
    fn about(self) -> &'static str {
        match self {
            Self::Lifecycle(verb) => verb.about(),
            Self::Doctor => "Check target readiness without running lifecycle work",
            Self::Prime => "Explain rapport conventions for the requested scope",
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Lifecycle(Verb::Fix) => "fix",
            Self::Lifecycle(Verb::Lint) => "lint",
            Self::Lifecycle(Verb::Build) => "build",
            Self::Lifecycle(Verb::Test) => "test",
            Self::Lifecycle(Verb::Validate) => "validate",
            Self::Lifecycle(Verb::Audit) => "audit",
            Self::Doctor => "doctor",
            Self::Prime => "prime",
        }
    }

    fn all() -> [Self; 8] {
        [
            Self::Lifecycle(Verb::Fix),
            Self::Lifecycle(Verb::Lint),
            Self::Lifecycle(Verb::Build),
            Self::Lifecycle(Verb::Test),
            Self::Lifecycle(Verb::Validate),
            Self::Lifecycle(Verb::Audit),
            Self::Doctor,
            Self::Prime,
        ]
    }
}

impl Display for CliVerb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug)]
enum Command {
    Fix { path: RepositoryPath },
    Lint { path: RepositoryPath },
    Build { path: RepositoryPath },
    Test { path: RepositoryPath },
    Validate { path: RepositoryPath },
    Audit { path: RepositoryPath },
    Doctor { path: RepositoryPath },
    Prime { path: RepositoryPath },
}

impl Command {
    #[must_use]
    fn lifecycle_verb(&self) -> Option<Verb> {
        match self {
            Self::Fix { .. } => Some(Verb::Fix),
            Self::Lint { .. } => Some(Verb::Lint),
            Self::Build { .. } => Some(Verb::Build),
            Self::Test { .. } => Some(Verb::Test),
            Self::Validate { .. } => Some(Verb::Validate),
            Self::Audit { .. } => Some(Verb::Audit),
            Self::Doctor { .. } | Self::Prime { .. } => None,
        }
    }

    #[must_use]
    fn path(&self) -> &RepositoryPath {
        match self {
            Self::Fix { path }
            | Self::Lint { path }
            | Self::Build { path }
            | Self::Test { path }
            | Self::Validate { path }
            | Self::Audit { path }
            | Self::Doctor { path }
            | Self::Prime { path } => path,
        }
    }

    fn from_argv_with_file_system(
        verb: CliVerb,
        rest: &[String],
        fs: &impl FileSystem,
    ) -> Result<Self, ParseError> {
        let [p] = rest else {
            return Err(ParseError::MissingArg {
                verb: verb.to_string(),
                expected: "path",
            });
        };
        let path: RepositoryPath = parse_validated(verb.as_str(), p, fs)?;
        Ok(match verb {
            CliVerb::Lifecycle(Verb::Fix) => Self::Fix { path },
            CliVerb::Lifecycle(Verb::Lint) => Self::Lint { path },
            CliVerb::Lifecycle(Verb::Build) => Self::Build { path },
            CliVerb::Lifecycle(Verb::Test) => Self::Test { path },
            CliVerb::Lifecycle(Verb::Validate) => Self::Validate { path },
            CliVerb::Lifecycle(Verb::Audit) => Self::Audit { path },
            CliVerb::Doctor => Self::Doctor { path },
            CliVerb::Prime => Self::Prime { path },
        })
    }
}

impl rapport_cli::Parser for Command {
    type Verb = CliVerb;

    fn parse_verb(name: &str) -> Result<CliVerb, ParseError> {
        match name {
            "doctor" => return Ok(CliVerb::Doctor),
            "prime" => return Ok(CliVerb::Prime),
            _ => {}
        }
        name.parse()
            .map(CliVerb::Lifecycle)
            .map_err(|_| ParseError::UnknownVerb(name.into()))
    }

    fn from_argv(verb: CliVerb, rest: &[String]) -> Result<Self, ParseError> {
        Self::from_argv_with_file_system(verb, rest, &RealFileSystem)
    }
}

impl Display for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fix { .. } => Verb::Fix.fmt(f),
            Self::Lint { .. } => Verb::Lint.fmt(f),
            Self::Build { .. } => Verb::Build.fmt(f),
            Self::Test { .. } => Verb::Test.fmt(f),
            Self::Validate { .. } => Verb::Validate.fmt(f),
            Self::Audit { .. } => Verb::Audit.fmt(f),
            Self::Doctor { .. } => f.write_str("doctor"),
            Self::Prime { .. } => f.write_str("prime"),
        }
    }
}

pub fn run<I, O, E>(argv: I, runner: &dyn CommandRunner, out: &mut O, err: &mut E) -> ExitCode
where
    I: IntoIterator<Item = String>,
    O: Write,
    E: Write,
{
    run_with_file_system(argv, runner, &RealFileSystem, out, err)
}

fn run_with_file_system<I, O, E>(
    argv: I,
    runner: &dyn CommandRunner,
    fs: &impl FileSystem,
    out: &mut O,
    err: &mut E,
) -> ExitCode
where
    I: IntoIterator<Item = String>,
    O: Write,
    E: Write,
{
    match parse_with_file_system(argv, fs) {
        Ok(Invocation::Run(command)) => run_command(&command, runner, fs, out, err),
        Ok(Invocation::Help(target)) => {
            let _ = writeln!(out, "{}", render_help(&target));
            ExitCode::SUCCESS
        }
        Err(parse_err) => {
            let _ = writeln!(err, "{}", render_error(&parse_err));
            ExitCode::from(2)
        }
    }
}

fn parse_with_file_system<I>(
    argv: I,
    fs: &impl FileSystem,
) -> Result<Invocation<Command>, ParseError>
where
    I: IntoIterator<Item = String>,
{
    let argv: Vec<String> = argv.into_iter().collect();
    match argv.as_slice() {
        [] => Err(ParseError::NoVerb),
        [a] if is_help_flag(a) || a == "help" => Ok(Invocation::Help(HelpTarget::Top)),
        [first, verb_name] if first == "help" => {
            let verb = Command::parse_verb(verb_name)?;
            Ok(Invocation::Help(HelpTarget::Verb(verb)))
        }
        [name, rest @ ..] => {
            let verb = Command::parse_verb(name)?;
            if rest.iter().any(|a| is_help_flag(a)) {
                Ok(Invocation::Help(HelpTarget::Verb(verb)))
            } else {
                Command::from_argv_with_file_system(verb, rest, fs).map(Invocation::Run)
            }
        }
    }
}

fn is_help_flag(s: &str) -> bool {
    s == "-h" || s == "--help"
}

fn render_help(target: &HelpTarget<CliVerb>) -> String {
    match target {
        HelpTarget::Top => render_help_top(),
        HelpTarget::Verb(v) => render_help_verb(*v),
    }
}

fn render_help_top() -> String {
    ViewBuilder::new()
        .title("rapport — workspace command runner")
        .section("Usage", |b| {
            b.usage(["rapport <verb> <path>", "rapport help [<verb>]"])
        })
        .section("Verbs", |b| {
            b.entries(CliVerb::all().into_iter().map(|v| (v, v.about())))
        })
        .next_actions(nonempty![RunHint::new("rapport help build")])
        .build()
}

fn render_help_verb(verb: CliVerb) -> String {
    ViewBuilder::new()
        .title(format!("rapport {verb} — {}", verb.about()))
        .section("Usage", |b| b.usage([format!("rapport {verb} <path>")]))
        .section("Args", |b| {
            b.entries([("<path>", "Repository directory to operate on")])
        })
        .next_actions(nonempty![RunHint::new(format!("rapport {verb} ."))])
        .build()
}

fn render_error(err: &ParseError) -> String {
    let vb = ViewBuilder::new();
    let (vb, hints) = match err {
        ParseError::NoVerb => (vb.paragraph(USAGE), nonempty![RunHint::new("rapport help")]),
        ParseError::UnknownVerb(v) => (
            vb.paragraph(format!("'{v}' is not a recognized verb."))
                .paragraph(USAGE),
            nonempty![RunHint::new("rapport help")],
        ),
        ParseError::MissingArg { verb, expected } => (
            vb.paragraph(format!("rapport {verb} requires a {expected} argument."))
                .paragraph(USAGE),
            nonempty![RunHint::new(format!("rapport help {verb}"))],
        ),
        ParseError::InvalidArg {
            verb,
            value,
            reason,
        } => (
            vb.paragraph(format!("You ran: rapport {verb} {value}"))
                .paragraph(format!("{value} {reason}.")),
            nonempty![RunHint::new(format!("rapport help {verb}"))],
        ),
    };
    vb.next_actions(hints).build()
}

fn render_discovery_failure(command: &Command, path: &Utf8Path, err: &DiscoveryError) -> String {
    let vb = ViewBuilder::new().paragraph(format!("You ran: rapport {command} {path}"));
    let vb = match err {
        DiscoveryError::NoSupportedProject { start, git_root } => vb
            .paragraph(format!(
                "No supported project marker was found between {start} and git root {git_root}."
            ))
            .paragraph(format!("Expected {}.", describe_expected_markers())),
        DiscoveryError::NonUtf8Start { path } => {
            vb.paragraph(format!("{path} is not a UTF-8 path."))
        }
        DiscoveryError::OutsideGitRepository { start } => {
            vb.paragraph(format!("{start} is not inside a git repository."))
        }
        DiscoveryError::UnreadableStart { path, err } => {
            vb.paragraph(format!("Failed to inspect {path}: {err}."))
        }
        DiscoveryError::UnreadableDirectory { path, err } => {
            vb.paragraph(format!("Failed to inspect directory {path}: {err}."))
        }
    };
    vb.next_actions(nonempty![RunHint::new(format!("rapport help {command}"))])
        .build()
}

fn render_convention_failure(command: &Command, project: &Project, reason: &str) -> String {
    let hints = if project.is_gradle() && reason.contains("`./gradlew`") {
        nonempty![RunHint::new(format!(
            "cd {} && gradle wrapper",
            project.root
        ))]
    } else {
        nonempty![RunHint::new(format!(
            "edit {}/{}",
            project.root,
            project.marker()
        ))]
    };

    ViewBuilder::new()
        .paragraph(format!("You ran: rapport {command} {}", project.root))
        .paragraph(project.label())
        .paragraph(reason)
        .next_actions(hints)
        .build()
}

fn render_tool_resolution_failure(
    command: &Command,
    project: &Project,
    err: &ToolResolutionError,
) -> String {
    let vb = ViewBuilder::new()
        .paragraph(format!("You ran: rapport {command} {}", project.root))
        .paragraph(project.label());
    match err {
        ToolResolutionError::Convention(reason) => vb
            .paragraph(reason)
            .next_actions(nonempty![RunHint::new(format!(
                "edit {}/{}",
                project.root,
                project.marker()
            ))])
            .build(),
        ToolResolutionError::MissingSwift(io_err) => vb
            .paragraph(format!("Failed to invoke swift: {io_err}"))
            .paragraph(project.toolchain_install_hint().unwrap_or_default())
            .next_actions(nonempty![RunHint::new("swift --version")])
            .build(),
        ToolResolutionError::MissingFormatter {
            config,
            install_hint,
            first_probe,
            second_probe,
        } => {
            let vb = vb
                .paragraph(format!(
                    "SwiftPM formatter config `{config}` is present, but formatter tooling was not found."
                ))
                .paragraph(*install_hint);
            match second_probe {
                Some(second_probe) => vb
                    .next_actions(nonempty![
                        RunHint::new(*first_probe),
                        RunHint::new(*second_probe)
                    ])
                    .build(),
                None => vb
                    .next_actions(nonempty![RunHint::new(*first_probe)])
                    .build(),
            }
        }
        ToolResolutionError::MissingLinter {
            config,
            install_hint,
            probe,
        } => vb
            .paragraph(format!(
                "SwiftPM linter config `{config}` is present, but `swiftlint` was not found."
            ))
            .paragraph(*install_hint)
            .next_actions(nonempty![RunHint::new(*probe)])
            .build(),
        ToolResolutionError::MissingKustomizeRenderer => vb
            .paragraph("Kustomize renderer tooling was not found.")
            .paragraph(project.toolchain_install_hint().unwrap_or_default())
            .next_actions(nonempty![
                RunHint::new("kustomize version"),
                RunHint::new("kubectl version --client")
            ])
            .build(),
        ToolResolutionError::MissingKubernetesValidator => vb
            .paragraph("Kubernetes static validation tooling was not found.")
            .paragraph(
                "Install kubeconform from https://github.com/yannh/kubeconform and make sure `kubeconform` is on PATH.",
            )
            .next_actions(nonempty![RunHint::new("kubeconform -v")])
            .build(),
        ToolResolutionError::MissingTflint => vb
            .paragraph("Terraform lint tooling was not found.")
            .paragraph(convention::terraform::tflint_install_hint())
            .next_actions(nonempty![RunHint::new("tflint --version")])
            .build(),
        ToolResolutionError::ProbeInvoke { program, err } => vb
            .paragraph(format!("Failed to invoke {program}: {err}"))
            .next_actions(nonempty![RunHint::new(format!("{program} --version"))])
            .build(),
    }
}

fn render_pass(
    started: Instant,
    projects: &[Project],
    messages: &[String],
    hints: NonEmpty<RunHint>,
) -> String {
    let mut vb = ViewBuilder::new();
    if projects.len() > 1 {
        vb = vb.section("Targets", |b| {
            b.items(
                projects
                    .iter()
                    .map(|project| format!("pass - {}", project.label())),
            )
        });
    }
    if !messages.is_empty() {
        vb = vb.section("Output", |b| b.captured(messages.join("\n")));
    }
    vb.status(Outcome::Pass, started.elapsed())
        .next_actions(hints)
        .build()
}

fn render_doctor(
    started: Instant,
    reports: &[DoctorTargetReport],
    requested_path: &Utf8Path,
) -> String {
    let failed = reports.iter().any(DoctorTargetReport::has_failures);
    let mut vb = ViewBuilder::new().section("Targets", |b| {
        b.items(reports.iter().map(|report| {
            format!(
                "{} - {} (`{}`)",
                report.target.path, report.target.ecosystem, report.target.marker
            )
        }))
    });

    for report in reports {
        let title = format!("{} Target", report.target.ecosystem);
        vb = vb.section(&title, |b| {
            let target_lines = [
                format!("path: {}", report.target.path),
                format!("marker: `{}`", report.target.marker),
            ];
            let tool_lines = report.tools.iter().map(|check| format_check("tool", check));
            let config_lines = report
                .configuration
                .iter()
                .map(|check| format_check("config", check));
            b.items(
                target_lines
                    .into_iter()
                    .chain(tool_lines)
                    .chain(config_lines),
            )
        });
    }

    let outcome = if failed { Outcome::Fail } else { Outcome::Pass };
    let hint = if failed {
        RunHint::new(format!("rapport doctor {requested_path}"))
    } else {
        RunHint::new(format!("rapport validate {requested_path}"))
    };
    vb.status(outcome, started.elapsed())
        .next_actions(nonempty![hint])
        .build()
}

fn render_prime(reports: &[PrimeTargetReport], requested_path: &Utf8Path) -> String {
    let mut vb = prime_base_view().section("Targets", |b| {
        b.items(reports.iter().map(|report| {
            format!(
                "{} - {} (`{}`)",
                report.target.path, report.target.ecosystem, report.target.marker
            )
        }))
    });

    for report in reports {
        let title = format!("{} Convention", report.target.ecosystem);
        vb = vb.section(&title, |b| {
            let target_lines = [
                format!(
                    "target: {} (`{}`)",
                    report.target.path, report.target.marker
                ),
                format!(
                    "convention: {}",
                    format_prime_convention_status(&report.convention_status)
                ),
            ];
            b.items(
                target_lines.into_iter().chain(
                    report
                        .expected
                        .iter()
                        .map(|line| format!("expects: {line}")),
                ),
            )
        });
    }

    vb.next_actions(nonempty![RunHint::new(format!(
        "rapport doctor {requested_path}"
    ))])
    .build()
}

fn render_prime_no_targets(requested_path: &Utf8Path, git_root: &Utf8Path) -> String {
    prime_base_view()
        .section("Scope", |b| {
            b.items([
                format!("requested: {requested_path}"),
                format!("git root: {git_root}"),
                "detected targets: none".to_owned(),
            ])
        })
        .section("Supported Markers", |b| b.items(expected_marker_entries()))
        .next_actions(nonempty![RunHint::new("rapport help prime")])
        .build()
}

fn prime_base_view() -> ViewBuilder {
    ViewBuilder::new()
        .title("rapport prime - convention guide")
        .section("Purpose", |b| {
            b.items([
                "rapport gives agents one conventional dev-cycle surface across supported targets",
                "standard lifecycle verbs are `fix`, `lint`, `build`, `test`, `validate`, and `audit`",
                "use `prime` for convention guidance before edits; use `doctor` to check whether detected targets are runnable now",
            ])
        })
        .section("Boundary", |b| {
            b.items([
                "rapport owns lifecycle proof: format/fix, lint, build, test, validate, and audit",
                "task runners own installs, local servers, deploys, migrations, generators, releases, and bespoke workflows",
                "Justfiles and similar runners may call rapport when they need the standard lifecycle answer",
            ])
        })
}

fn format_prime_convention_status(status: &PrimeConventionStatus) -> String {
    match status {
        PrimeConventionStatus::Ok => "ok".to_owned(),
        PrimeConventionStatus::Missing(reason) => format!("missing - {reason}"),
    }
}

fn format_check(kind: &str, check: &DoctorCheck) -> String {
    let status = match check.status {
        DoctorStatus::Pass => "ok",
        DoctorStatus::Warn => "warn",
        DoctorStatus::Fail => "missing",
    };
    let mut parts = vec![
        format!("{kind} [{status}] {}", check.name),
        check.detail.clone(),
    ];
    if let Some(probe) = &check.probe {
        parts.push(format!("probe `{probe}`"));
    }
    if !check.affects.is_empty() {
        parts.push(format!("affects {}", format_verbs(&check.affects)));
    }
    if let Some(remediation) = &check.remediation {
        parts.push(remediation.clone());
    }
    parts.join("; ")
}

fn format_verbs(verbs: &[Verb]) -> String {
    verbs
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_step_failure(
    project: &Project,
    phase: Phase,
    outcome: &CommandOutcome,
    started: Instant,
    hints: NonEmpty<RunHint>,
    show_project_context: bool,
) -> String {
    let combined = combined_output(outcome);
    let failure_output = project.curate_failure_output(&combined);
    let mut vb = ViewBuilder::new();
    if show_project_context || project.should_report_failure_context() {
        vb = vb
            .paragraph(project.label())
            .paragraph(format!("Failing phase: {phase}"));
    }
    if !failure_output.is_empty() {
        vb = vb.section("Output", |b| b.captured(failure_output));
    }
    vb.status(Outcome::Fail, started.elapsed())
        .next_actions(hints)
        .build()
}

fn render_invoke_failure(
    command: &Command,
    project: &Project,
    spec: &CommandSpec,
    err: &io::Error,
) -> String {
    let vb = ViewBuilder::new().paragraph(format!("You ran: rapport {command} {}", project.root));
    let vb = if project.should_report_failure_context() {
        vb.paragraph(project.label())
    } else {
        vb
    }
    .paragraph(format!("Failed to invoke {}: {err}", spec.program));

    if project.is_swift_package_manager() && spec.program == project.primary_program() {
        vb.paragraph(project.toolchain_install_hint().unwrap_or_default())
            .next_actions(nonempty![RunHint::new("swift --version")])
            .build()
    } else if project.direct_formatter_program() == Some(spec.program.as_str()) {
        vb.paragraph(project.formatter_install_hint().unwrap_or_default())
            .next_actions(nonempty![RunHint::new("swift-format --version")])
            .build()
    } else if let Some(hint) = project.auxiliary_toolchain_install_hint(&spec.program) {
        vb.paragraph(hint)
            .next_actions(nonempty![RunHint::new(format!("which {}", spec.program))])
            .build()
    } else if spec.program == project.primary_program() {
        let vb = if let Some(hint) = project.toolchain_install_hint() {
            vb.paragraph(hint)
        } else {
            vb
        };
        vb.next_actions(nonempty![RunHint::new(format!(
            "which {}",
            project.primary_program()
        ))])
        .build()
    } else {
        vb.next_actions(nonempty![RunHint::new(format!("which {}", spec.program))])
            .build()
    }
}

fn combined_output(outcome: &CommandOutcome) -> String {
    let stderr = outcome.stderr.trim();
    let stdout = outcome.stdout.trim();
    match (stderr.is_empty(), stdout.is_empty()) {
        (true, true) => String::new(),
        (false, true) => stderr.to_owned(),
        (true, false) => stdout.to_owned(),
        (false, false) => format!("{stderr}\n\n{stdout}"),
    }
}

fn run_command<O, E>(
    command: &Command,
    runner: &dyn CommandRunner,
    fs: &impl FileSystem,
    out: &mut O,
    err: &mut E,
) -> ExitCode
where
    O: Write,
    E: Write,
{
    match command {
        Command::Doctor { .. } => return run_doctor(command, runner, fs, out, err),
        Command::Prime { .. } => return run_prime(command, fs, out, err),
        Command::Fix { .. }
        | Command::Lint { .. }
        | Command::Build { .. }
        | Command::Test { .. }
        | Command::Validate { .. }
        | Command::Audit { .. } => {}
    }

    let Some(verb) = command.lifecycle_verb() else {
        unreachable!("non-lifecycle commands are handled above");
    };
    let requested_path = command.path().as_path();
    let projects = match Project::discover_all(requested_path, fs) {
        Ok(projects) => projects,
        Err(discovery_err) => {
            let _ = writeln!(
                err,
                "{}",
                render_discovery_failure(command, requested_path, &discovery_err)
            );
            return ExitCode::from(2);
        }
    };

    let mut messages = Vec::new();
    let started = Instant::now();
    for project in &projects {
        if let Err(reason) = project.validate_manifest(fs) {
            let _ = writeln!(
                err,
                "{}",
                render_convention_failure(command, project, &reason)
            );
            return ExitCode::from(2);
        }

        let steps = match project.lifecycle_steps(verb, runner, fs) {
            Ok(steps) => steps,
            Err(tool_err) => {
                let _ = writeln!(
                    err,
                    "{}",
                    render_tool_resolution_failure(command, project, &tool_err)
                );
                return ExitCode::from(2);
            }
        };

        for step in steps {
            match step.action {
                LifecycleAction::Command(spec) => {
                    let outcome = match runner.run(&spec, &project.root) {
                        Ok(o) => o,
                        Err(io_err) => {
                            let _ = writeln!(
                                err,
                                "{}",
                                render_invoke_failure(command, project, &spec, &io_err)
                            );
                            return ExitCode::from(2);
                        }
                    };
                    if !outcome.success {
                        let hints = verb.hints(Outcome::Fail, &project.root);
                        let show_project_context = projects.len() > 1;
                        let _ = writeln!(
                            err,
                            "{}",
                            render_step_failure(
                                project,
                                step.phase,
                                &outcome,
                                started,
                                hints,
                                show_project_context
                            )
                        );
                        return ExitCode::from(1);
                    }
                }
                LifecycleAction::Message(message) => {
                    messages.push(format!("{}: {message}", project.label()));
                }
            }
        }
    }
    let hint_path = if let [project] = projects.as_slice() {
        project.root.as_path()
    } else {
        requested_path
    };
    let hints = verb.hints(Outcome::Pass, hint_path);
    let _ = writeln!(out, "{}", render_pass(started, &projects, &messages, hints));
    ExitCode::SUCCESS
}

fn run_prime<O, E>(command: &Command, fs: &impl FileSystem, out: &mut O, err: &mut E) -> ExitCode
where
    O: Write,
    E: Write,
{
    let requested_path = command.path().as_path();
    let projects = match Project::discover_all(requested_path, fs) {
        Ok(projects) => projects,
        Err(DiscoveryError::NoSupportedProject { git_root, .. }) => {
            let _ = writeln!(
                out,
                "{}",
                render_prime_no_targets(requested_path, &git_root)
            );
            return ExitCode::SUCCESS;
        }
        Err(discovery_err) => {
            let _ = writeln!(
                err,
                "{}",
                render_discovery_failure(command, requested_path, &discovery_err)
            );
            return ExitCode::from(2);
        }
    };

    let reports = projects
        .iter()
        .map(|project| project.prime_report(fs))
        .collect::<Vec<_>>();
    let _ = writeln!(out, "{}", render_prime(&reports, requested_path));
    ExitCode::SUCCESS
}

fn run_doctor<O, E>(
    command: &Command,
    runner: &dyn CommandRunner,
    fs: &impl FileSystem,
    out: &mut O,
    err: &mut E,
) -> ExitCode
where
    O: Write,
    E: Write,
{
    let requested_path = command.path().as_path();
    let projects = match Project::discover_all(requested_path, fs) {
        Ok(projects) => projects,
        Err(discovery_err) => {
            let _ = writeln!(
                err,
                "{}",
                render_discovery_failure(command, requested_path, &discovery_err)
            );
            return ExitCode::from(2);
        }
    };

    let started = Instant::now();
    let reports = projects
        .iter()
        .map(|project| project.doctor_report(runner, fs))
        .collect::<Vec<_>>();
    let failed = reports.iter().any(DoctorTargetReport::has_failures);
    let _ = writeln!(out, "{}", render_doctor(started, &reports, requested_path));
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use rapport_cli::{Utf8Path, Utf8PathBuf};
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_DIR_COUNTER: AtomicUsize = AtomicUsize::new(0);

    #[derive(Debug)]
    struct TestDir {
        path: Utf8PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            let dir = Self::new_without_git();
            dir.write(".git", "gitdir: test\n");
            dir
        }

        fn new_without_git() -> Self {
            let id = TEST_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = Utf8PathBuf::from_path_buf(
                std::env::temp_dir().join(format!("rapport-test-{}-{id}", std::process::id())),
            )
            .expect("temp dir path should be utf8");
            fs::create_dir_all(&path).expect("test directory should be created");
            let path = Utf8PathBuf::from_path_buf(
                fs::canonicalize(&path).expect("test directory should canonicalize"),
            )
            .expect("canonical test directory should be utf8");
            Self { path }
        }

        fn cargo_project() -> Self {
            let dir = Self::new();
            dir.write(
                "Cargo.toml",
                "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
            );
            dir
        }

        fn bun_project() -> Self {
            let dir = Self::new();
            dir.write("bun.lock", "");
            dir.write(
                "package.json",
                r#"{
  "name": "sample",
  "scripts": {
    "build": "bun build ./src/index.ts --outdir ./dist",
    "test": "bun test",
    "lint": "biome check .",
    "fix": "biome check --write .",
    "audit": "bun audit && bun run build --minify"
  }
}
"#,
            );
            dir.write("src/index.ts", "export const answer = 42;\n");
            dir
        }

        fn swift_project() -> Self {
            let dir = Self::new();
            dir.write(
                "Package.swift",
                "// swift-tools-version: 6.0\nimport PackageDescription\n",
            );
            dir.write(".swift-format", "{\n  \"version\": 1\n}\n");
            fs::create_dir_all(dir.path.join("Sources"))
                .expect("Sources directory should be created");
            dir.write("Sources/main.swift", "print(\"hello\")\n");
            dir
        }

        fn fastlane_project() -> Self {
            let dir = Self::new();
            dir.write(
                "Gemfile",
                "source \"https://rubygems.org\"\ngem \"fastlane\", \"2.228.0\"\n",
            );
            dir.write("fastlane/Fastfile", standard_fastfile());
            dir
        }

        fn kustomize_project() -> Self {
            let dir = Self::new();
            dir.write("kustomization.yaml", "resources:\n  - deployment.yaml\n");
            dir.write(
                "deployment.yaml",
                "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: app\n",
            );
            dir
        }

        fn zola_project() -> Self {
            let dir = Self::new();
            write_zola_site(&dir, ".");
            dir
        }

        fn terraform_project() -> Self {
            let dir = Self::new();
            dir.write(
                "main.tf",
                "terraform {\n  required_version = \">= 1.6.0\"\n}\n",
            );
            dir
        }

        fn gradle_project() -> Self {
            let dir = Self::new();
            dir.write("settings.gradle.kts", "rootProject.name = \"app\"\n");
            dir.write("gradlew", "#!/bin/sh\nexit 0\n");
            dir.write("build.gradle.kts", "// test fixture\n");
            dir
        }

        fn as_str(&self) -> &str {
            self.path.as_str()
        }

        fn write(&self, relative: &str, contents: &str) {
            let path = self.path.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("parent directory should be created");
            }
            fs::write(path, contents).expect("test file should be written");
        }

        fn write_cargo_package(&self, relative: &str, name: &str) {
            self.write(
                &format!("{relative}/Cargo.toml"),
                &format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"),
            );
            self.write(
                &format!("{relative}/src/lib.rs"),
                "pub fn answer() -> u8 { 42 }\n",
            );
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct RecordedCommand {
        program: String,
        args: Vec<String>,
        cwd: Utf8PathBuf,
    }

    #[derive(Debug)]
    struct FakeRunner {
        outcomes: RefCell<VecDeque<io::Result<CommandOutcome>>>,
        calls: RefCell<Vec<RecordedCommand>>,
    }

    impl FakeRunner {
        fn new(outcomes: Vec<io::Result<CommandOutcome>>) -> Self {
            Self {
                outcomes: RefCell::new(outcomes.into()),
                calls: RefCell::new(Vec::new()),
            }
        }

        fn all_pass(count: usize) -> Self {
            let outcomes = (0..count).map(|_| Ok(pass())).collect();
            Self::new(outcomes)
        }

        fn calls(&self) -> Vec<RecordedCommand> {
            self.calls.borrow().clone()
        }
    }

    impl CommandRunner for FakeRunner {
        fn run(&self, spec: &CommandSpec, cwd: &Utf8Path) -> io::Result<CommandOutcome> {
            self.calls.borrow_mut().push(RecordedCommand {
                program: spec.program.clone(),
                args: spec.args.clone(),
                cwd: cwd.to_owned(),
            });
            self.outcomes
                .borrow_mut()
                .pop_front()
                .expect("fake runner should have an outcome for each command")
        }
    }

    fn pass() -> CommandOutcome {
        CommandOutcome {
            success: true,
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    fn fail(stdout: &str, stderr: &str) -> CommandOutcome {
        CommandOutcome {
            success: false,
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    fn standard_fastfile() -> &'static str {
        "lane :build do\nend\n\
         lane :test do\nend\n\
         lane :lint do\nend\n\
         lane :fix do\nend\n\
         lane :validate do\nend\n\
         lane :audit do\nend\n"
    }

    fn write_zola_site(dir: &TestDir, root: &str) {
        let prefix = if root == "." {
            String::new()
        } else {
            format!("{root}/")
        };
        dir.write(
            &format!("{prefix}config.toml"),
            "base_url = \"https://example.com\"\n\n[markdown]\nhighlighting_theme = \"base16-ocean-dark\"\n",
        );
        dir.write(
            &format!("{prefix}content/_index.md"),
            "+++\ntitle = \"Home\"\n+++\n",
        );
        dir.write(
            &format!("{prefix}templates/index.html"),
            "{{ section.title }}\n",
        );
    }

    fn run_with(args: &[&str], runner: &dyn CommandRunner) -> (ExitCode, String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run(
            args.iter().map(|arg| (*arg).to_owned()),
            runner,
            &mut out,
            &mut err,
        );
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn build_runs_cargo_check_in_the_given_directory() {
        let dir = TestDir::cargo_project();
        let runner = FakeRunner::all_pass(1);

        let (code, out, err) = run_with(&["build", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains("status: pass"));
        assert!(out.contains(&format!("└ run rapport test {}", dir.as_str())));
        assert_eq!(
            runner.calls(),
            vec![RecordedCommand {
                program: "cargo".into(),
                args: vec!["check".into(), "--package".into(), "sample".into()],
                cwd: dir.path.clone(),
            }]
        );
    }

    #[test]
    fn doctor_checks_cargo_readiness_without_running_lifecycle_work() {
        let dir = TestDir::cargo_project();
        let runner = FakeRunner::all_pass(4);

        let (code, out, err) = run_with(&["doctor", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains("## Targets"));
        assert!(out.contains("Cargo (`Cargo.toml`)"));
        assert!(out.contains("tool [ok] cargo; usable on PATH; probe `cargo --version`"));
        assert!(out.contains("tool [ok] cargo fmt; usable on PATH; probe `cargo fmt --version`"));
        assert!(
            out.contains(
                "tool [ok] cargo nextest; usable on PATH; probe `cargo nextest --version`"
            )
        );
        assert!(out.contains("config [ok] Cargo.toml; present"));
        assert!(out.contains("status: pass"));
        assert_eq!(
            runner.calls(),
            vec![
                RecordedCommand {
                    program: "cargo".into(),
                    args: vec!["--version".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "cargo".into(),
                    args: vec!["fmt".into(), "--version".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "cargo".into(),
                    args: vec!["clippy".into(), "--version".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "cargo".into(),
                    args: vec!["nextest".into(), "--version".into()],
                    cwd: dir.path.clone(),
                },
            ]
        );
    }

    #[test]
    fn doctor_fails_when_required_bun_script_is_missing() {
        let dir = TestDir::bun_project();
        dir.write(
            "package.json",
            r#"{
  "name": "sample",
  "scripts": {
    "test": "bun test",
    "lint": "biome check .",
    "fix": "biome check --write .",
    "audit": "bun audit"
  }
}
"#,
        );
        let runner = FakeRunner::all_pass(1);

        let (code, out, err) = run_with(&["doctor", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::from(1));
        assert_eq!(err, "");
        assert!(out.contains("config [missing] package.json script `build`; missing"));
        assert!(out.contains("affects build, validate"));
        assert!(out.contains("status: FAIL"));
        assert_eq!(
            runner.calls(),
            vec![RecordedCommand {
                program: "bun".into(),
                args: vec!["--version".into()],
                cwd: dir.path.clone(),
            }]
        );
    }

    #[test]
    fn doctor_uses_same_mixed_scope_discovery_as_lifecycle_commands() {
        let dir = TestDir::new();
        dir.write_cargo_package("apps/api", "api");
        dir.write(
            "infra/main.tf",
            "resource \"null_resource\" \"example\" {}\n",
        );
        let runner = FakeRunner::all_pass(6);

        let (code, out, err) = run_with(&["doctor", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains("Cargo (`Cargo.toml`)"));
        assert!(out.contains("Terraform (`*.tf`)"));
        assert!(out.contains("config [ok] Terraform `*.tf` files; present"));
        assert!(out.contains("status: pass"));
        assert_eq!(
            runner.calls(),
            vec![
                RecordedCommand {
                    program: "cargo".into(),
                    args: vec!["--version".into()],
                    cwd: dir.path.join("apps/api"),
                },
                RecordedCommand {
                    program: "cargo".into(),
                    args: vec!["fmt".into(), "--version".into()],
                    cwd: dir.path.join("apps/api"),
                },
                RecordedCommand {
                    program: "cargo".into(),
                    args: vec!["clippy".into(), "--version".into()],
                    cwd: dir.path.join("apps/api"),
                },
                RecordedCommand {
                    program: "cargo".into(),
                    args: vec!["nextest".into(), "--version".into()],
                    cwd: dir.path.join("apps/api"),
                },
                RecordedCommand {
                    program: "terraform".into(),
                    args: vec!["--version".into()],
                    cwd: dir.path.join("infra"),
                },
                RecordedCommand {
                    program: "tflint".into(),
                    args: vec!["--version".into()],
                    cwd: dir.path.join("infra"),
                },
            ]
        );
    }

    #[test]
    fn help_lists_prime_as_a_cli_verb() {
        let runner = FakeRunner::all_pass(0);

        let (code, out, err) = run_with(&["help"], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains("`prime`"));
        assert!(out.contains("Explain rapport conventions"));
        assert_eq!(runner.calls(), Vec::new());
    }

    #[test]
    fn prime_reports_cargo_conventions_without_running_tool_probes() {
        let dir = TestDir::cargo_project();
        let runner = FakeRunner::all_pass(0);

        let (code, out, err) = run_with(&["prime", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains("# rapport prime"));
        assert!(out.contains("use `prime` for convention guidance before edits"));
        assert!(out.contains(&format!("{} - Cargo (`Cargo.toml`)", dir.as_str())));
        assert!(out.contains("convention: ok"));
        assert!(out.contains("expects: requires `Cargo.toml`"));
        assert!(out.contains("expects: build uses `cargo check`"));
        assert!(out.contains(&format!("└ run rapport doctor {}", dir.as_str())));
        assert_eq!(runner.calls(), Vec::new());
    }

    #[test]
    fn prime_uses_same_mixed_scope_discovery_as_lifecycle_commands() {
        let dir = TestDir::new();
        dir.write_cargo_package("apps/api", "api");
        dir.write(
            "infra/main.tf",
            "resource \"null_resource\" \"example\" {}\n",
        );
        let runner = FakeRunner::all_pass(0);

        let (code, out, err) = run_with(&["prime", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains("Cargo (`Cargo.toml`)"));
        assert!(out.contains("Terraform (`*.tf`)"));
        assert!(out.contains("expects: optional `.tflint.hcl` enables required TFLint"));
        assert_eq!(runner.calls(), Vec::new());
    }

    #[test]
    fn prime_reports_missing_convention_without_failing_the_guidance_run() {
        let dir = TestDir::new();
        dir.write("fastlane/Fastfile", standard_fastfile());
        let runner = FakeRunner::all_pass(0);

        let (code, out, err) = run_with(&["prime", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains("Fastlane (`fastlane/Fastfile`)"));
        assert!(out.contains("convention: missing - Fastlane projects must include a `Gemfile`"));
        assert!(out.contains("expects: requires `Gemfile` and `fastlane/Fastfile`"));
        assert_eq!(runner.calls(), Vec::new());
    }

    #[test]
    fn prime_reports_no_supported_targets_inside_a_git_scope() {
        let dir = TestDir::new();
        dir.write("README.md", "# no supported targets here\n");
        let runner = FakeRunner::all_pass(0);

        let (code, out, err) = run_with(&["prime", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains("detected targets: none"));
        assert!(out.contains("## Supported Markers"));
        assert!(out.contains("`Cargo.toml` for Cargo"));
        assert!(out.contains("`*.tf` for Terraform"));
        assert!(out.contains("└ run rapport help prime"));
        assert_eq!(runner.calls(), Vec::new());
    }

    #[test]
    fn validate_runs_lint_build_test_pipeline() {
        let dir = TestDir::cargo_project();
        let runner = FakeRunner::all_pass(5);

        let (code, out, err) = run_with(&["validate", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains(&format!("└ run rapport audit {}", dir.as_str())));
        assert_eq!(
            runner.calls(),
            vec![
                RecordedCommand {
                    program: "cargo".into(),
                    args: vec!["nextest".into(), "--version".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "cargo".into(),
                    args: vec![
                        "fmt".into(),
                        "--package".into(),
                        "sample".into(),
                        "--".into(),
                        "--check".into(),
                    ],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "cargo".into(),
                    args: vec![
                        "clippy".into(),
                        "--package".into(),
                        "sample".into(),
                        "--all-targets".into(),
                        "--".into(),
                        "-D".into(),
                        "warnings".into(),
                    ],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "cargo".into(),
                    args: vec!["check".into(), "--package".into(), "sample".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "cargo".into(),
                    args: vec![
                        "nextest".into(),
                        "run".into(),
                        "--package".into(),
                        "sample".into()
                    ],
                    cwd: dir.path.clone(),
                },
            ]
        );
    }

    #[test]
    fn test_prefers_nextest_but_falls_back_to_cargo_test_when_missing() {
        let dir = TestDir::cargo_project();
        let runner = FakeRunner::new(vec![Ok(fail("", "missing nextest")), Ok(pass())]);

        let (code, out, err) = run_with(&["test", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains(&format!("└ run rapport validate {}", dir.as_str())));
        assert_eq!(
            runner.calls(),
            vec![
                RecordedCommand {
                    program: "cargo".into(),
                    args: vec!["nextest".into(), "--version".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "cargo".into(),
                    args: vec!["test".into(), "--package".into(), "sample".into()],
                    cwd: dir.path.clone(),
                },
            ]
        );
    }

    #[test]
    fn cargo_metadata_supplies_conventional_feature_and_target_args() {
        let dir = TestDir::new();
        dir.write(
            "Cargo.toml",
            "[package]\n\
             name = \"sample\"\n\
             version = \"0.1.0\"\n\
             edition = \"2024\"\n\n\
             [features]\n\
             extra = []\n\n\
             [package.metadata.rapport.cargo]\n\
             no-default-features = true\n\
             features = [\"extra\"]\n\
             target = \"wasm32-unknown-unknown\"\n",
        );
        let runner = FakeRunner::all_pass(1);

        let (code, _out, err) = run_with(&["build", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![RecordedCommand {
                program: "cargo".into(),
                args: vec![
                    "check".into(),
                    "--package".into(),
                    "sample".into(),
                    "--no-default-features".into(),
                    "--features".into(),
                    "extra".into(),
                    "--target".into(),
                    "wasm32-unknown-unknown".into(),
                ],
                cwd: dir.path.clone(),
            }]
        );
    }

    #[test]
    fn child_directory_runs_nearest_parent_cargo_project() {
        let dir = TestDir::cargo_project();
        let crate_dir = dir.path.join("crates/app");
        fs::create_dir_all(crate_dir.join("src")).expect("crate src directory should be created");
        fs::write(
            crate_dir.join("Cargo.toml"),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .expect("nested Cargo.toml should be written");
        let crate_child = crate_dir.join("src");
        let runner = FakeRunner::all_pass(1);

        let (code, out, err) = run_with(&["build", crate_child.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains(&format!("└ run rapport test {crate_dir}")));
        assert_eq!(
            runner.calls(),
            vec![RecordedCommand {
                program: "cargo".into(),
                args: vec!["check".into(), "--package".into(), "app".into()],
                cwd: crate_dir,
            }]
        );
    }

    #[test]
    fn git_root_is_used_when_it_is_the_only_cargo_project() {
        let dir = TestDir::cargo_project();
        fs::create_dir_all(dir.path.join("src/deep")).expect("child directory should be created");
        let child = dir.path.join("src/deep");
        let runner = FakeRunner::all_pass(1);

        let (code, _out, err) = run_with(&["build", child.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![RecordedCommand {
                program: "cargo".into(),
                args: vec!["check".into(), "--package".into(), "sample".into()],
                cwd: dir.path.clone(),
            }]
        );
    }

    #[test]
    fn step_failure_stops_pipeline_and_reports_captured_output() {
        let dir = TestDir::cargo_project();
        let runner = FakeRunner::new(vec![
            Ok(pass()),
            Ok(fail("stdout details", "stderr details")),
        ]);

        let (code, out, err) = run_with(&["lint", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::from(1));
        assert_eq!(out, "");
        assert!(err.contains("## Output"));
        assert!(err.contains("stderr details"));
        assert!(err.contains("stdout details"));
        assert!(err.contains("status: FAIL"));
        assert!(err.contains(&format!("└ run rapport fix {}", dir.as_str())));
        assert_eq!(runner.calls().len(), 2);
    }

    #[test]
    fn invoke_failure_reports_recovery_hint() {
        let dir = TestDir::cargo_project();
        let runner = FakeRunner::new(vec![Err(io::Error::new(
            io::ErrorKind::NotFound,
            "missing cargo",
        ))]);

        let (code, out, err) = run_with(&["build", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains(&format!("You ran: rapport build {}", dir.as_str())));
        assert!(err.contains("Failed to invoke cargo: missing cargo"));
        assert!(err.contains("└ run which cargo"));
    }

    #[test]
    fn missing_project_marker_errors_before_running_any_commands() {
        let dir = TestDir::new();
        let runner = FakeRunner::all_pass(1);

        let (code, out, err) = run_with(&["build", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("No supported project marker was found"));
        assert!(err.contains("Cargo.toml"));
        assert!(err.contains("package.json"));
        assert!(err.contains("Package.swift"));
        assert!(err.contains("fastlane/Fastfile"));
        assert!(err.contains("settings.gradle.kts"));
        assert!(err.contains("config.toml"));
        assert!(err.contains("kustomization.yaml"));
        assert!(err.contains("*.tf"));
        assert_eq!(runner.calls(), Vec::new());
    }

    #[test]
    fn cargo_discovery_walks_up_from_child_path() {
        let dir = TestDir::cargo_project();
        fs::create_dir_all(dir.path.join("src/bin")).expect("child directory should be created");
        let child = dir.path.join("src/bin");
        let runner = FakeRunner::all_pass(1);

        let (code, _out, err) = run_with(&["build", child.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![RecordedCommand {
                program: "cargo".into(),
                args: vec!["check".into(), "--package".into(), "sample".into()],
                cwd: dir.path.clone(),
            }]
        );
    }

    #[test]
    fn malformed_swift_tools_version_errors_before_running_any_commands() {
        let dir = TestDir::new();
        dir.write("Package.swift", "import PackageDescription\n");
        let runner = FakeRunner::all_pass(1);

        let (code, out, err) = run_with(&["build", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("Package.swift"));
        assert!(err.contains("swift-tools-version"));
        assert_eq!(runner.calls(), Vec::new());
    }

    #[test]
    fn swift_build_runs_without_resolving_formatter() {
        let dir = TestDir::swift_project();
        let runner = FakeRunner::all_pass(1);

        let (code, out, err) = run_with(&["build", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains(&format!("└ run rapport test {}", dir.as_str())));
        assert_eq!(
            runner.calls(),
            vec![RecordedCommand {
                program: "swift".into(),
                args: vec!["build".into()],
                cwd: dir.path.clone(),
            }]
        );
    }

    #[test]
    fn swift_test_runs_without_resolving_formatter() {
        let dir = TestDir::swift_project();
        let runner = FakeRunner::all_pass(1);

        let (code, _out, err) = run_with(&["test", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![RecordedCommand {
                program: "swift".into(),
                args: vec!["test".into()],
                cwd: dir.path.clone(),
            }]
        );
    }

    #[test]
    fn swift_lint_prefers_driver_formatter() {
        let dir = TestDir::swift_project();
        let runner = FakeRunner::all_pass(2);

        let (code, _out, err) = run_with(&["lint", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![
                RecordedCommand {
                    program: "swift".into(),
                    args: vec!["format".into(), "--version".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "swift".into(),
                    args: vec![
                        "format".into(),
                        "lint".into(),
                        "--strict".into(),
                        "--recursive".into(),
                        "--no-color-diagnostics".into(),
                        "--configuration".into(),
                        ".swift-format".into(),
                        "Package.swift".into(),
                        "Sources".into(),
                    ],
                    cwd: dir.path.clone(),
                },
            ]
        );
    }

    #[test]
    fn swift_lint_runs_configured_linter_after_formatter_check() {
        let dir = TestDir::swift_project();
        dir.write(".swiftlint.yml", "disabled_rules: []\n");
        let runner = FakeRunner::all_pass(4);

        let (code, _out, err) = run_with(&["lint", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![
                RecordedCommand {
                    program: "swift".into(),
                    args: vec!["format".into(), "--version".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "swiftlint".into(),
                    args: vec!["version".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "swift".into(),
                    args: vec![
                        "format".into(),
                        "lint".into(),
                        "--strict".into(),
                        "--recursive".into(),
                        "--no-color-diagnostics".into(),
                        "--configuration".into(),
                        ".swift-format".into(),
                        "Package.swift".into(),
                        "Sources".into(),
                    ],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "swiftlint".into(),
                    args: vec![
                        "lint".into(),
                        "--strict".into(),
                        "--config".into(),
                        ".swiftlint.yml".into(),
                    ],
                    cwd: dir.path.clone(),
                },
            ]
        );
    }

    #[test]
    fn swift_lint_without_style_configs_is_a_noop() {
        let dir = TestDir::new();
        dir.write(
            "Package.swift",
            "// swift-tools-version: 6.0\nimport PackageDescription\n",
        );
        dir.write("Sources/main.swift", "print(\"hello\")\n");
        let runner = FakeRunner::all_pass(0);

        let (code, out, err) = run_with(&["lint", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains("status: pass"));
        assert_eq!(runner.calls(), Vec::new());
    }

    #[test]
    fn swift_fix_falls_back_to_direct_formatter() {
        let dir = TestDir::swift_project();
        let runner = FakeRunner::new(vec![
            Ok(fail("", "unknown subcommand")),
            Ok(pass()),
            Ok(pass()),
        ]);

        let (code, _out, err) = run_with(&["fix", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![
                RecordedCommand {
                    program: "swift".into(),
                    args: vec!["format".into(), "--version".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "swift-format".into(),
                    args: vec!["--version".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "swift-format".into(),
                    args: vec![
                        "format".into(),
                        "--in-place".into(),
                        "--recursive".into(),
                        "--no-color-diagnostics".into(),
                        "--configuration".into(),
                        ".swift-format".into(),
                        "Package.swift".into(),
                        "Sources".into(),
                    ],
                    cwd: dir.path.clone(),
                },
            ]
        );
    }

    #[test]
    fn swift_validate_runs_lint_build_test_pipeline() {
        let dir = TestDir::swift_project();
        let runner = FakeRunner::all_pass(4);

        let (code, _out, err) = run_with(&["validate", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![
                RecordedCommand {
                    program: "swift".into(),
                    args: vec!["format".into(), "--version".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "swift".into(),
                    args: vec![
                        "format".into(),
                        "lint".into(),
                        "--strict".into(),
                        "--recursive".into(),
                        "--no-color-diagnostics".into(),
                        "--configuration".into(),
                        ".swift-format".into(),
                        "Package.swift".into(),
                        "Sources".into(),
                    ],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "swift".into(),
                    args: vec!["build".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "swift".into(),
                    args: vec!["test".into()],
                    cwd: dir.path.clone(),
                },
            ]
        );
    }

    #[test]
    fn swift_audit_runs_validate_then_release_build() {
        let dir = TestDir::swift_project();
        let runner = FakeRunner::all_pass(5);

        let (code, _out, err) = run_with(&["audit", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![
                RecordedCommand {
                    program: "swift".into(),
                    args: vec!["format".into(), "--version".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "swift".into(),
                    args: vec![
                        "format".into(),
                        "lint".into(),
                        "--strict".into(),
                        "--recursive".into(),
                        "--no-color-diagnostics".into(),
                        "--configuration".into(),
                        ".swift-format".into(),
                        "Package.swift".into(),
                        "Sources".into(),
                    ],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "swift".into(),
                    args: vec!["build".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "swift".into(),
                    args: vec!["test".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "swift".into(),
                    args: vec!["build".into(), "-c".into(), "release".into()],
                    cwd: dir.path.clone(),
                },
            ]
        );
    }

    #[test]
    fn swift_missing_tool_reports_cross_platform_install_hint() {
        let dir = TestDir::swift_project();
        let runner = FakeRunner::new(vec![Err(io::Error::new(
            io::ErrorKind::NotFound,
            "missing swift",
        ))]);

        let (code, out, err) = run_with(&["build", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("Failed to invoke swift: missing swift"));
        assert!(err.contains("https://www.swift.org/install/"));
        assert!(!err.contains("Homebrew"));
    }

    #[test]
    fn swift_missing_formatter_blocks_lint_but_not_build() {
        let dir = TestDir::swift_project();
        let runner = FakeRunner::new(vec![
            Ok(fail("", "unknown subcommand")),
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "missing swift-format",
            )),
        ]);

        let (code, out, err) = run_with(&["lint", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("SwiftPM formatter config `.swift-format` is present"));
        assert!(err.contains("swift format --version"));
        assert!(err.contains("swift-format --version"));
        assert_eq!(runner.calls().len(), 2);
    }

    #[test]
    fn swift_discovery_walks_up_from_child_path() {
        let dir = TestDir::swift_project();
        fs::create_dir_all(dir.path.join("Sources/App"))
            .expect("child directory should be created");
        let child = dir.path.join("Sources/App");
        let runner = FakeRunner::all_pass(1);

        let (code, _out, err) = run_with(&["build", child.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![RecordedCommand {
                program: "swift".into(),
                args: vec!["build".into()],
                cwd: dir.path.clone(),
            }]
        );
    }

    #[test]
    fn bun_build_runs_package_script() {
        let dir = TestDir::bun_project();
        let runner = FakeRunner::all_pass(1);

        let (code, out, err) = run_with(&["build", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains(&format!("└ run rapport test {}", dir.as_str())));
        assert_eq!(
            runner.calls(),
            vec![RecordedCommand {
                program: "bun".into(),
                args: vec!["run".into(), "build".into()],
                cwd: dir.path.clone(),
            }]
        );
    }

    #[test]
    fn bun_validate_composes_lint_build_test_scripts() {
        let dir = TestDir::bun_project();
        let runner = FakeRunner::all_pass(3);

        let (code, _out, err) = run_with(&["validate", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![
                RecordedCommand {
                    program: "bun".into(),
                    args: vec!["run".into(), "lint".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "bun".into(),
                    args: vec!["run".into(), "build".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "bun".into(),
                    args: vec!["run".into(), "test".into()],
                    cwd: dir.path.clone(),
                },
            ]
        );
    }

    #[test]
    fn bun_package_can_use_ancestor_lockfile() {
        let dir = TestDir::new();
        dir.write("bun.lock", "");
        dir.write(
            "packages/app/package.json",
            r#"{
  "name": "app",
  "scripts": {
    "build": "bun build ./src/index.ts --outdir ./dist"
  }
}
"#,
        );
        dir.write("packages/app/src/index.ts", "export const answer = 42;\n");
        let app = dir.path.join("packages/app");
        let runner = FakeRunner::all_pass(1);

        let (code, _out, err) = run_with(&["build", app.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![RecordedCommand {
                program: "bun".into(),
                args: vec!["run".into(), "build".into()],
                cwd: app,
            }]
        );
    }

    #[test]
    fn bun_workspace_root_without_scripts_aggregates_child_packages() {
        let dir = TestDir::new();
        dir.write("bun.lock", "");
        dir.write(
            "package.json",
            r#"{
  "name": "workspace",
  "workspaces": ["packages/*"]
}
"#,
        );
        dir.write(
            "packages/api/package.json",
            r#"{
  "name": "api",
  "scripts": {
    "build": "bun build ./src/index.ts --outdir ./dist"
  }
}
"#,
        );
        dir.write("packages/api/src/index.ts", "export const api = 1;\n");
        dir.write(
            "packages/web/package.json",
            r#"{
  "name": "web",
  "scripts": {
    "build": "bun build ./src/index.ts --outdir ./dist"
  }
}
"#,
        );
        dir.write("packages/web/src/index.ts", "export const web = 1;\n");
        let runner = FakeRunner::all_pass(2);

        let (code, _out, err) = run_with(&["build", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![
                RecordedCommand {
                    program: "bun".into(),
                    args: vec!["run".into(), "build".into()],
                    cwd: dir.path.join("packages/api"),
                },
                RecordedCommand {
                    program: "bun".into(),
                    args: vec!["run".into(), "build".into()],
                    cwd: dir.path.join("packages/web"),
                },
            ]
        );
    }

    #[test]
    fn bun_missing_script_errors_before_running_any_commands() {
        let dir = TestDir::new();
        dir.write("bun.lock", "");
        dir.write(
            "package.json",
            r#"{
  "name": "sample",
  "scripts": {
    "test": "bun test"
  }
}
"#,
        );
        let runner = FakeRunner::all_pass(1);

        let (code, out, err) = run_with(&["build", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("Bun project must define script `build`"));
        assert_eq!(runner.calls(), Vec::new());
    }

    #[test]
    fn bun_missing_tool_reports_install_hint() {
        let dir = TestDir::bun_project();
        let runner = FakeRunner::new(vec![Err(io::Error::new(
            io::ErrorKind::NotFound,
            "missing bun",
        ))]);

        let (code, out, err) = run_with(&["build", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("Bun project"));
        assert!(err.contains("Failed to invoke bun: missing bun"));
        assert!(err.contains("https://bun.sh/docs/installation"));
        assert!(err.contains("└ run which bun"));
    }

    #[test]
    fn zola_build_runs_attached_bun_build_then_zola_build() {
        let dir = TestDir::zola_project();
        dir.write("bun.lock", "");
        dir.write(
            "package.json",
            r#"{
  "name": "site",
  "scripts": {
    "build": "tailwindcss -i ./src/input.css -o ./static/css/style.css"
  }
}
"#,
        );
        let runner = FakeRunner::all_pass(2);

        let (code, out, err) = run_with(&["build", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains(&format!("└ run rapport test {}", dir.as_str())));
        assert_eq!(
            runner.calls(),
            vec![
                RecordedCommand {
                    program: "bun".into(),
                    args: vec!["run".into(), "build".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "zola".into(),
                    args: vec!["build".into()],
                    cwd: dir.path.clone(),
                },
            ]
        );
    }

    #[test]
    fn zola_validate_uses_check_script_and_avoids_duplicate_zola_check() {
        let dir = TestDir::zola_project();
        dir.write("bun.lock", "");
        dir.write(
            "package.json",
            r#"{
  "name": "site",
  "scripts": {
    "check": "prettier --check .",
    "build": "tailwindcss -i ./src/input.css -o ./static/css/style.css",
    "test": "bun test"
  }
}
"#,
        );
        let runner = FakeRunner::all_pass(5);

        let (code, _out, err) = run_with(&["validate", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![
                RecordedCommand {
                    program: "bun".into(),
                    args: vec!["run".into(), "check".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "zola".into(),
                    args: vec!["check".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "bun".into(),
                    args: vec!["run".into(), "build".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "zola".into(),
                    args: vec!["build".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "bun".into(),
                    args: vec!["run".into(), "test".into()],
                    cwd: dir.path.clone(),
                },
            ]
        );
    }

    #[test]
    fn zola_fix_without_bun_fix_is_a_clear_noop() {
        let dir = TestDir::zola_project();
        let runner = FakeRunner::all_pass(0);

        let (code, out, err) = run_with(&["fix", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains("Zola has no autofix"));
        assert_eq!(runner.calls(), Vec::new());
    }

    #[test]
    fn zola_missing_template_directory_errors_before_running_commands() {
        let dir = TestDir::new();
        dir.write(
            "config.toml",
            "base_url = \"https://example.com\"\n\n[markdown]\nhighlighting_theme = \"base16-ocean-dark\"\n",
        );
        dir.write("content/_index.md", "+++\ntitle = \"Home\"\n+++\n");
        let runner = FakeRunner::all_pass(1);

        let (code, out, err) = run_with(&["build", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("templates/"));
        assert_eq!(runner.calls(), Vec::new());
    }

    #[test]
    fn zola_reports_bun_install_hint_for_attached_script() {
        let dir = TestDir::zola_project();
        dir.write("bun.lock", "");
        dir.write(
            "package.json",
            r#"{
  "name": "site",
  "scripts": {
    "build": "tailwindcss -i ./src/input.css -o ./static/css/style.css"
  }
}
"#,
        );
        let runner = FakeRunner::new(vec![Err(io::Error::new(
            io::ErrorKind::NotFound,
            "missing bun",
        ))]);

        let (code, out, err) = run_with(&["build", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("Zola project"));
        assert!(err.contains("Failed to invoke bun: missing bun"));
        assert!(err.contains("https://bun.sh/docs/installation"));
        assert!(err.contains("└ run which bun"));
    }

    #[test]
    fn zola_wins_over_colocated_bun_package() {
        let dir = TestDir::zola_project();
        dir.write("bun.lock", "");
        dir.write(
            "package.json",
            r#"{
  "name": "site",
  "scripts": {
    "build": "tailwindcss -i ./src/input.css -o ./static/css/style.css"
  }
}
"#,
        );
        let runner = FakeRunner::all_pass(2);

        let (code, _out, err) = run_with(&["build", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![
                RecordedCommand {
                    program: "bun".into(),
                    args: vec!["run".into(), "build".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "zola".into(),
                    args: vec!["build".into()],
                    cwd: dir.path.clone(),
                },
            ]
        );
    }

    #[test]
    fn bun_workspace_root_without_scripts_aggregates_zola_sites() {
        let dir = TestDir::new();
        dir.write("web/bun.lock", "");
        dir.write(
            "web/package.json",
            r#"{
  "name": "website",
  "private": true,
  "workspaces": ["sites/*"]
}
"#,
        );
        for site in ["company", "product"] {
            write_zola_site(&dir, &format!("web/sites/{site}"));
            dir.write(
                &format!("web/sites/{site}/package.json"),
                r#"{
  "name": "site",
  "scripts": {
    "build": "tailwindcss -i ./src/input.css -o ./static/css/style.css"
  }
}
"#,
            );
        }
        let web = dir.path.join("web");
        let runner = FakeRunner::all_pass(4);

        let (code, _out, err) = run_with(&["build", web.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![
                RecordedCommand {
                    program: "bun".into(),
                    args: vec!["run".into(), "build".into()],
                    cwd: dir.path.join("web/sites/company"),
                },
                RecordedCommand {
                    program: "zola".into(),
                    args: vec!["build".into()],
                    cwd: dir.path.join("web/sites/company"),
                },
                RecordedCommand {
                    program: "bun".into(),
                    args: vec!["run".into(), "build".into()],
                    cwd: dir.path.join("web/sites/product"),
                },
                RecordedCommand {
                    program: "zola".into(),
                    args: vec!["build".into()],
                    cwd: dir.path.join("web/sites/product"),
                },
            ]
        );
    }

    #[test]
    fn fastlane_build_runs_bundle_exec_fastlane_lane() {
        let dir = TestDir::fastlane_project();
        let runner = FakeRunner::all_pass(1);

        let (code, out, err) = run_with(&["build", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains(&format!("└ run rapport test {}", dir.as_str())));
        assert_eq!(
            runner.calls(),
            vec![RecordedCommand {
                program: "bundle".into(),
                args: vec!["exec".into(), "fastlane".into(), "build".into()],
                cwd: dir.path.clone(),
            }]
        );
    }

    #[test]
    fn fastlane_validate_runs_validate_lane_directly() {
        let dir = TestDir::fastlane_project();
        let runner = FakeRunner::all_pass(1);

        let (code, _out, err) = run_with(&["validate", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![RecordedCommand {
                program: "bundle".into(),
                args: vec!["exec".into(), "fastlane".into(), "validate".into()],
                cwd: dir.path.clone(),
            }]
        );
    }

    #[test]
    fn fastlane_audit_runs_audit_lane_directly() {
        let dir = TestDir::fastlane_project();
        let runner = FakeRunner::all_pass(1);

        let (code, _out, err) = run_with(&["audit", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![RecordedCommand {
                program: "bundle".into(),
                args: vec!["exec".into(), "fastlane".into(), "audit".into()],
                cwd: dir.path.clone(),
            }]
        );
    }

    #[test]
    fn fastlane_missing_gemfile_errors_before_running_any_commands() {
        let dir = TestDir::new();
        dir.write("fastlane/Fastfile", standard_fastfile());
        let runner = FakeRunner::all_pass(1);

        let (code, out, err) = run_with(&["build", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("Gemfile"));
        assert_eq!(runner.calls(), Vec::new());
    }

    #[test]
    fn fastlane_missing_lane_errors_before_running_any_commands() {
        let dir = TestDir::new();
        dir.write(
            "Gemfile",
            "source \"https://rubygems.org\"\ngem \"fastlane\", \"2.228.0\"\n",
        );
        dir.write("fastlane/Fastfile", "lane :build do\nend\n");
        let runner = FakeRunner::all_pass(1);

        let (code, out, err) = run_with(&["build", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("standard lanes"));
        assert!(err.contains("`test`"));
        assert_eq!(runner.calls(), Vec::new());
    }

    #[test]
    fn fastlane_missing_bundle_reports_install_hint() {
        let dir = TestDir::fastlane_project();
        let runner = FakeRunner::new(vec![Err(io::Error::new(
            io::ErrorKind::NotFound,
            "missing bundle",
        ))]);

        let (code, out, err) = run_with(&["build", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("Fastlane project"));
        assert!(err.contains("Failed to invoke bundle: missing bundle"));
        assert!(err.contains("gem install bundler"));
        assert!(err.contains("└ run which bundle"));
    }

    #[test]
    fn gradle_build_runs_wrapper_task() {
        let dir = TestDir::gradle_project();
        let runner = FakeRunner::all_pass(1);

        let (code, out, err) = run_with(&["build", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains(&format!("└ run rapport test {}", dir.as_str())));
        assert_eq!(
            runner.calls(),
            vec![RecordedCommand {
                program: "./gradlew".into(),
                args: vec!["--no-daemon".into(), "assemble".into()],
                cwd: dir.path.clone(),
            }]
        );
    }

    #[test]
    fn gradle_validate_runs_convention_task_directly() {
        let dir = TestDir::gradle_project();
        let runner = FakeRunner::all_pass(1);

        let (code, _out, err) = run_with(&["validate", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![RecordedCommand {
                program: "./gradlew".into(),
                args: vec!["--no-daemon".into(), "build".into()],
                cwd: dir.path.clone(),
            }]
        );
    }

    #[test]
    fn gradle_missing_java_output_is_curated_from_wrapper_failure() {
        let dir = TestDir::gradle_project();
        let runner = FakeRunner::new(vec![Ok(fail(
            "",
            "ERROR: JAVA_HOME is not set and no 'java' command could be found in your PATH.",
        ))]);

        let (code, out, err) = run_with(&["build", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::from(1));
        assert_eq!(out, "");
        assert!(err.contains("Gradle wrapper could not find Java"));
        assert!(err.contains("Install a JDK"));
        assert_eq!(runner.calls().len(), 1);
    }

    #[test]
    fn kustomize_build_prefers_standalone_renderer() {
        let dir = TestDir::kustomize_project();
        let runner = FakeRunner::all_pass(2);

        let (code, out, err) = run_with(&["build", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains(&format!("└ run rapport test {}", dir.as_str())));
        assert_eq!(
            runner.calls(),
            vec![
                RecordedCommand {
                    program: "kustomize".into(),
                    args: vec!["version".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "kustomize".into(),
                    args: vec!["build".into(), ".".into()],
                    cwd: dir.path.clone(),
                },
            ]
        );
    }

    #[test]
    fn kustomize_build_falls_back_to_kubectl_renderer() {
        let dir = TestDir::kustomize_project();
        let runner = FakeRunner::new(vec![
            Err(io::Error::new(io::ErrorKind::NotFound, "missing kustomize")),
            Ok(pass()),
            Ok(pass()),
        ]);

        let (code, _out, err) = run_with(&["build", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![
                RecordedCommand {
                    program: "kustomize".into(),
                    args: vec!["version".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "kubectl".into(),
                    args: vec!["version".into(), "--client".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "kubectl".into(),
                    args: vec!["kustomize".into(), ".".into()],
                    cwd: dir.path.clone(),
                },
            ]
        );
    }

    #[test]
    fn kustomize_lint_renders_then_validates() {
        let dir = TestDir::kustomize_project();
        let runner = FakeRunner::all_pass(3);

        let (code, _out, err) = run_with(&["lint", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![
                RecordedCommand {
                    program: "kustomize".into(),
                    args: vec!["version".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "kubeconform".into(),
                    args: vec!["-v".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "/bin/sh".into(),
                    args: vec![
                        "-c".into(),
                        "set -e; rendered=\"$(kustomize build .)\"; printf '%s\\n' \"$rendered\" | kubeconform -strict -summary -ignore-missing-schemas -".into(),
                    ],
                    cwd: dir.path.clone(),
                },
            ]
        );
    }

    #[test]
    fn kustomize_fix_and_test_are_clear_noops() {
        let dir = TestDir::kustomize_project();
        let runner = FakeRunner::all_pass(0);

        let (fix_code, fix_out, fix_err) = run_with(&["fix", dir.as_str()], &runner);
        let (test_code, test_out, test_err) = run_with(&["test", dir.as_str()], &runner);

        assert_eq!(fix_code, ExitCode::SUCCESS);
        assert_eq!(test_code, ExitCode::SUCCESS);
        assert_eq!(fix_err, "");
        assert_eq!(test_err, "");
        assert!(fix_out.contains("Kustomize has no autofix"));
        assert!(test_out.contains("No Kubernetes tests configured"));
        assert_eq!(runner.calls(), Vec::new());
    }

    #[test]
    fn kustomize_missing_renderer_reports_install_hint() {
        let dir = TestDir::kustomize_project();
        let runner = FakeRunner::new(vec![
            Err(io::Error::new(io::ErrorKind::NotFound, "missing kustomize")),
            Err(io::Error::new(io::ErrorKind::NotFound, "missing kubectl")),
        ]);

        let (code, out, err) = run_with(&["build", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("Kustomize renderer tooling was not found"));
        assert!(err.contains("Install standalone Kustomize"));
        assert!(err.contains("└ run kustomize version"));
        assert!(err.contains("└ run kubectl version --client"));
    }

    #[test]
    fn kustomize_missing_validator_reports_install_hint() {
        let dir = TestDir::kustomize_project();
        let runner = FakeRunner::new(vec![
            Ok(pass()),
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "missing kubeconform",
            )),
        ]);

        let (code, out, err) = run_with(&["lint", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("Kubernetes static validation tooling was not found"));
        assert!(err.contains("kubeconform"));
        assert!(err.contains("└ run kubeconform -v"));
    }

    #[test]
    fn kustomize_umbrella_discovery_runs_child_targets() {
        let dir = TestDir::new();
        dir.write(
            "platform/base/kustomization.yaml",
            "resources:\n  - deployment.yaml\n",
        );
        dir.write(
            "platform/base/deployment.yaml",
            "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: app\n",
        );
        dir.write(
            "platform/overlays/dev/kustomization.yaml",
            "resources:\n  - ../../base\n",
        );
        let platform = dir.path.join("platform");
        let runner = FakeRunner::all_pass(4);

        let (code, _out, err) = run_with(&["build", platform.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![
                RecordedCommand {
                    program: "kustomize".into(),
                    args: vec!["version".into()],
                    cwd: dir.path.join("platform/base"),
                },
                RecordedCommand {
                    program: "kustomize".into(),
                    args: vec!["build".into(), ".".into()],
                    cwd: dir.path.join("platform/base"),
                },
                RecordedCommand {
                    program: "kustomize".into(),
                    args: vec!["version".into()],
                    cwd: dir.path.join("platform/overlays/dev"),
                },
                RecordedCommand {
                    program: "kustomize".into(),
                    args: vec!["build".into(), ".".into()],
                    cwd: dir.path.join("platform/overlays/dev"),
                },
            ]
        );
    }

    #[test]
    fn umbrella_scope_discovers_multiple_child_cargo_targets() {
        let dir = TestDir::new();
        dir.write_cargo_package("services/api", "api");
        dir.write_cargo_package("tools/cli", "cli");
        let runner = FakeRunner::all_pass(2);

        let (code, out, err) = run_with(&["build", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains("## Targets"));
        assert!(out.contains(&format!(
            "pass - Cargo project: {}",
            dir.path.join("services/api")
        )));
        assert!(out.contains(&format!(
            "pass - Cargo project: {}",
            dir.path.join("tools/cli")
        )));
        assert!(out.contains(&format!("└ run rapport test {}", dir.as_str())));
        assert_eq!(
            runner.calls(),
            vec![
                RecordedCommand {
                    program: "cargo".into(),
                    args: vec!["check".into(), "--package".into(), "api".into()],
                    cwd: dir.path.join("services/api"),
                },
                RecordedCommand {
                    program: "cargo".into(),
                    args: vec!["check".into(), "--package".into(), "cli".into()],
                    cwd: dir.path.join("tools/cli"),
                },
            ]
        );
    }

    #[test]
    fn cargo_workspace_scope_does_not_duplicate_member_crates() {
        let dir = TestDir::new();
        dir.write(
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/alpha\", \"crates/beta\"]\nresolver = \"3\"\n",
        );
        dir.write_cargo_package("crates/alpha", "alpha");
        dir.write_cargo_package("crates/beta", "beta");
        let runner = FakeRunner::all_pass(1);

        let (code, out, err) = run_with(&["build", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(!out.contains("## Targets"));
        assert_eq!(
            runner.calls(),
            vec![RecordedCommand {
                program: "cargo".into(),
                args: vec!["check".into(), "--workspace".into()],
                cwd: dir.path.clone(),
            }]
        );
    }

    #[test]
    fn cargo_workspace_member_path_remains_package_scoped() {
        let dir = TestDir::new();
        dir.write(
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/alpha\", \"crates/beta\"]\nresolver = \"3\"\n",
        );
        dir.write_cargo_package("crates/alpha", "alpha");
        dir.write_cargo_package("crates/beta", "beta");
        let member_child = dir.path.join("crates/alpha/src");
        let runner = FakeRunner::all_pass(1);

        let (code, out, err) = run_with(&["build", member_child.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains(&format!(
            "└ run rapport test {}",
            dir.path.join("crates/alpha")
        )));
        assert_eq!(
            runner.calls(),
            vec![RecordedCommand {
                program: "cargo".into(),
                args: vec!["check".into(), "--package".into(), "alpha".into()],
                cwd: dir.path.join("crates/alpha"),
            }]
        );
    }

    #[test]
    fn mixed_scope_discovers_additive_ecosystems() {
        let dir = TestDir::new();
        dir.write_cargo_package("apps/api", "api");
        dir.write("infra/main.tf", "resource \"null_resource\" \"app\" {}\n");
        let runner = FakeRunner::all_pass(2);

        let (code, out, err) = run_with(&["build", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains("## Targets"));
        assert!(out.contains("Cargo project"));
        assert!(out.contains("Terraform project"));
        assert_eq!(
            runner.calls(),
            vec![
                RecordedCommand {
                    program: "cargo".into(),
                    args: vec!["check".into(), "--package".into(), "api".into()],
                    cwd: dir.path.join("apps/api"),
                },
                RecordedCommand {
                    program: "terraform".into(),
                    args: vec!["validate".into()],
                    cwd: dir.path.join("infra"),
                },
            ]
        );
    }

    #[test]
    fn recursive_scope_ignores_generated_and_dependency_directories() {
        let dir = TestDir::new();
        dir.write_cargo_package("apps/api", "api");
        for ignored in [
            ".build",
            ".cache",
            ".gradle",
            ".next",
            ".swiftpm",
            ".terraform",
            ".turbo",
            "DerivedData",
            "Pods",
            "build",
            "coverage",
            "dist",
            "node_modules",
            "target",
            "vendor",
        ] {
            dir.write_cargo_package(&format!("{ignored}/fake"), "fake");
        }
        let runner = FakeRunner::all_pass(1);

        let (code, _out, err) = run_with(&["build", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![RecordedCommand {
                program: "cargo".into(),
                args: vec!["check".into(), "--package".into(), "api".into()],
                cwd: dir.path.join("apps/api"),
            }]
        );
    }

    #[test]
    fn path_inside_project_without_child_targets_keeps_nearest_parent_behavior() {
        let dir = TestDir::cargo_project();
        fs::create_dir_all(dir.path.join("src/deep")).expect("child directory should be created");
        let child = dir.path.join("src/deep");
        let runner = FakeRunner::all_pass(1);

        let (code, _out, err) = run_with(&["build", child.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![RecordedCommand {
                program: "cargo".into(),
                args: vec!["check".into(), "--package".into(), "sample".into()],
                cwd: dir.path.clone(),
            }]
        );
    }

    #[test]
    fn multi_target_failure_identifies_failing_cargo_target() {
        let dir = TestDir::new();
        dir.write_cargo_package("services/api", "api");
        dir.write_cargo_package("services/web", "web");
        let failing = dir.path.join("services/web");
        let runner = FakeRunner::new(vec![Ok(pass()), Ok(fail("cargo stdout", "cargo stderr"))]);

        let (code, out, err) = run_with(&["build", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::from(1));
        assert_eq!(out, "");
        assert!(err.contains(&format!("Cargo project: {failing}")));
        assert!(err.contains("Failing phase: build"));
        assert!(err.contains("cargo stderr"));
        assert!(err.contains("cargo stdout"));
        assert_eq!(runner.calls().len(), 2);
    }

    #[test]
    fn terraform_build_runs_validate() {
        let dir = TestDir::terraform_project();
        let runner = FakeRunner::all_pass(1);

        let (code, out, err) = run_with(&["build", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains(&format!("└ run rapport test {}", dir.as_str())));
        assert_eq!(
            runner.calls(),
            vec![RecordedCommand {
                program: "terraform".into(),
                args: vec!["validate".into()],
                cwd: dir.path.clone(),
            }]
        );
    }

    #[test]
    fn terraform_fix_runs_recursive_fmt() {
        let dir = TestDir::terraform_project();
        let runner = FakeRunner::all_pass(1);

        let (code, _out, err) = run_with(&["fix", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![RecordedCommand {
                program: "terraform".into(),
                args: vec!["fmt".into(), "-recursive".into()],
                cwd: dir.path.clone(),
            }]
        );
    }

    #[test]
    fn terraform_lint_runs_fmt_and_available_tflint() {
        let dir = TestDir::terraform_project();
        let runner = FakeRunner::all_pass(3);

        let (code, _out, err) = run_with(&["lint", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![
                RecordedCommand {
                    program: "tflint".into(),
                    args: vec!["--version".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "terraform".into(),
                    args: vec!["fmt".into(), "-check".into(), "-recursive".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "tflint".into(),
                    args: vec!["--recursive".into()],
                    cwd: dir.path.clone(),
                },
            ]
        );
    }

    #[test]
    fn terraform_lint_allows_missing_optional_tflint() {
        let dir = TestDir::terraform_project();
        let runner = FakeRunner::new(vec![
            Err(io::Error::new(io::ErrorKind::NotFound, "missing tflint")),
            Ok(pass()),
        ]);

        let (code, _out, err) = run_with(&["lint", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![
                RecordedCommand {
                    program: "tflint".into(),
                    args: vec!["--version".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "terraform".into(),
                    args: vec!["fmt".into(), "-check".into(), "-recursive".into()],
                    cwd: dir.path.clone(),
                },
            ]
        );
    }

    #[test]
    fn terraform_required_tflint_missing_reports_install_hint() {
        let dir = TestDir::terraform_project();
        dir.write(
            ".tflint.hcl",
            "plugin \"terraform\" {\n  enabled = true\n}\n",
        );
        let runner = FakeRunner::new(vec![Err(io::Error::new(
            io::ErrorKind::NotFound,
            "missing tflint",
        ))]);

        let (code, out, err) = run_with(&["lint", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("Terraform lint tooling was not found"));
        assert!(err.contains("https://github.com/terraform-linters/tflint"));
        assert!(err.contains("└ run tflint --version"));
        assert_eq!(runner.calls().len(), 1);
    }

    #[test]
    fn terraform_test_is_a_clear_noop() {
        let dir = TestDir::terraform_project();
        let runner = FakeRunner::all_pass(0);

        let (code, out, err) = run_with(&["test", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains("No Terraform tests configured"));
        assert_eq!(runner.calls(), Vec::new());
    }

    #[test]
    fn terraform_audit_runs_validate_without_extra_steps() {
        let dir = TestDir::terraform_project();
        let runner = FakeRunner::new(vec![
            Err(io::Error::new(io::ErrorKind::NotFound, "missing tflint")),
            Ok(pass()),
            Ok(pass()),
        ]);

        let (code, _out, err) = run_with(&["audit", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![
                RecordedCommand {
                    program: "tflint".into(),
                    args: vec!["--version".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "terraform".into(),
                    args: vec!["fmt".into(), "-check".into(), "-recursive".into()],
                    cwd: dir.path.clone(),
                },
                RecordedCommand {
                    program: "terraform".into(),
                    args: vec!["validate".into()],
                    cwd: dir.path.clone(),
                },
            ]
        );
    }

    #[test]
    fn terraform_recursive_discovery_ignores_generated_cache_contents() {
        let dir = TestDir::new();
        dir.write(
            ".terraform/modules/generated/main.tf",
            "resource \"null_resource\" \"generated\" {}\n",
        );
        dir.write("cloud/main.tf", "resource \"null_resource\" \"app\" {}\n");
        let runner = FakeRunner::all_pass(1);

        let (code, _out, err) = run_with(&["build", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![RecordedCommand {
                program: "terraform".into(),
                args: vec!["validate".into()],
                cwd: dir.path.join("cloud"),
            }]
        );
    }

    #[test]
    fn terraform_paths_inside_generated_cache_walk_up_to_real_project() {
        let dir = TestDir::terraform_project();
        dir.write(
            ".terraform/modules/generated/main.tf",
            "resource \"null_resource\" \"generated\" {}\n",
        );
        let generated = dir.path.join(".terraform/modules/generated");
        let runner = FakeRunner::all_pass(1);

        let (code, _out, err) = run_with(&["build", generated.as_str()], &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![RecordedCommand {
                program: "terraform".into(),
                args: vec!["validate".into()],
                cwd: dir.path.clone(),
            }]
        );
    }

    #[test]
    fn missing_path_errors_before_running_any_commands() {
        let missing = Utf8PathBuf::from_path_buf(std::env::temp_dir().join(format!(
            "rapport-missing-{}",
            TEST_DIR_COUNTER.fetch_add(1, Ordering::Relaxed)
        )))
        .expect("temp dir path should be utf8");
        let runner = FakeRunner::all_pass(1);

        let (code, out, err) = run_with(&["build", missing.as_str()], &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains(&format!("You ran: rapport build {missing}")));
        assert!(err.contains("does not exist or is not a directory"));
        assert!(err.contains("└ run rapport help build"));
        assert_eq!(runner.calls(), Vec::new());
    }

    #[test]
    fn path_outside_git_repository_errors_before_running_any_commands() {
        let dir = TestDir::new_without_git();
        dir.write(
            "Cargo.toml",
            "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        );
        let runner = FakeRunner::all_pass(1);

        let (code, out, err) = run_with(&["build", dir.as_str()], &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains(&format!("You ran: rapport build {}", dir.as_str())));
        assert!(err.contains("is not inside a git repository"));
        assert_eq!(runner.calls(), Vec::new());
    }

    #[test]
    fn git_repository_without_supported_project_errors_before_running_any_commands() {
        let dir = TestDir::new();
        fs::create_dir_all(dir.path.join("src/deep")).expect("child directory should be created");
        let child = dir.path.join("src/deep");
        let runner = FakeRunner::all_pass(1);

        let (code, out, err) = run_with(&["build", child.as_str()], &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains(&format!("You ran: rapport build {child}")));
        assert!(err.contains("No supported project marker was found"));
        assert!(err.contains("git root"));
        assert_eq!(runner.calls(), Vec::new());
    }
}
