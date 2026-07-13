//! Worktree-local Work and Task ledger.

mod build;
mod command;
mod develop;
mod domain;
mod error;
mod grade;
mod history;
mod integrate;
mod repository;
mod review;

#[cfg(test)]
mod tests;

pub(crate) use build::{Cli as BuildCli, run as run_build};
pub(crate) use command::{Cli, run};
pub(crate) use develop::{Cli as DevelopCli, run as run_develop};
pub(crate) use error::Error;
pub(crate) use integrate::{Cli as IntegrateCli, run as run_integrate};
pub(crate) use review::{Cli as ReviewCli, run as run_review};

pub(crate) fn active_target(
    fs: &impl rapport_files::FileSystem,
    root: &rapport_files::Utf8Path,
) -> Result<Option<String>, Error> {
    repository::Store::new(root)
        .load_work(fs)
        .map(|work| work.map(|work| work.target_branch))
}
