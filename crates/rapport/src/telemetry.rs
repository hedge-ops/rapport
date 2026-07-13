//! Local command telemetry records.
//!
//! This module owns the durable event schema and append-only repository-local
//! writer; individual commands decide which outcome to record.

use crate::paths::RapportPaths;
use rapport_files::FileSystem;
use serde::de::IgnoredAny;
use serde::{Deserialize, Deserializer, Serialize};
use std::io;

pub const EVENT_SCHEMA_VERSION: u16 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandEventOutcome {
    Success,
    Failure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommandEvent {
    pub schema_version: u16,
    pub timestamp: String,
    #[serde(default)]
    pub argument_count: usize,
    pub command: String,
    pub outcome: CommandEventOutcome,
    pub exit_code: u8,
}

impl<'de> Deserialize<'de> for CommandEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct WireEvent {
            schema_version: u16,
            timestamp: String,
            #[serde(default)]
            argument_count: Option<usize>,
            #[serde(default)]
            argv: Vec<IgnoredAny>,
            command: String,
            outcome: CommandEventOutcome,
            exit_code: u8,
        }

        let wire = WireEvent::deserialize(deserializer)?;
        Ok(Self {
            schema_version: wire.schema_version,
            timestamp: wire.timestamp,
            argument_count: wire.argument_count.unwrap_or(wire.argv.len()),
            command: wire.command,
            outcome: wire.outcome,
            exit_code: wire.exit_code,
        })
    }
}

impl CommandEvent {
    #[must_use]
    pub fn new(
        timestamp: impl Into<String>,
        argv: Vec<String>,
        command: impl Into<String>,
        outcome: CommandEventOutcome,
        exit_code: u8,
    ) -> Self {
        let argument_count = argv.len();
        drop(argv);
        Self {
            schema_version: EVENT_SCHEMA_VERSION,
            timestamp: timestamp.into(),
            argument_count,
            command: command.into(),
            outcome,
            exit_code,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TelemetryWriter {
    paths: RapportPaths,
}

impl TelemetryWriter {
    #[must_use]
    pub fn new(paths: RapportPaths) -> Self {
        Self { paths }
    }

    /// Append one command telemetry event to `.rapport/events.jsonl`.
    ///
    /// # Errors
    ///
    /// Returns [`TelemetryError`] when the `.rapport` directory cannot be
    /// created, the event cannot be encoded, or the JSONL file cannot be
    /// appended.
    pub fn append(
        &self,
        fs: &mut impl FileSystem,
        event: &CommandEvent,
    ) -> Result<(), TelemetryError> {
        fs.create_dir_all(self.paths.rapport_dir())?;
        self.sanitize_legacy_events(fs)?;
        let line = serde_json::to_string(event)?;
        fs.append_line(self.paths.events_file(), line)?;
        Ok(())
    }

    fn sanitize_legacy_events(&self, fs: &mut impl FileSystem) -> Result<(), TelemetryError> {
        let path = self.paths.events_file();
        if !fs.is_file(&path) {
            return Ok(());
        }
        let contents = fs.read_to_string(&path)?;
        if !contents.contains("\"argv\"") {
            return Ok(());
        }
        let mut sanitized = Vec::new();
        for line in contents.lines().filter(|line| !line.trim().is_empty()) {
            let Ok(mut event) = serde_json::from_str::<CommandEvent>(line) else {
                continue;
            };
            event.schema_version = EVENT_SCHEMA_VERSION;
            sanitized.push(serde_json::to_string(&event)?);
        }
        let rewritten = if sanitized.is_empty() {
            String::new()
        } else {
            format!("{}\n", sanitized.join("\n"))
        };
        fs.write_string(path, rewritten)?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TelemetryError {
    #[error("telemetry filesystem error: {0}")]
    Io(#[from] io::Error),
    #[error("telemetry encode error: {0}")]
    Encode(#[from] serde_json::Error),
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "telemetry tests unwrap deterministic in-memory event fixtures"
)]
mod tests {
    use super::*;
    use rapport_files::InMemoryFileSystem;

    #[test]
    fn telemetry_writer_appends_jsonl_events() {
        let mut fs = InMemoryFileSystem::default();
        let writer = TelemetryWriter::new(RapportPaths::new("/repo"));

        writer
            .append(
                &mut fs,
                &CommandEvent::new(
                    "2026-07-07T23:00:00Z",
                    vec![String::from("build"), String::from("PRIVATE ARGUMENT")],
                    "build",
                    CommandEventOutcome::Failure,
                    2,
                ),
            )
            .unwrap();

        let contents = fs.read_to_string("/repo/.rapport/events.jsonl").unwrap();
        let event: CommandEvent = serde_json::from_str(contents.lines().next().unwrap()).unwrap();

        assert_eq!(event.schema_version, EVENT_SCHEMA_VERSION);
        assert_eq!(event.argument_count, 2);
        assert_eq!(event.command, "build");
        assert_eq!(event.outcome, CommandEventOutcome::Failure);
        assert_eq!(event.exit_code, 2);
        assert!(!contents.contains("PRIVATE"));
        assert!(!format!("{event:?}").contains("PRIVATE"));
    }

    #[test]
    fn command_event_reads_legacy_lines_without_retaining_argv() {
        let event: CommandEvent = serde_json::from_str(
            r#"{"schema_version":1,"timestamp":"2026-07-07T23:00:00Z","argv":["PRIVATE"],"command":"build","outcome":"success","exit_code":0}"#,
        )
        .unwrap();

        assert_eq!(event.schema_version, 1);
        assert_eq!(event.argument_count, 1);
        assert_eq!(event.command, "build");
        assert!(!format!("{event:?}").contains("PRIVATE"));
    }

    #[test]
    fn telemetry_writer_sanitizes_durable_legacy_argv_before_appending() {
        let mut fs = InMemoryFileSystem::default();
        fs.write_string(
            "/repo/.rapport/events.jsonl",
            concat!(
                "{\"schema_version\":1,\"timestamp\":\"2026-07-07T22:00:00Z\",\"argv\":[\"PRIVATE\",\"SECOND\"],\"command\":\"integrate\",\"outcome\":\"success\",\"exit_code\":0}\n",
                "{\"argv\":[\"PRIVATE MALFORMED\"]\n",
            ),
        )
        .unwrap();
        let writer = TelemetryWriter::new(RapportPaths::new("/repo"));

        writer
            .append(
                &mut fs,
                &CommandEvent::new(
                    "2026-07-07T23:00:00Z",
                    vec![String::from("build")],
                    "build",
                    CommandEventOutcome::Success,
                    0,
                ),
            )
            .unwrap();

        let contents = fs.read_to_string("/repo/.rapport/events.jsonl").unwrap();
        assert!(!contents.contains("PRIVATE"));
        assert!(!contents.contains("\"argv\""));
        let events = contents
            .lines()
            .map(|line| serde_json::from_str::<CommandEvent>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].schema_version, EVENT_SCHEMA_VERSION);
        assert_eq!(events[0].argument_count, 2);
        assert_eq!(events[0].command, "integrate");
        assert_eq!(events[1].argument_count, 1);
        assert_eq!(events[1].command, "build");
    }
}
