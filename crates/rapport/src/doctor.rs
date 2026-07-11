use crate::context::{Clock, CommandContext};
use crate::runner::{CommandOutcome, CommandSpec};
use crate::telemetry::{CommandEvent, CommandEventOutcome, TelemetryError, TelemetryWriter};
use crate::{RunHint, ViewBuilder};
use crate::{project_context, rules};
use nonempty::nonempty;
use rapport_files::FileSystem;
use std::collections::BTreeSet;
use std::io::Write;
use std::process::ExitCode;

const SUCCESS: u8 = 0;
const FAILURE: u8 = 2;
const PROJECT_CONTEXT_CHECK: &str = "Project Context";

pub fn run<F, C, O, E>(
    arguments: Vec<String>,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> ExitCode
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let report = diagnose(context);
    let result = if report.passed() {
        let _ = writeln!(context.out, "{}", render_report(&report));
        CommandResult::success()
    } else {
        let _ = writeln!(context.err, "{}", render_report(&report));
        CommandResult::failure()
    };
    finish("doctor", arguments, context, result)
}

fn diagnose<F, C, O, E>(context: &CommandContext<'_, F, C, O, E>) -> DoctorReport
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let mut checks = Vec::new();
    let git_marker = context.paths.repo_root().join(".git");
    if !context.fs.exists(&git_marker) {
        checks.push(DoctorCheck::fail(
            "git repository",
            format!("no `.git` marker found at {git_marker}"),
        ));
        return DoctorReport { checks };
    }
    checks.push(DoctorCheck::pass(
        "git repository",
        format!("found {git_marker}"),
    ));

    match context.runner.run(
        &CommandSpec::new("git", ["remote", "get-url", "origin"]),
        context.paths.repo_root(),
    ) {
        Ok(outcome) if outcome.success => {
            let origin = outcome.stdout.trim();
            if origin.is_empty() {
                checks.push(DoctorCheck::fail(
                    "origin remote",
                    "`git remote get-url origin` returned no URL",
                ));
            } else {
                checks.push(DoctorCheck::pass("origin remote", origin.to_string()));
                if origin_url_is_github(origin) {
                    checks.push(DoctorCheck::pass(
                        "GitHub origin",
                        "origin host is github.com",
                    ));
                } else {
                    checks.push(DoctorCheck::fail(
                        "GitHub origin",
                        format!("origin does not point at GitHub: {origin}"),
                    ));
                }
            }
        }
        Ok(outcome) => checks.push(DoctorCheck::fail(
            "origin remote",
            failed_origin_detail(&outcome),
        )),
        Err(error) => checks.push(DoctorCheck::fail(
            "origin remote",
            format!("could not run `git remote get-url origin`: {error}"),
        )),
    }

    checks.extend(project_context_checks(context));

    DoctorReport { checks }
}

