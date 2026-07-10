use rapport_files::Utf8Path;
use std::io;

/// A command to run: a program plus its arguments.
///
/// Paired with [`CommandOutcome`] across the [`CommandRunner`] trait: spec
/// describes what to run, outcome describes what happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
}

impl CommandSpec {
    pub fn new<I, S>(program: impl Into<String>, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }
}

/// The result of a single command invocation.
///
/// Exit-code success is in [`Self::success`]; non-zero is not an error.
/// `io::Error` is reserved for failures to *invoke* the program. Output is
/// captured as `String` via `String::from_utf8_lossy`.
#[derive(Clone)]
pub struct CommandOutcome {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

impl std::fmt::Debug for CommandOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandOutcome")
            .field("success", &self.success)
            .field("stdout_bytes", &self.stdout.len())
            .field("stderr_bytes", &self.stderr.len())
            .finish()
    }
}

/// Runs external programs. Production code uses [`RealCommandRunner`];
/// tests inject a fake.
pub trait CommandRunner {
    /// Run the program described by `spec` inside `cwd`, capturing output.
    ///
    /// # Errors
    ///
    /// Returns [`io::Error`] only when the program could not be invoked.
    /// Programs that ran but exited non-zero return `Ok` with
    /// [`CommandOutcome::success`] = `false`.
    fn run(&self, spec: &CommandSpec, cwd: &Utf8Path) -> io::Result<CommandOutcome>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct RealCommandRunner;

impl CommandRunner for RealCommandRunner {
    fn run(&self, spec: &CommandSpec, cwd: &Utf8Path) -> io::Result<CommandOutcome> {
        let output = std::process::Command::new(&spec.program)
            .args(&spec.args)
            .current_dir(cwd)
            .output()?;
        Ok(CommandOutcome {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::CommandOutcome;

    #[test]
    fn command_outcome_debug_summarizes_captured_output() {
        let outcome = CommandOutcome {
            success: false,
            stdout: String::from("PRIVATE STDOUT"),
            stderr: String::from("PRIVATE STDERR"),
        };

        let debug = format!("{outcome:?}");

        assert!(!debug.contains("PRIVATE"));
        assert!(debug.contains("stdout_bytes: 14"));
        assert!(debug.contains("stderr_bytes: 14"));
    }
}
