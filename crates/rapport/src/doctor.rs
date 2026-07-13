//! Repository prerequisite diagnostics.
//!
//! This module owns read-only GitHub, Context, and active-Work checks and their
//! combined user-facing report.

use crate::context::{Clock, CommandContext};
use crate::runner::{CommandOutcome, CommandSpec};
use crate::{RunHint, ViewBuilder};
use nonempty::nonempty;
use rapport_files::FileSystem;
use std::io::Write;
use std::process::ExitCode;

const PROJECT_CONTEXT_CHECK: &str = "Project Context";

pub fn run<F, C, O, E>(context: &mut CommandContext<'_, F, C, O, E>) -> ExitCode
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let report = diagnose(context);
    if report.passed() {
        let _ = writeln!(context.out, "{}", render_report(&report));
        ExitCode::SUCCESS
    } else {
        let _ = writeln!(context.err, "{}", render_report(&report));
        ExitCode::from(2)
    }
}

fn diagnose<F, C, O, E>(context: &mut CommandContext<'_, F, C, O, E>) -> DoctorReport
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

    let mut github_origin = false;
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
                    github_origin = true;
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

    checks.push(
        match crate::policy_context::doctor_all(
            context.fs,
            context.paths.repo_root(),
            context.runner,
        ) {
            Ok(()) => DoctorCheck::pass(PROJECT_CONTEXT_CHECK, "validated Context policy"),
            Err(error) => DoctorCheck::fail(PROJECT_CONTEXT_CHECK, error.to_string()),
        },
    );

    if github_origin {
        checks.push(match crate::github::diagnose(context) {
            Ok(detail) => DoctorCheck::pass("GitHub integration", detail),
            Err(error) => DoctorCheck::fail("GitHub integration", error),
        });
    }

    DoctorReport { checks }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, derive_more::Display)]
enum CheckStatus {
    #[display("pass")]
    Pass,
    #[display("fail")]
    Fail,
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

fn render_report(report: &DoctorReport) -> String {
    let next = if report.passed() {
        RunHint::new("rapport integrate start")
    } else if report.has_failed_check("origin remote") || report.has_failed_check("GitHub origin") {
        RunHint::new("configure GitHub origin, then run rapport doctor")
    } else if report.has_failed_check("GitHub integration") {
        RunHint::new("run rapport github setup, then rapport doctor")
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
