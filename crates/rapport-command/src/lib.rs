//! External process execution for Rapport.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
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

/// A validated name for an exclusive machine-local resource.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResourceKey(String);

impl ResourceKey {
    /// Create a resource key safe for use as a lock filename.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidResourceKey`] when the key is empty or contains
    /// anything other than ASCII letters, digits, `.`, `_`, or `-`.
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidResourceKey> {
        let value = value.into();
        if value.is_empty()
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        {
            return Err(InvalidResourceKey(value));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A resource key that cannot safely identify a lock file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidResourceKey(String);

impl fmt::Display for InvalidResourceKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid machine resource key: {:?}", self.0)
    }
}

impl std::error::Error for InvalidResourceKey {}

/// Coordinates named exclusive resources across processes on one machine.
#[derive(Debug, Clone)]
pub struct MachineResources {
    lock_directory: PathBuf,
}

impl MachineResources {
    #[must_use]
    pub fn new(lock_directory: impl Into<PathBuf>) -> Self {
        Self {
            lock_directory: lock_directory.into(),
        }
    }

    /// Use Rapport's stable lock directory beneath the current user's temporary
    /// directory.
    #[must_use]
    pub fn rapport_default() -> Self {
        Self::new(std::env::temp_dir().join("rapport").join("resources"))
    }

    /// Wait until the named resource is available and hold it until the returned
    /// guard is dropped.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the lock directory or lock file cannot be
    /// created, or when the operating system cannot acquire the file lock.
    pub fn acquire(&self, key: &ResourceKey) -> io::Result<ResourceGuard> {
        std::fs::create_dir_all(&self.lock_directory)?;
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(self.lock_directory.join(format!("{}.lock", key.as_str())))?;
        File::lock(&file)?;
        Ok(ResourceGuard { file })
    }
}

/// Holds an exclusive machine resource until dropped.
#[derive(Debug)]
pub struct ResourceGuard {
    file: File,
}

impl Drop for ResourceGuard {
    fn drop(&mut self) {
        let _ = File::unlock(&self.file);
    }
}

/// One named command in a concurrent batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    name: String,
    command: CommandSpec,
    resource: Option<ResourceKey>,
}

impl Job {
    #[must_use]
    pub fn new(name: impl Into<String>, command: CommandSpec) -> Self {
        Self {
            name: name.into(),
            command,
            resource: None,
        }
    }

    #[must_use]
    pub fn requiring(mut self, resource: ResourceKey) -> Self {
        self.resource = Some(resource);
        self
    }
}

/// The result of one batch job.
#[derive(Debug)]
pub struct JobOutcome {
    name: String,
    result: io::Result<CommandOutcome>,
}

impl JobOutcome {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn result(&self) -> &io::Result<CommandOutcome> {
        &self.result
    }

    /// Consume the job outcome and return the underlying command result.
    ///
    /// # Errors
    ///
    /// Returns the process invocation or resource-lock error recorded for this
    /// job.
    pub fn into_result(self) -> io::Result<CommandOutcome> {
        self.result
    }
}

/// Runs independent commands concurrently while respecting resource keys.
#[derive(Debug, Clone)]
pub struct BatchRunner {
    max_parallelism: NonZeroUsize,
    resources: MachineResources,
}

impl BatchRunner {
    #[must_use]
    pub fn new(max_parallelism: NonZeroUsize, resources: MachineResources) -> Self {
        Self {
            max_parallelism,
            resources,
        }
    }

