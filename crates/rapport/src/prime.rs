//! Agent workflow orientation command.
//!
//! This module owns the concise operational instructions printed before agents
//! plan, change, validate, or integrate repository work.

use crate::context::{Clock, CommandContext};
use crate::{RunHint, ViewBuilder};
use nonempty::nonempty;
use rapport_files::FileSystem;
use std::io::Write;
use std::process::ExitCode;

pub fn run<F, C, O, E>(context: &mut CommandContext<'_, F, C, O, E>) -> ExitCode
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let _ = writeln!(context.out, "{}", render_prime());
    ExitCode::SUCCESS
}

fn render_prime() -> String {
    ViewBuilder::new()
        .title("rapport prime")
        .section("Purpose", |b| {
            b.items([
                "Rapport structures repository architecture and review benchmarks in context.toml.",
                "Generate sourced review prompts for humans and agents without Work, build, integration, or GitHub state.",
            ])
        })
        .section("Review", |b| {
            b.items([
                "`rapport context init <path> --purpose <text>` - create architecture context for a repository area",
                "`rapport context show <path>` - inspect purpose, ownership, boundaries, and inherited standards",
                "`rapport ruleset catalog list` - discover reusable standards packs",
                "`rapport ruleset catalog install <ID>` - install a standards pack before including it",
                "`rapport review <path> [<path> ...]` - print a complete Markdown review prompt with source paths",
                "Pass the prompt and relevant code or diff to your reviewer; Rapport does not invoke an agent.",
            ])
        })
        .section("Repository ownership", |b| {
            b.items([
                "Edit context.toml directly or use context commands; commit architecture and standards with the repository.",
                "Ancestor context and included packs apply alongside local declarations. Conflicting or missing standards fail explicitly.",
                "Planning, development, tests, builds, and integration belong to the repository's own tools and process.",
                "Legacy lifecycle commands remain callable for compatibility but are deprecated; see docs/lifecycle-migration.md.",
            ])
        })
        .next_actions(nonempty![RunHint::new("rapport context show .")])
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prime_should_orient_users_to_stateless_review() {
        let view = render_prime();
        assert!(view.contains("rapport review <path>"));
        assert!(view.contains("rapport context init"));
        assert!(view.contains("without Work"));
        assert!(!view.contains("rapport work start"));
    }
}
