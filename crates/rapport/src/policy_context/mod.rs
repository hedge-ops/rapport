//! Contextual repository policy.
//!
//! This module composes Context storage, effective policy projection, signoff requirements, commands, and diagnostics.

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
            records.insert(
                (record.directory().components().count(), record.path()),
                record,
            );
        }
    }
    let mut markdown = String::from("## Selected Paths by Context\n\n");
    for (context, files) in &grouped {
        let _ = writeln!(markdown, "- `{context}` — {}", files.join(", "));
    }
    markdown.push_str("\n## Effective Context\n");
    let mut rule_ids = BTreeSet::new();
    let mut shared = BTreeSet::new();
    let mut standards = BTreeMap::new();
    for record in records.values() {
        let context = record.context();
        render_architecture(&mut markdown, record, repo_root);
        for rule in context.ruleset().rules() {
            collect_standard(
                &mut standards,
                rule,
                render::display(repo_root, record.path()),
            )?;
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
        let summary = repository.shared().require(&id)?;
        let source = render::display(repo_root, summary.path());
        let _ = writeln!(
            markdown,
            "\n- `{id}` — {} — source `{source}`",
            summary.purpose()
        );
        for rule in summary.rules() {
            collect_standard(&mut standards, rule, source.clone())?;
        }
    }
    markdown.push_str("\n## Review Benchmarks\n");
    for (id, (rule, sources)) in standards {
        rule_ids.insert(id);
        render_rule(&mut markdown, rule);
        let _ = writeln!(
            markdown,
            "\nSources: {}",
            sources
                .into_iter()
                .map(|source| format!("`{source}`"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(ReviewPolicy {
        markdown,
        minimum_grade: minimum.to_string(),
        rule_ids,
    })
}

fn render_architecture(markdown: &mut String, record: &repository::Record, repo_root: &Utf8Path) {
    let context = record.context();
    let _ = write!(
        markdown,
        "\n### `{}`\n\nPurpose: {}\n\nOwnership — Prefer Here:\n\n",
        context.id(),
        context.purpose()
    );
    for entry in context.ownership() {
        let _ = writeln!(markdown, "- `{}` — {}", entry.id(), entry.text());
    }
    markdown.push_str("\nBoundaries — Avoid Here:\n\n");
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
    let _ = writeln!(
        markdown,
        "\nSource: `{}`",
        render::display(repo_root, record.path())
    );
    if let Some(kind) = context.component_type() {
        let _ = writeln!(markdown, "\nType: `{kind}`");
    }
}

fn collect_standard<'rule>(
    standards: &mut BTreeMap<String, (&'rule crate::shared_ruleset::Rule, BTreeSet<String>)>,
    rule: &'rule crate::shared_ruleset::Rule,
    source: String,
) -> Result<(), Error> {
    if let Some((existing, sources)) = standards.get_mut(rule.id().as_str()) {
        if *existing != rule {
            return Err(crate::shared_ruleset::Error::ConflictingRule {
                rule: rule.id().to_string(),
                first: sources.iter().cloned().collect::<Vec<_>>().join(", "),
                second: source,
            }
            .into());
        }
        sources.insert(source);
    } else {
        standards.insert(rule.id().to_string(), (rule, BTreeSet::from([source])));
    }
    Ok(())
}

fn render_rule(markdown: &mut String, rule: &crate::shared_ruleset::Rule) {
    let _ = write!(
        markdown,
        "\n### `{}`\n\n{}\n\nRationale: {}\n",
        rule.id(),
        rule.text(),
        rule.rationale()
    );
    for (label, example) in [("Avoid", rule.avoid()), ("Prefer", rule.prefer())] {
        let longest = example
            .text()
            .split(|character| character != '`')
            .map(str::len)
            .max()
            .unwrap_or(0);
        let fence = "`".repeat(3.max(longest + 1));
        let _ = write!(
            markdown,
            "\n{label} ({}):\n\n{fence}{}\n{}\n{fence}\n",
            example.language().as_str(),
            example.language().as_str(),
            example.text()
        );
    }
    if let Some(reference) = rule.reference() {
        let _ = writeln!(markdown, "\nReference: {}", reference.markdown());
    }
}