    /// Run every job, preserving input order in the returned outcomes.
    #[must_use]
    pub fn run<R: Runner>(&self, runner: &R, jobs: Vec<Job>) -> Vec<JobOutcome> {
        let job_count = jobs.len();
        if job_count == 0 {
            return Vec::new();
        }

        let queue = Arc::new(Mutex::new(
            jobs.into_iter().enumerate().collect::<VecDeque<_>>(),
        ));
        let outcomes = Arc::new(Mutex::new(Vec::with_capacity(job_count)));
        let worker_count = self.max_parallelism.get().min(job_count);

        std::thread::scope(|scope| {
            for _ in 0..worker_count {
                let queue = Arc::clone(&queue);
                let outcomes = Arc::clone(&outcomes);
                let resources = self.resources.clone();
                scope.spawn(move || {
                    loop {
                        let next = match queue.lock() {
                            Ok(mut queue) => queue.pop_front(),
                            Err(poisoned) => poisoned.into_inner().pop_front(),
                        };
                        let Some((index, job)) = next else {
                            break;
                        };

                        let result = match job.resource.as_ref() {
                            Some(resource) => resources
                                .acquire(resource)
                                .and_then(|_guard| runner.run(&job.command)),
                            None => runner.run(&job.command),
                        };
                        let outcome = (
                            index,
                            JobOutcome {
                                name: job.name,
                                result,
                            },
                        );
                        match outcomes.lock() {
                            Ok(mut outcomes) => outcomes.push(outcome),
                            Err(poisoned) => poisoned.into_inner().push(outcome),
                        }
                    }
                });
            }
        });

        let mut outcomes = match Arc::try_unwrap(outcomes) {
            Ok(outcomes) => match outcomes.into_inner() {
                Ok(outcomes) => outcomes,
                Err(poisoned) => poisoned.into_inner(),
            },
            Err(outcomes) => match outcomes.lock() {
                Ok(mut outcomes) => std::mem::take(&mut *outcomes),
                Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
            },
        };
        outcomes.sort_by_key(|(index, _)| *index);
        outcomes.into_iter().map(|(_, outcome)| outcome).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BatchRunner, CommandOutcome, CommandSpec, Job, MachineResources, ResourceKey, Runner,
    };
    use std::io;
    use std::num::NonZeroUsize;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, SystemTime};

    #[derive(Debug, Default)]
    struct CountingRunner {
        active: AtomicUsize,
        greatest_parallelism: AtomicUsize,
    }

    impl Runner for CountingRunner {
        fn run(&self, _spec: &CommandSpec) -> io::Result<CommandOutcome> {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.greatest_parallelism
                .fetch_max(active, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(20));
            self.active.fetch_sub(1, Ordering::SeqCst);
            Ok(CommandOutcome {
                success: true,
                exit_code: Some(0),
                stdout: Vec::new(),
                stderr: Vec::new(),
                elapsed: Duration::ZERO,
            })
        }
    }

    fn unique_lock_directory(test_name: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("rapport-command-{test_name}-{unique}"))
    }

    #[test]
    fn debug_output_redacts_arguments_environment_and_output() {
        let spec = CommandSpec::new("tool")
            .arg("PRIVATE ARGUMENT")
            .env("TOKEN", "PRIVATE TOKEN");
        let outcome = CommandOutcome {
            success: false,
            exit_code: Some(1),
            stdout: b"PRIVATE STDOUT".to_vec(),
            stderr: b"PRIVATE STDERR".to_vec(),
            elapsed: Duration::ZERO,
        };

        let debug = format!("{spec:?} {outcome:?}");

        assert!(!debug.contains("PRIVATE"));
        assert!(debug.contains("argument_count: 1"));
        assert!(debug.contains("environment_variable_count: 1"));
        assert!(debug.contains("stdout_bytes: 14"));
    }

    #[test]
    fn batch_runs_unrestricted_jobs_concurrently() {
        let runner = CountingRunner::default();
        let batch = BatchRunner::new(
            NonZeroUsize::new(3).unwrap_or(NonZeroUsize::MIN),
            MachineResources::new(unique_lock_directory("concurrent")),
        );
        let jobs = (0..3)
            .map(|index| Job::new(format!("job-{index}"), CommandSpec::new("unused")))
            .collect();

        let outcomes = batch.run(&runner, jobs);

        assert_eq!(outcomes.len(), 3);
        assert!(runner.greatest_parallelism.load(Ordering::SeqCst) > 1);
    }

    #[test]
    fn batch_serializes_jobs_sharing_a_machine_resource() {
        let runner = CountingRunner::default();
        let lock_directory = unique_lock_directory("exclusive");
        let resources = MachineResources::new(&lock_directory);
        let batch = BatchRunner::new(NonZeroUsize::new(3).unwrap_or(NonZeroUsize::MIN), resources);
        let resource = ResourceKey::new("macos-screen").expect("valid test resource");
        let jobs = (0..3)
            .map(|index| {
                Job::new(format!("job-{index}"), CommandSpec::new("unused"))
                    .requiring(resource.clone())
            })
            .collect();

        let outcomes = batch.run(&runner, jobs);

        assert_eq!(outcomes.len(), 3);
        assert_eq!(runner.greatest_parallelism.load(Ordering::SeqCst), 1);
        let _ = std::fs::remove_dir_all(lock_directory);
    }

    #[test]
    fn resource_keys_are_safe_lock_filenames() {
        assert!(ResourceKey::new("macos-screen_1.0").is_ok());
        assert!(ResourceKey::new("").is_err());
        assert!(ResourceKey::new("../outside").is_err());
        assert!(ResourceKey::new("contains spaces").is_err());
    }
}
