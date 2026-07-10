use crate::context::{Clock, CommandContext};
use crate::signoff_contract;
use crate::telemetry::{CommandEvent, CommandEventOutcome, TelemetryError, TelemetryWriter};
use crate::{RunHint, ViewBuilder};
use nonempty::nonempty;
use rapport_files::FileSystem;
use std::io;
use std::io::Write;
use std::process::ExitCode;

const SUCCESS: u8 = 0;
const FAILURE: u8 = 2;
const AGENTS_FILE: &str = "AGENTS.md";
const START_MARKER: &str = "<!-- rapport:init:start -->";
const END_MARKER: &str = "<!-- rapport:init:end -->";

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
    let path = context.paths.repo_root().join(AGENTS_FILE);
    let result = match load_agents(context.fs, &path) {
        Ok(existing) => {
            let contents = upsert_rapport_section(existing.as_deref());
            match context.fs.write_string(&path, contents) {
                Ok(()) => {
                    match signoff_contract::write_shared(context.fs, context.paths.repo_root()) {
                        Ok(()) => {
                            let status = if existing.is_some() {
                                "updated"
                            } else {
                                "created"
                            };
                            let _ = writeln!(context.out, "{}", render_initialized(status));
                            CommandResult::success()
                        }
                        Err(error) => {
                            let _ = writeln!(context.err, "{}", render_init_error(&error));
                            CommandResult::failure()
                        }
                    }
                }
                Err(error) => {
                    let _ = writeln!(context.err, "{}", render_init_error(&error));
                    CommandResult::failure()
                }
            }
        }
        Err(error) => {
            let _ = writeln!(context.err, "{}", render_init_error(&error));
            CommandResult::failure()
        }
    };
    finish("init", arguments, context, result)
}

fn load_agents(
    fs: &impl FileSystem,
    path: &rapport_files::Utf8Path,
) -> Result<Option<String>, io::Error> {
    if fs.is_file(path) {
        fs.read_to_string(path).map(Some)
    } else {
        Ok(None)
    }
}

fn upsert_rapport_section(existing: Option<&str>) -> String {
    let section = rapport_section();
    match existing {
        Some(contents) if contents.contains(START_MARKER) && contents.contains(END_MARKER) => {
            replace_section(contents, &section)
        }
        Some(contents) if contents.trim().is_empty() => section,
        Some(contents) => append_section(contents, &section),
        None => section,
    }
}

fn replace_section(contents: &str, section: &str) -> String {
    let Some(start) = contents.find(START_MARKER) else {
        return append_section(contents, section);
    };
    let Some(end) = contents
        .find(END_MARKER)
        .map(|index| index + END_MARKER.len())
    else {
        return append_section(contents, section);
    };

    let mut updated = String::new();
    updated.push_str(contents[..start].trim_end());
    if !updated.is_empty() {
        updated.push_str("\n\n");
    }
    updated.push_str(section.trim_end());
    let rest = contents[end..].trim_start();
    if !rest.is_empty() {
        updated.push_str("\n\n");
        updated.push_str(rest);
    }
    updated.push('\n');
    updated
}

fn append_section(contents: &str, section: &str) -> String {
    let mut updated = contents.trim_end().to_string();
    if !updated.is_empty() {
        updated.push_str("\n\n");
    }
    updated.push_str(section);
    updated
}

fn rapport_section() -> String {
    format!(
        "{START_MARKER}\n## Software Factory\n\nThis project uses Rapport for planning, coding, testing, building, and reviewing code. Call `rapport prime` for all the details before doing any of these activities.\n{END_MARKER}\n"
    )
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

fn render_initialized(status: &str) -> String {
    ViewBuilder::new()
        .title("rapport init")
        .section("Agent Instructions", |b| {
            b.entries([
                ("status", status.to_string()),
                ("path", AGENTS_FILE.to_string()),
            ])
        })
        .section("Signoff Workflow", |b| {
            b.entries([("path", signoff_contract::SHARED_WORKFLOW.to_string())])
        })
        .next_actions(nonempty![RunHint::new("rapport work status")])
        .build()
}

fn render_init_error(error: &io::Error) -> String {
    ViewBuilder::new()
        .title("rapport init")
        .paragraph("Could not update repository agent instructions.")
        .paragraph(error)
        .next_actions(nonempty![RunHint::new("check repository file permissions")])
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
    fn upsert_rapport_section_appends_to_existing_content() {
        let updated = upsert_rapport_section(Some("# Instructions\n\nKeep it tidy.\n"));

        assert!(updated.contains("# Instructions"));
        assert!(updated.contains("## Software Factory"));
        assert!(updated.contains("rapport prime"));
        assert_eq!(updated.matches(START_MARKER).count(), 1);
    }

    #[test]
    fn upsert_rapport_section_replaces_existing_section() {
        let updated = upsert_rapport_section(Some(
            "# Instructions\n\n<!-- rapport:init:start -->\nold\n<!-- rapport:init:end -->\n",
        ));

        assert!(updated.contains("# Instructions"));
        assert!(updated.contains("planning, coding, testing, building, and reviewing code"));
        assert!(updated.contains("rapport prime"));
        assert!(!updated.contains("\nold\n"));
        assert_eq!(updated.matches(START_MARKER).count(), 1);
    }
}
