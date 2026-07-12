//! Contextual repository policy.

mod boundary;
mod command;
mod domain;
mod error;
mod repository;
mod workflow;

#[cfg(test)]
mod tests;

pub(crate) use command::{Cli, doctor_all, run};
pub(crate) use error::Error;

use rapport_files::{FileSystem, Utf8Path};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RequiredSignoff {
    pub(crate) id: String,
    pub(crate) source_context: String,
    pub(crate) target: String,
    pub(crate) stage: u32,
    pub(crate) resource_group: Option<String>,
    pub(crate) triggers: Vec<String>,
}

pub(crate) fn required_signoffs_for_paths<'path>(
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
    paths: impl IntoIterator<Item = &'path Utf8Path>,
) -> Result<Vec<RequiredSignoff>, Error> {
    let repository = repository::Repository::load(fs, repo_root)?;
    let mut signoffs = BTreeMap::<String, RequiredSignoff>::new();
    for path in paths {
        for matched in repository.applicable_signoffs(path)? {
            let entry = signoffs
                .entry(matched.signoff.id().to_owned())
                .or_insert_with(|| RequiredSignoff {
                    id: matched.signoff.id().to_owned(),
                    source_context: matched.record.context().id().to_string(),
                    target: matched.signoff.target().to_owned(),
                    stage: matched.signoff.stage(),
                    resource_group: matched.signoff.resource_group().map(str::to_owned),
                    triggers: Vec::new(),
                });
            if !entry.triggers.contains(&matched.trigger) {
                entry.triggers.push(matched.trigger);
                entry.triggers.sort();
            }
        }
    }
    Ok(signoffs.into_values().collect())
}
