use crate::cli::WorkStartArgs;
use crate::context::{Clock, CommandContext};
use crate::state::{WorkFact, WorkState, WorkStateError, WorkStateStore};
use crate::telemetry::{CommandEvent, CommandEventOutcome, TelemetryError, TelemetryWriter};
use crate::{RunHint, ViewBuilder};
use nonempty::nonempty;
use rapport_files::FileSystem;
use std::io::Write;
use std::process::ExitCode;

const SUCCESS: u8 = 0;
const FAILURE: u8 = 2;

pub fn status<F, C, O, E>(
    arguments: Vec<String>,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> ExitCode
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let store = WorkStateStore::new(context.paths.clone());
    let result = match store.load(context.fs) {
        Ok(Some(state)) => {
            let _ = writeln!(context.out, "{}", render_active_work(&state));
            CommandResult::success()
        }
        Ok(None) => {
            let _ = writeln!(
                context.out,
                "{}",
                render_no_work(context.paths.work_state_file().as_str())
            );
            CommandResult::success()
        }
        Err(error) => {
            let _ = writeln!(context.err, "{}", render_invalid_work_state(&error));
            CommandResult::failure()
        }
    };
    finish("work status", arguments, context, result)
}

pub fn start<F, C, O, E>(
    start_args: &WorkStartArgs,
    arguments: Vec<String>,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> ExitCode
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let store = WorkStateStore::new(context.paths.clone());
    match store.load(context.fs) {
        Ok(Some(existing)) => {
            let _ = writeln!(context.err, "{}", render_existing_work(&existing));
            finish("work start", arguments, context, CommandResult::failure())
        }
        Ok(None) => {
            let now = context.clock.now_rfc3339();
            let state = WorkState::new(start_args.title.clone(), now)
                .with_objective(start_args.objective.clone())
                .with_ticket(start_args.ticket.clone())
                .with_plan(start_args.plan.clone())
                .with_paths(start_args.paths.iter().map(ToString::to_string));
            match store.save(context.fs, &state) {
                Ok(()) => {
                    let _ = writeln!(context.out, "{}", render_active_work(&state));
                    finish("work start", arguments, context, CommandResult::success())
                }
                Err(error) => {
                    let _ = writeln!(context.err, "{}", render_state_error(&error));
                    finish("work start", arguments, context, CommandResult::failure())
                }
            }
        }
        Err(error) => {
            let _ = writeln!(context.err, "{}", render_state_error(&error));
            finish("work start", arguments, context, CommandResult::failure())
        }
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

fn render_no_work(state_file: &str) -> String {
    ViewBuilder::new()
        .title("rapport work status")
        .paragraph(format!("No active work state found at `{state_file}`."))
        .paragraph("Start work to create local context for the current task.")
        .next_actions(nonempty![RunHint::new(
            "rapport work start --title \"...\" --path <path>"
        )])
        .build()
}

pub fn render_active_work(state: &WorkState) -> String {
    let mut details = vec![("title", state.title.clone())];
    if let Some(ticket) = &state.ticket {
        details.push(("ticket", ticket.clone()));
    }
    if let Some(plan) = &state.plan {
        details.push(("plan", plan.clone()));
    }
    if let Some(objective) = &state.objective {
        details.push(("objective", objective.clone()));
    }
    details.extend([
        ("stage", state.stage.to_string()),
        ("status", state.status.to_string()),
        ("created", state.created_at.clone()),
        ("updated", state.updated_at.clone()),
    ]);

    let paths = if state.paths.is_empty() {
        vec![String::from("No paths added yet.")]
    } else {
        state.paths.clone()
    };

    let mut builder = ViewBuilder::new()
        .title("rapport work status")
        .section("Work", |b| b.entries(details))
        .section("Paths", |b| b.items(paths));

    let facts = recent_facts(state);
    if !facts.is_empty() {
        builder = builder.section("Recent", |b| b.entries(facts));
    }

    builder
        .next_actions(nonempty![RunHint::new("rapport build")])
        .build()
}

fn recent_facts(state: &WorkState) -> Vec<(&'static str, String)> {
    [
        ("build", state.build.as_ref()),
        ("integrate", state.integrate.as_ref()),
        ("signoff", state.signoff.as_ref()),
    ]
    .into_iter()
    .filter_map(|(name, fact)| fact.map(|fact| (name, render_fact(fact))))
    .collect()
}

fn render_fact(fact: &WorkFact) -> String {
    let mut rendered = fact.status.clone();
    if let Some(at) = &fact.at {
        rendered.push_str(" at ");
        rendered.push_str(at);
    }
    if let Some(summary) = &fact.summary {
        rendered.push_str(": ");
        rendered.push_str(summary);
    }
    rendered
}

fn render_invalid_work_state(error: &WorkStateError) -> String {
    ViewBuilder::new()
        .title("rapport work status")
        .paragraph("Could not read active work state.")
        .paragraph(error)
        .next_actions(nonempty![RunHint::new(
            "fix .rapport/work.toml or remove it before starting new work"
        )])
        .build()
}

fn render_existing_work(state: &WorkState) -> String {
    ViewBuilder::new()
        .title("rapport work start")
        .paragraph(format!("Active work already exists: `{}`.", state.title))
        .paragraph("Rapport will not overwrite `.rapport/work.toml`.")
        .next_actions(nonempty![RunHint::new("rapport work status")])
        .build()
}

fn render_state_error(error: &WorkStateError) -> String {
    ViewBuilder::new()
        .title("rapport work start")
        .paragraph("Could not write active work state.")
        .paragraph(error)
        .next_actions(nonempty![RunHint::new("rapport work status")])
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
    use crate::state::{WorkStage, WorkStatus};

    #[test]
    fn active_work_view_includes_recent_facts_when_present() {
        let mut state = WorkState::new("Do the thing", "2026-07-07T23:00:00Z");
        state.paths = vec![String::from("app/api")];
        state.stage = WorkStage::Development;
        state.status = WorkStatus::Active;
        state.build = Some(WorkFact {
            status: String::from("pass"),
            at: Some(String::from("2026-07-07T23:05:00Z")),
            summary: Some(String::from("just ci")),
        });

        let view = render_active_work(&state);

        assert!(view.contains("Do the thing"));
        assert!(view.contains("app/api"));
        assert!(view.contains("just ci"));
    }
}