fn project_context_checks<F, C, O, E>(context: &CommandContext<'_, F, C, O, E>) -> Vec<DoctorCheck>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let context_validation =
        project_context::validate_repository(context.fs, context.paths.repo_root());
    let rule_validation = rules::validate_repository(context.fs, &context.paths);
    let problems = context_validation
        .problem_details()
        .chain(rule_validation.problem_details())
        .map(ToString::to_string)
        .collect::<BTreeSet<_>>();

    if problems.is_empty() {
        return vec![DoctorCheck::pass(
            PROJECT_CONTEXT_CHECK,
            format!(
                "validated {}, {}, {}, {}, and {}",
                file_count(context_validation.context_file_count(), "context.toml file"),
                file_count(
                    context_validation.embedded_ruleset_count(),
                    "embedded ruleset"
                ),
                file_count(context_validation.signoff_count(), "signoff declaration"),
                file_count(rule_validation.rule_file_count(), "standalone ruleset"),
                file_count(
                    context_validation.local_rule_count() + rule_validation.local_rule_count(),
                    "locally declared rule"
                )
            ),
        )];
    }

    problems
        .into_iter()
        .map(|problem| DoctorCheck::fail(PROJECT_CONTEXT_CHECK, problem))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DoctorReport {
    checks: Vec<DoctorCheck>,
}

impl DoctorReport {
    fn passed(&self) -> bool {
        self.checks
            .iter()
            .all(|check| check.status == CheckStatus::Pass)
    }

    fn has_failed_check(&self, name: &str) -> bool {
        self.checks
            .iter()
            .any(|check| check.name == name && check.status == CheckStatus::Fail)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DoctorCheck {
    name: &'static str,
    status: CheckStatus,
    detail: String,
}

impl DoctorCheck {
    fn pass(name: &'static str, detail: impl Into<String>) -> Self {
        Self {
            name,
            status: CheckStatus::Pass,
            detail: detail.into(),
        }
    }

    fn fail(name: &'static str, detail: impl Into<String>) -> Self {
        Self {
            name,
            status: CheckStatus::Fail,
            detail: detail.into(),
        }
    }

    fn line(&self) -> String {
        format!("`{}` -- {}: {}", self.name, self.status, self.detail)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckStatus {
    Pass,
    Fail,
}

impl std::fmt::Display for CheckStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pass => f.write_str("pass"),
            Self::Fail => f.write_str("fail"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct CommandResult {
    outcome: CommandEventOutcome,
    exit_code: u8,
}

impl CommandResult {
    fn success() -> Self {
        Self {
            outcome: CommandEventOutcome::Success,
            exit_code: SUCCESS,
        }
    }

    fn failure() -> Self {
        Self {
            outcome: CommandEventOutcome::Failure,
            exit_code: FAILURE,
        }
    }
}

fn failed_origin_detail(outcome: &CommandOutcome) -> String {
    let output = [outcome.stderr.trim(), outcome.stdout.trim()]
        .into_iter()
        .find(|output| !output.is_empty())
        .unwrap_or("origin remote is not configured");
    format!("`git remote get-url origin` failed: {output}")
}

fn origin_url_is_github(origin: &str) -> bool {
    let origin = origin.trim();
    origin.starts_with("https://github.com/")
        || origin.starts_with("http://github.com/")
        || origin.starts_with("git@github.com:")
        || origin.starts_with("ssh://git@github.com/")
}

fn file_count(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("{count} {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

fn finish<F, C, O, E>(
    command: &'static str,
    arguments: Vec<String>,
    context: &mut CommandContext<'_, F, C, O, E>,
    result: CommandResult,
) -> ExitCode
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let event = CommandEvent::new(
        context.clock.now_rfc3339(),
        arguments,
        command,
        result.outcome,
        result.exit_code,
    );
    match TelemetryWriter::new(context.paths.clone()).append(context.fs, &event) {
        Ok(()) => ExitCode::from(result.exit_code),
        Err(error) => {
            let _ = writeln!(context.err, "{}", render_telemetry_error(&error));
            ExitCode::FAILURE
        }
    }
}

fn render_report(report: &DoctorReport) -> String {
    let next = if report.passed() {
        RunHint::new("rapport integrate")
    } else if report.has_failed_check("origin remote") || report.has_failed_check("GitHub origin") {
        RunHint::new("configure GitHub origin, then run rapport doctor")
    } else {
        RunHint::new("fix failed checks, then run rapport doctor")
    };
    ViewBuilder::new()
        .title("rapport doctor")
        .section("Checks", |b| {
            b.items(report.checks.iter().map(DoctorCheck::line))
        })
        .next_actions(nonempty![next])
        .build()
}

fn render_telemetry_error(error: &TelemetryError) -> String {
    ViewBuilder::new()
        .title("rapport telemetry")
        .paragraph("Command completed, but telemetry could not be written.")
        .paragraph(error)
        .next_actions(nonempty![RunHint::new("rapport work status")])
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_origin_url_supports_common_forms() {
        assert!(origin_url_is_github(
            "https://github.com/hedge-ops/rapport.git"
        ));
        assert!(origin_url_is_github("git@github.com:hedge-ops/rapport.git"));
        assert!(origin_url_is_github(
            "ssh://git@github.com/hedge-ops/rapport.git"
        ));
    }

    #[test]
    fn github_origin_url_rejects_other_hosts() {
        assert!(!origin_url_is_github(
            "https://gitlab.com/hedge-ops/rapport.git"
        ));
        assert!(!origin_url_is_github(
            "git@example.com:hedge-ops/rapport.git"
        ));
    }

    #[test]
    fn report_passes_only_when_all_checks_pass() {
        let report = DoctorReport {
            checks: vec![
                DoctorCheck::pass("git repository", "found .git"),
                DoctorCheck::fail("origin remote", "missing"),
            ],
        };

        assert!(!report.passed());
    }
}
