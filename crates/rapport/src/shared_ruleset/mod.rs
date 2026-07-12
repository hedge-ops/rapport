//! Shared repository standards.
//!
//! Owns Phase 1 Ruleset commands and domain behavior. Legacy Context Rules
//! remain isolated in the previous implementation until Phase 2 replaces them.

mod boundary;
mod catalog;
mod command;
mod domain;
mod error;
mod policy;
mod repository;

pub(crate) use command::{Cli, run};
pub(crate) use domain::{
    ExampleUpdate, NewRule, Reference, ReferenceUpdate, RuleUpdate, Ruleset, RulesetId,
};
pub(crate) use error::Error;
pub(crate) use policy::SharedRulesets;
