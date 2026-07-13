//! Contextual repository policy.

mod boundary;
mod cli;
mod command;
mod doctor;
mod domain;
mod error;
mod render;
mod repository;
mod signoff;
mod workflow;

#[cfg(test)]
mod tests;

pub(crate) use cli::Cli;
pub(crate) use command::run;
pub(crate) use doctor::doctor_all;
pub(crate) use error::Error;
pub(crate) use workflow::{SHARED_PATH as SHARED_WORKFLOW_PATH, write_shared};

use rapport_files::{FileSystem, Utf8Path};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

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
            render::show(fs, repo_root, path, false).map(|policy| (path.to_string(), policy))
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

pub(crate) struct ReviewPolicy {
    pub(crate) markdown: String,
    pub(crate) minimum_grade: String,
    pub(crate) rule_ids: BTreeSet<String>,
}

pub(crate) fn review_policy_for_paths<'path>(
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
    paths: impl IntoIterator<Item = &'path Utf8Path>,
) -> Result<ReviewPolicy, Error> {
    let repository = repository::Repository::load(fs, repo_root)?;
    let mut records = BTreeMap::new();
    let mut grouped = BTreeMap::<String, Vec<String>>::new();
    let mut minimum = domain::Grade::DEFAULT;
    for path in paths {
        let nearest = repository.at(path)?;
        grouped
            .entry(nearest.context().id().to_string())
            .or_default()
            .push(path.to_string());
        minimum = minimum.max(repository.effective_grade(path)?);
        for record in repository.effective(path)? {
            records.insert(record.context().id().to_string(), record);
        }
    }
    let mut markdown = String::from("## Changed Files by Context\n\n");
    for (context, files) in &grouped {
        let _ = writeln!(markdown, "- `{context}` — {}", files.join(", "));
    }
    markdown.push_str("\n## Effective Context\n");
    let mut rule_ids = BTreeSet::new();
    let mut shared = BTreeSet::new();
    for record in records.values() {
        let context = record.context();
        let _ = write!(
            markdown,
            "\n### `{}`\n\nPurpose: {}\n\nOwnership — Prefer Here:\n",
            context.id(),
            context.purpose()
        );
        for entry in context.ownership() {
            let _ = writeln!(markdown, "- `{}` — {}", entry.id(), entry.text());
        }
        markdown.push_str("\nBoundaries — Avoid Here:\n");
        for boundary in context.boundaries() {
            let _ = writeln!(
                markdown,
                "- `{}` — {}{}",
                boundary.id(),
                boundary.text(),
                boundary
                    .owner()
                    .map_or_else(String::new, |owner| format!(" — owner `{owner}`"))
            );
        }
        markdown.push_str("\nContext-owned Rules:\n");
        for rule in context.ruleset().rules() {
            rule_ids.insert(rule.id().to_string());
            render_rule(&mut markdown, rule);
        }
        for id in context.ruleset().includes() {
            shared.insert(id.clone());
            shared.extend(
                repository
                    .shared()
                    .require(id)?
                    .transitive()
                    .iter()
                    .cloned(),
            );
        }
    }
    markdown.push_str("\n## Applicable Shared Rulesets\n");
    for id in shared {
        let path = repo_root
            .join(".rapport/rules")
            .join(id.conventional_path());
        let contents = fs.read_to_string(&path).map_err(|source| Error::Io {
            path: path.clone(),
            source,
        })?;
        for line in contents.lines() {
            if let Some(id) = line
                .trim()
                .strip_prefix("id = \"")
                .and_then(|v| v.strip_suffix('"'))
                && id.rsplit_once('_').is_some_and(|(_, suffix)| {
                    suffix.len() == 3 && suffix.bytes().all(|byte| byte.is_ascii_digit())
                })
            {
                rule_ids.insert(id.to_owned());
            }
        }
        let _ = write!(markdown, "\n### `{id}`\n\n```toml\n{contents}```\n");
    }
    Ok(ReviewPolicy {
        markdown,
        minimum_grade: minimum.to_string(),
        rule_ids,
    })
}

fn render_rule(markdown: &mut String, rule: &crate::shared_ruleset::Rule) {
    let _ = write!(
        markdown,
        "\n- `{}` — {}\n  - Rationale: {}\n  - Avoid ({}): {}\n  - Prefer ({}): {}{}\n",
        rule.id(),
        rule.text(),
        rule.rationale(),
        rule.avoid().language().as_str(),
        rule.avoid().text(),
        rule.prefer().language().as_str(),
        rule.prefer().text(),
        rule.reference()
            .map_or_else(String::new, |reference| format!(
                "\n  - Reference: {}",
                reference.markdown()
            ))
    );
}
