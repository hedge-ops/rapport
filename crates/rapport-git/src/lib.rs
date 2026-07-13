//! Concrete Git repository semantics for Rapport.
//!
//! The crate root exposes validated Git domain values, repository operations,
//! and one primary error while focused modules own their implementations.

mod domain;
mod error;
mod repository;

pub use domain::{
    BranchName, LocalBranch, ObjectId, Operation, RebaseOutcome, RemoteTrackingBranch, Repository,
    Revision, SourceSideChanges, WorktreeStatus,
};
pub use error::{GitError, InvalidBranchName, InvalidObjectId, InvalidRevision};
pub use repository::Git;

#[cfg(test)]
mod tests;
