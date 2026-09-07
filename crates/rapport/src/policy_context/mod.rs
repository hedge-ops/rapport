//! Repository architecture and review benchmarks.
//!
//! Composes context storage, inherited standards, validation, and sourced prompts.

mod boundary;
mod cli;
mod command;
mod domain;
mod error;
mod render;
mod repository;
mod validation;

#[cfg(test)]
mod tests;

pub(crate) use cli::Cli;
pub(crate) use command::run;
pub(crate) use error::Error;

use rapport_files::{FileSystem, Utf8Path};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

pub(crate) struct ReviewPolicy {
    pub(crate) markdown: String,
}

pub(crate) fn review_policy_for_paths<'path>(
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
    paths: impl IntoIterator<Item = &'path Utf8Path>,
) -> Result<ReviewPolicy, Error> {
    let repository = repository::Repository::load(fs, repo_root)?;
    review_policy(&repository, repo_root, paths)
}

fn review_policy<'path>(
    repository: &repository::Repository,
    repo_root: &Utf8Path,
    paths: impl IntoIterator<Item = &'path Utf8Path>,
) -> Result<ReviewPolicy, Error> {
    let mut records = BTreeMap::new();
    let mut grouped = BTreeMap::<String, Vec<String>>::new();
    for path in paths {
        let nearest = repository.at(path)?;
        grouped
            .entry(nearest.context().id().to_string())
            .or_default()
            .push(path.to_string());
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
    let mut shared = BTreeSet::new();
    let mut standards = BTreeMap::new();
    for record in records.values() {
        let context = record.context();
        render_architecture(&mut markdown, record, repo_root, repository);
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
    for (rule, sources) in standards.into_values() {
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
    Ok(ReviewPolicy { markdown })
}

fn render_architecture(
    markdown: &mut String,
    record: &repository::Record,
    repo_root: &Utf8Path,
    repository: &repository::Repository,
) {
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
    render::render_component_declarations(markdown, record, repo_root, repository);
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
