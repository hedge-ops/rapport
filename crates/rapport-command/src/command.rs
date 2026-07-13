//! Typed external-command specifications and execution outcomes.
//!
//! This module owns process invocation data and the operating-system runner.

use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// A program invocation, including its working directory and environment.
#[derive(Clone, PartialEq, Eq)]
pub struct CommandSpec {
    program: String,
    args: Vec<String>,
    current_dir: Option<PathBuf>,
    environment: BTreeMap<String, String>,
}

impl CommandSpec {
    #[must_use]
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            current_dir: None,
            environment: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    #[must_use]
    pub fn current_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.current_dir = Some(path.into());
        self
    }

    #[must_use]
    pub fn env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.environment.insert(name.into(), value.into());
        self
    }

    #[must_use]
    pub fn program(&self) -> &str {
        &self.program
    }

    #[must_use]
    pub fn arguments(&self) -> &[String] {
        &self.args
    }

    #[must_use]
    pub fn working_directory(&self) -> Option<&Path> {
        self.current_dir.as_deref()
    }
}

impl fmt::Debug for CommandSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CommandSpec")
            .field("program", &self.program)
            .field("argument_count", &self.args.len())
            .field("current_dir", &self.current_dir)
            .field("environment_variable_count", &self.environment.len())
            .finish()
    }
}

/// Captured output from a program that was successfully started.
#[derive(Clone, PartialEq, Eq)]
pub struct CommandOutcome {
    success: bool,
    exit_code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    elapsed: Duration,
}

impl CommandOutcome {
    #[must_use]
    pub fn new(
        success: bool,
        exit_code: Option<i32>,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        elapsed: Duration,
    ) -> Self {
        Self {
            success,
            exit_code,
            stdout,
            stderr,
            elapsed,
        }
    }

    #[must_use]
    pub fn success(&self) -> bool {
        self.success
    }

    #[must_use]
    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    #[must_use]
    pub fn stdout(&self) -> &[u8] {
        &self.stdout
    }

    #[must_use]
    pub fn stderr(&self) -> &[u8] {
        &self.stderr
    }

    #[must_use]
    pub fn stdout_lossy(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    #[must_use]
    pub fn stderr_lossy(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }

    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }
}

impl fmt::Debug for CommandOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CommandOutcome")
            .field("success", &self.success)
            .field("exit_code", &self.exit_code)
            .field("stdout_bytes", &self.stdout.len())
            .field("stderr_bytes", &self.stderr.len())
            .field("elapsed", &self.elapsed)
            .finish()
    }
}

/// Executes command specifications.
pub trait Runner: Send + Sync {
    /// Run a command and capture its output.
    ///
    /// A non-zero exit is a successful invocation represented by
    /// [`CommandOutcome::success`]. Errors mean the process could not be run.
    ///
    /// # Errors
    ///
    /// Returns the operating-system error when the process cannot be started or
    /// waited on.
    fn run(&self, spec: &CommandSpec) -> io::Result<CommandOutcome>;
}

/// Executes commands through the operating system.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemRunner;

impl Runner for SystemRunner {
    fn run(&self, spec: &CommandSpec) -> io::Result<CommandOutcome> {
        let started_at = Instant::now();
        let mut command = Command::new(&spec.program);
        command.args(&spec.args).envs(&spec.environment);
        if let Some(current_dir) = &spec.current_dir {
            command.current_dir(current_dir);
        }
        let output = command.output()?;
        Ok(CommandOutcome {
            success: output.status.success(),
            exit_code: output.status.code(),
            stdout: output.stdout,
            stderr: output.stderr,
            elapsed: started_at.elapsed(),
        })
    }
}
