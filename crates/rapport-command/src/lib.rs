//! External process execution for Rapport.
//!
//! The crate root exposes typed command, machine-resource, and concurrent batch
//! APIs while their implementations remain in focused owned modules.

mod batch;
mod command;
mod error;
mod resource;

pub use batch::{BatchRunner, Job, JobEvent, JobOutcome};
pub use command::{CommandOutcome, CommandSpec, Runner, SystemRunner};
pub use error::InvalidResourceKey;
pub use resource::{MachineResources, ResourceGuard, ResourceKey};

#[cfg(test)]
mod tests;
