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
                "Use Rapport before planning, coding, testing, building, reviewing, or integrating code.",
                "Rapport records active work, resolves repository rules, runs validation, and carries local work into GitHub.",
            ])
        })
        .section("Loop", |b| {
            b.items([
                "`rapport work start --ticket <ticket> --title \"...\" --target <branch>` - create active Work from a durable request",
                "`rapport work status` - inspect the current candidate, Tasks, proof, blockers, and next command",
                "`rapport context show <path>` - read folder purpose, ownership, boundaries, and applicable benchmarks",
                "`rapport doctor` - verify Git and GitHub prerequisites before integration",
                "`rapport work task next` - inspect the next ordered Develop Task action without executing it",
                "`rapport develop task start <ID>` - start the pending Develop Task before performing its engineering correction",
                "`rapport develop task complete <ID> --result \"<correction and evidence>\"` - complete the Task with a meaningful result; checkpoint first only when repository state changed",
                "`rapport work checkpoint start` - reconcile and stage a coherent Git checkpoint",
                "`rapport build` - prove the exact clean checkpoint with applicable Context signoffs",
                "`rapport review start` - request one independent Review; use `rapport review complete --result <file>` to record it",
                "`rapport integrate start` - publish accepted Work and create its evidence-carrying pull request",
                "`rapport integrate status` - inspect GitHub state without changing it",
                "`rapport integrate complete` - revalidate, squash-merge, delete the remote branch, and archive Work",
                "`rapport work history list` - find finalized Work; use `rapport work history show <id>` for its complete record",
            ])
        })
        .section("Boundaries", |b| {
            b.items([
                "Keep `.rapport/work.toml` local; it is working memory, not project source.",
                "Work History remains local in Rapport's platform state directory and is never uploaded implicitly.",
                "Prefer repository tools and rules discovered by Rapport over ad hoc workflow guesses.",
                "When changing Rapport itself, run an installed or copied Rapport binary for dogfooding builds.",
            ])
        })
        .next_actions(nonempty![RunHint::new("rapport work status")])
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prime_view_includes_the_core_workflow() {
        let view = render_prime();

        assert!(view.contains("planning, coding, testing, building, reviewing"));
        assert!(view.contains("rapport work start"));
        assert!(view.contains("rapport context show"));
        assert!(view.contains("rapport doctor"));
        assert!(view.contains("rapport work task next"));
        assert!(
            view.contains("rapport develop task start <ID>"),
            "expecting prime to show how to start a Develop Task"
        );
        assert!(
            view.contains(
                "rapport develop task complete <ID> --result \"<correction and evidence>\""
            ),
            "expecting prime to require a correction-and-evidence result"
        );
        assert!(
            view.contains("checkpoint first only when repository state changed"),
            "expecting prime to make checkpointing conditional on repository changes"
        );
        assert!(view.contains("rapport work checkpoint start"));
        assert!(view.contains("rapport build"));
        assert!(view.contains("rapport review"));
        assert!(view.contains("rapport integrate"));
        assert!(view.contains("rapport integrate complete"));
        assert!(view.contains("rapport work history list"));
    }
}
