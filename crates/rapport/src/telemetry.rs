use crate::paths::RapportPaths;
use rapport_files::FileSystem;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;
use std::io;

pub const EVENT_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandEventOutcome {
    Success,
    Failure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandEvent {
    pub schema_version: u16,
    pub timestamp: String,
    pub argv: Vec<String>,
    pub command: String,
    pub outcome: CommandEventOutcome,
    pub exit_code: u8,
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
        Self {
            schema_version: EVENT_SCHEMA_VERSION,
            timestamp: timestamp.into(),
            argv,
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
        let line = serde_json::to_string(event)?;
        fs.append_line(self.paths.events_file(), line)?;
        Ok(())
    }
}

#[derive(Debug)]
pub enum TelemetryError {
    Io(io::Error),
    Encode(serde_json::Error),
}

impl fmt::Display for TelemetryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "telemetry filesystem error: {error}"),
            Self::Encode(error) => write!(f, "telemetry encode error: {error}"),
        }
    }
}

impl Error for TelemetryError {}

impl From<io::Error> for TelemetryError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for TelemetryError {
    fn from(error: serde_json::Error) -> Self {
        Self::Encode(error)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
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
                    vec![String::from("build")],
                    "build",
                    CommandEventOutcome::Failure,
                    2,
                ),
            )
            .unwrap();

        let contents = fs.read_to_string("/repo/.rapport/events.jsonl").unwrap();
        let event: CommandEvent = serde_json::from_str(contents.lines().next().unwrap()).unwrap();

        assert_eq!(event.schema_version, EVENT_SCHEMA_VERSION);
        assert_eq!(event.command, "build");
        assert_eq!(event.outcome, CommandEventOutcome::Failure);
        assert_eq!(event.exit_code, 2);
    }
}
