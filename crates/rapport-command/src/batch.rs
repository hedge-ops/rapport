//! Concurrent command batches with resource serialization.
//!
//! This module owns jobs, lifecycle events, ordered outcomes, and worker
//! coordination while delegating process execution and locking to peer APIs.

use crate::{CommandOutcome, CommandSpec, MachineResources, ResourceKey, Runner};
use std::collections::VecDeque;
use std::io;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, mpsc};

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

/// A lifecycle event emitted while a concurrent batch is running.
#[derive(Debug)]
pub enum JobEvent {
    /// A worker has acquired any resource and is about to run the command.
    Started { name: String },
    /// The command or resource acquisition finished.
    Finished(JobOutcome),
}

impl JobEvent {
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Started { name } => name,
            Self::Finished(outcome) => outcome.name(),
        }
    }

    #[must_use]
    pub fn outcome(&self) -> Option<&JobOutcome> {
        match self {
            Self::Started { .. } => None,
            Self::Finished(outcome) => Some(outcome),
        }
    }
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
        self.run_with_events(runner, jobs, |_| {})
    }

    /// Run every job and report lifecycle events on the calling thread.
    ///
    /// The callback can safely persist incremental state without requiring the
    /// persistence implementation itself to be thread-safe.
    #[must_use]
    pub fn run_with_events<R, F>(
        &self,
        runner: &R,
        jobs: Vec<Job>,
        mut on_event: F,
    ) -> Vec<JobOutcome>
    where
        R: Runner,
        F: FnMut(&JobEvent),
    {
        let job_count = jobs.len();
        if job_count == 0 {
            return Vec::new();
        }

        let queue = Arc::new(Mutex::new(
            jobs.into_iter().enumerate().collect::<VecDeque<_>>(),
        ));
        let (events, received_events) = mpsc::channel();
        let worker_count = self.max_parallelism.get().min(job_count);

        std::thread::scope(|scope| {
            for _ in 0..worker_count {
                let queue = Arc::clone(&queue);
                let resources = self.resources.clone();
                let events = events.clone();
                scope.spawn(move || {
                    loop {
                        let next = match queue.lock() {
                            Ok(mut queue) => queue.pop_front(),
                            Err(poisoned) => poisoned.into_inner().pop_front(),
                        };
                        let Some((index, job)) = next else {
                            break;
                        };

                        let result = if let Some(resource) = job.resource.as_ref() {
                            resources.acquire(resource).and_then(|_guard| {
                                let _ = events.send((
                                    index,
                                    JobEvent::Started {
                                        name: job.name.clone(),
                                    },
                                ));
                                runner.run(&job.command)
                            })
                        } else {
                            let _ = events.send((
                                index,
                                JobEvent::Started {
                                    name: job.name.clone(),
                                },
                            ));
                            runner.run(&job.command)
                        };
                        let event = (
                            index,
                            JobEvent::Finished(JobOutcome {
                                name: job.name,
                                result,
                            }),
                        );
                        let _ = events.send(event);
                    }
                });
            }

            drop(events);
            let mut finished = 0;
            let mut outcomes = Vec::with_capacity(job_count);
            while finished < job_count {
                let Ok((index, event)) = received_events.recv() else {
                    break;
                };
                on_event(&event);
                if let JobEvent::Finished(outcome) = event {
                    outcomes.push((index, outcome));
                    finished += 1;
                }
            }
            outcomes.sort_by_key(|(index, _)| *index);
            outcomes.into_iter().map(|(_, outcome)| outcome).collect()
        })
    }
}
