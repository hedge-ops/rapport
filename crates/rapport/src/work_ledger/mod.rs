//! Worktree-local Work and Task ledger.
//!
//! This module composes active Work, development Tasks, validation proofs, integration, and finalized history.

mod build;
mod checkpoint;
mod cli;
mod command;
mod develop;
mod domain;
mod error;
mod grade;
mod history;
mod integrate;
mod rebase;
mod repository;
mod review;
mod status;

#[cfg(test)]
mod tests;

pub(crate) use build::{Cli as BuildCli, run as run_build};
pub(crate) use cli::Cli;
pub(crate) use command::run;
pub(crate) use develop::{Cli as DevelopCli, run as run_develop};
pub(crate) use error::Error;
pub(crate) use integrate::{Cli as IntegrateCli, run as run_integrate};
pub(crate) use review::{Cli as ReviewCli, run as run_review};
