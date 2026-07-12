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
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RequiredSignoff {
    pub(crate) id: String,
    pub(crate) source_context: String,
    pub(crate) working_directory: String,
    pub(crate) target: String,
    pub(crate) identity: String,
    pub(crate) stage: u32,
    pub(crate) resource_group: Option<String>,
    pub(crate) triggers: Vec<String>,
    pub(crate) contract_digest: String,
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
            workflow::validate_file(
                fs,
                repo_root,
                matched.record.context().id(),
                matched.record.directory(),
                matched.signoff,
            )?;
            workflow::validate_shared(fs, repo_root)?;
            let rendered = workflow::render(
                matched.record.context().id(),
                matched.record.directory(),
                repo_root,
                matched.signoff,
            );
            let directory = matched
                .record
                .directory()
                .strip_prefix(repo_root)
                .unwrap_or(matched.record.directory());
            let working_directory = if directory.as_str().is_empty() {
                ".".to_owned()
            } else {
                directory.to_string()
            };
            let contract_digest = format!(
                "{:x}",
                Sha256::digest(
                    format!(
                        "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
                        matched.signoff.id(),
                        matched.record.context().id(),
                        working_directory,
                        matched.signoff.target(),
                        matched.signoff.stage(),
                        matched.signoff.resource_group().unwrap_or("none"),
                        workflow::shared_contents(),
                        rendered
                    )
                    .as_bytes()
                )
            );
            let entry = signoffs
                .entry(matched.signoff.id().to_owned())
                .or_insert_with(|| RequiredSignoff {
                    id: matched.signoff.id().to_owned(),
                    source_context: matched.record.context().id().to_string(),
                    working_directory,
                    target: matched.signoff.target().to_owned(),
                    identity: workflow::check_name(matched.record.context().id(), matched.signoff),
                    stage: matched.signoff.stage(),
                    resource_group: matched.signoff.resource_group().map(str::to_owned),
                    triggers: Vec::new(),
                    contract_digest,
                });
            if !entry.triggers.contains(&matched.trigger) {
                entry.triggers.push(matched.trigger);
                entry.triggers.sort();
            }
        }
    }
    Ok(signoffs.into_values().collect())
}

pub(crate) fn effective_policy_digest_for_paths<'path>(
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
    paths: impl IntoIterator<Item = &'path Utf8Path>,
) -> Result<String, Error> {
    let mut rendered = paths
        .into_iter()
        .map(|path| {
            command::show(fs, repo_root, path, false).map(|policy| (path.to_string(), policy))
        })
        .collect::<Result<Vec<_>, _>>()?;
    rendered.sort_by(|left, right| left.0.cmp(&right.0));
    rendered.dedup();
    let mut digest = Sha256::new();
    if rendered.is_empty() {
        digest.update(b"no-applicable-policy");
    }
    for (path, policy) in rendered {
        digest.update(path.as_bytes());
        digest.update([0]);
        digest.update(policy.as_bytes());
        digest.update([0]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
