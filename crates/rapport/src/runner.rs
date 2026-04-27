use camino::Utf8Path;
use std::io;

/// A command to run: a program plus its arguments.
///
/// Paired with [`CommandOutcome`] across the [`CommandRunner`] trait — spec
/// describes what to run, outcome describes what happened.
#[derive(Debug, Clone, Copy)]
pub struct CommandSpec {
    pub program: &'static str,
    pub args: &'static [&'static str],
}

/// The result of a single command invocation.
///
/// Exit-code success is in [`Self::success`]; non-zero is not an error.
/// `io::Error` is reserved for failures to *invoke* the program. Output is
/// captured as `String` via `String::from_utf8_lossy`.
#[derive(Debug, Clone)]
pub struct CommandOutcome {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
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
        let output = std::process::Command::new(spec.program)
            .args(spec.args)
            .current_dir(cwd)
            .output()?;
        Ok(CommandOutcome {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}
