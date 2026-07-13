//! Crate-level behavior tests.

use super::{BatchRunner, CommandOutcome, CommandSpec, Job, MachineResources, ResourceKey, Runner};
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
        Ok(CommandOutcome::new(
            true,
            Some(0),
            Vec::new(),
            Vec::new(),
            Duration::ZERO,
        ))
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
fn debug_output_includes_operations_and_redacts_environment_values() {
    let spec = CommandSpec::new("tool")
        .arg("PRIVATE ARGUMENT")
        .env("TOKEN", "PRIVATE TOKEN");
    let outcome = CommandOutcome::new(
        false,
        Some(1),
        b"PRIVATE STDOUT".to_vec(),
        b"PRIVATE STDERR".to_vec(),
        Duration::ZERO,
    );

    let debug = format!("{spec:?} {outcome:?}");

    assert!(debug.contains("PRIVATE ARGUMENT"));
    assert!(debug.contains("PRIVATE STDOUT"));
    assert!(debug.contains("PRIVATE STDERR"));
    assert!(debug.contains("TOKEN"));
    assert!(!debug.contains("PRIVATE TOKEN"));
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
fn batch_reports_started_before_finished_for_incremental_persistence() {
    let runner = CountingRunner::default();
    let batch = BatchRunner::new(
        NonZeroUsize::new(2).unwrap_or(NonZeroUsize::MIN),
        MachineResources::new(unique_lock_directory("events")),
    );
    let jobs = ["first", "second"]
        .into_iter()
        .map(|name| Job::new(name, CommandSpec::new("test")))
        .collect();
    let mut events = Vec::new();

    let outcomes = batch.run_with_events(&runner, jobs, |event| {
        events.push((event.name().to_owned(), event.outcome().is_some()));
    });

    assert_eq!(outcomes.len(), 2);
    for name in ["first", "second"] {
        let started = events
            .iter()
            .position(|event| event == &(name.to_owned(), false));
        let finished = events
            .iter()
            .position(|event| event == &(name.to_owned(), true));
        assert!(started.is_some_and(|started| finished.is_some_and(|done| started < done)));
    }
}

#[test]
fn batch_serializes_jobs_sharing_a_machine_resource() {
    let runner = CountingRunner::default();
    let lock_directory = unique_lock_directory("exclusive");
    let resources = MachineResources::new(&lock_directory);
    let batch = BatchRunner::new(NonZeroUsize::new(3).unwrap_or(NonZeroUsize::MIN), resources);
    let Ok(resource) = ResourceKey::new("macos-screen") else {
        panic!("test resource should be valid");
    };
    let jobs = (0..3)
        .map(|index| {
            Job::new(format!("job-{index}"), CommandSpec::new("unused")).requiring(resource.clone())
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
