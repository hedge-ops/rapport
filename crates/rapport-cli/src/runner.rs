use camino::Utf8Path;
use std::io;

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

/// Runs external programs. Mirrors the testability split used for [`crate::FileSystem`]:
/// production code uses [`RealCommandRunner`]; tests inject a fake.
pub trait CommandRunner {
    /// Run `program` with `args` inside `cwd`, capturing output.
    ///
    /// # Errors
    ///
    /// Returns [`io::Error`] only when the program could not be invoked.
    /// Programs that ran but exited non-zero return `Ok` with
    /// [`CommandOutcome::success`] = `false`.
    fn run(&self, program: &str, args: &[&str], cwd: &Utf8Path) -> io::Result<CommandOutcome>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct RealCommandRunner;

impl CommandRunner for RealCommandRunner {
    fn run(&self, program: &str, args: &[&str], cwd: &Utf8Path) -> io::Result<CommandOutcome> {
        let output = std::process::Command::new(program)
            .args(args)
            .current_dir(cwd)
            .output()?;
        Ok(CommandOutcome {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}
