//! Repository initialization command.
//!
//! This module owns idempotent agent instructions, local rules ignore policy,
//! and repository architecture guidance.

use crate::context::CommandContext;
use crate::{RunHint, ViewBuilder};
use nonempty::nonempty;
use rapport_files::FileSystem;
use std::io;
use std::io::Write;
use std::process::ExitCode;

const AGENTS_SECTION: &str = include_str!("../rapport agents section.md");
const RULES_IGNORE_SECTION: &str = include_str!("../rapport.gitignore");

const FAILURE: u8 = 2;
const AGENTS_FILE: &str = "AGENTS.md";
const START_MARKER: &str = "<!-- rapport:init:start -->";
const END_MARKER: &str = "<!-- rapport:init:end -->";
const GITIGNORE_FILE: &str = ".gitignore";
const RULES_START_MARKER: &str = "# rapport:init-rules:start";
const RULES_END_MARKER: &str = "# rapport:init-rules:end";

pub fn run<F, O, E>(context: &mut CommandContext<'_, F, O, E>) -> ExitCode
where
    F: FileSystem,
    O: Write,
    E: Write,
{
    let path = context.paths.repo_root().join(AGENTS_FILE);
    match load_agents(context.fs, &path) {
        Ok(existing) => {
            let contents = upsert_rapport_section(existing.as_deref());
            match context.fs.write_string(&path, contents) {
                Ok(()) => {
                    if let Err(error) = write_rules_gitignore(context.fs, context.paths.repo_root())
                    {
                        let _ = writeln!(context.err, "{}", render_init_error(&error));
                        return ExitCode::from(FAILURE);
                    }
                    let status = if existing.is_some() {
                        "updated"
                    } else {
                        "created"
                    };
                    let _ = writeln!(context.out, "{}", render_initialized(status));
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    let _ = writeln!(context.err, "{}", render_init_error(&error));
                    ExitCode::from(FAILURE)
                }
            }
        }
        Err(error) => {
            let _ = writeln!(context.err, "{}", render_init_error(&error));
            ExitCode::from(FAILURE)
        }
    }
}

fn write_rules_gitignore(
    fs: &mut impl FileSystem,
    repo_root: &rapport_files::Utf8Path,
) -> io::Result<()> {
    let path = repo_root.join(GITIGNORE_FILE);
    let existing = if fs.is_file(&path) {
        Some(fs.read_to_string(&path)?)
    } else {
        None
    };
    fs.write_string(&path, upsert_rules_ignore(existing.as_deref()))
}

fn upsert_rules_ignore(existing: Option<&str>) -> String {
    let section = RULES_IGNORE_SECTION;
    match existing {
        Some(contents)
            if contents.contains(RULES_START_MARKER) && contents.contains(RULES_END_MARKER) =>
        {
            replace_marked(contents, section, RULES_START_MARKER, RULES_END_MARKER)
        }
        Some(contents) if !contents.trim().is_empty() => {
            format!("{}\n\n{section}", contents.trim_end())
        }
        _ => section.to_owned(),
    }
}

fn replace_marked(contents: &str, section: &str, start_marker: &str, end_marker: &str) -> String {
    let Some(start) = contents.find(start_marker) else {
        return contents.to_string();
    };
    let Some(end) = contents
        .find(end_marker)
        .map(|index| index + end_marker.len())
    else {
        return contents.to_string();
    };
    let before = contents[..start].trim_end();
    let after = contents[end..].trim_start();
    match (before.is_empty(), after.is_empty()) {
        (true, true) => section.to_string(),
        (false, true) => format!("{before}\n\n{section}"),
        (true, false) => format!("{}\n\n{after}", section.trim_end()),
        (false, false) => format!("{before}\n\n{}\n\n{after}", section.trim_end()),
    }
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
    let section = AGENTS_SECTION;
    match existing {
        Some(contents) if contents.contains(START_MARKER) && contents.contains(END_MARKER) => {
            replace_section(contents, section)
        }
        Some(contents) if contents.trim().is_empty() => section.to_owned(),
        Some(contents) => append_section(contents, section),
        None => section.to_owned(),
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

fn render_initialized(status: &str) -> String {
    ViewBuilder::new()
        .title("rapport init")
        .section("Agent Instructions", |b| {
            b.entries([
                ("status", status.to_string()),
                ("path", AGENTS_FILE.to_string()),
            ])
        })
        .next_actions(nonempty![RunHint::new("rapport context show .")])
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_rapport_section_appends_to_existing_content() {
        let updated = upsert_rapport_section(Some("# Instructions\n\nKeep it tidy.\n"));

        assert_eq!(
            updated,
            format!(
                "# Instructions

Keep it tidy.

{AGENTS_SECTION}"
            )
        );
    }

    #[test]
    fn upsert_rapport_section_replaces_existing_section() {
        let updated = upsert_rapport_section(Some(
            "# Instructions\n\n<!-- rapport:init:start -->\nold\n<!-- rapport:init:end -->\n",
        ));

        assert_eq!(
            updated,
            format!(
                "# Instructions

{AGENTS_SECTION}"
            )
        );
    }

    #[test]
    fn rules_gitignore_is_idempotent_and_preserves_unrelated_content() {
        let once = upsert_rules_ignore(Some("target/\n"));
        let twice = upsert_rules_ignore(Some(&once));
        assert_eq!(once, twice);
        assert_eq!(
            once,
            format!(
                "target/

{RULES_IGNORE_SECTION}"
            )
        );
    }
}
