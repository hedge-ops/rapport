//! Context policy rendering.
//!
//! This module owns list and show views, effective Ruleset attribution, signoff
//! presentation, and stable display helpers shared by mutations.

use super::Error;
use super::repository::{Record, Repository};
use crate::shared_ruleset::RulesetId;
use rapport_files::{FileSystem, Utf8Path};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

pub(super) fn list(
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
    path: &Utf8Path,
) -> Result<String, Error> {
    let repository = Repository::load(fs, repo_root)?;
    let records = repository.descendants(path)?;
    let lines = records
        .iter()
        .map(|record| {
            format!(
                "- `{}` — {} — {}",
                record.context().id(),
                record.context().purpose(),
                display(repo_root, record.directory())
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!("# rapport context list\n\n{}", or_none(&lines)))
}

pub(super) fn show(
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
    path: &Utf8Path,
    declared: bool,
) -> Result<String, Error> {
    let repository = Repository::load(fs, repo_root)?;
    let records = if declared {
        vec![repository.at(path)?]
    } else {
        repository.effective(path)?
    };
    let grade = repository.effective_grade(path)?;
    let nearest = records
        .last()
        .ok_or_else(|| Error::MissingContext(path.to_path_buf()))?;
    let mut output = String::from("# rapport context show\n");
    for record in &records {
        render_context_record(&mut output, record, repo_root, nearest);
    }

    let shared = shared_attributions(&records, nearest, &repository)?;
    render_shared_rulesets(&mut output, &shared, &repository)?;
    render_required_signoffs(&mut output, declared, nearest, &repository, path, repo_root)?;
    let digest = format!("{:x}", Sha256::digest(output.as_bytes()));
    let _ = write!(
        output,
        "\n\n- `effective review minimum` — {grade}\n- `policy digest` — `{digest}`"
    );
    Ok(output)
}

fn render_context_record(
    output: &mut String,
    record: &Record,
    repo_root: &Utf8Path,
    nearest: &Record,
) {
    let scope = if record.context().id() == nearest.context().id() {
        "direct"
    } else {
        "inherited"
    };
    let _ = write!(
        output,
        "\n## `{}`\n\n- `path` — {}\n- `scope` — {scope}\n- `purpose` — {}\n- `embedded Ruleset` — `{}`\n- `declared review minimum` — {}\n",
        record.context().id(),
        display(repo_root, record.directory()),
        record.context().purpose(),
        record.context().ruleset().id(),
        record
            .context()
            .minimum_grade()
            .map_or_else(|| "inherited".to_owned(), |grade| grade.to_string())
    );
    output.push_str("\n### Ownership — Prefer Here\n\n");
    output.push_str(&or_none(
        &record
            .context()
            .ownership()
            .iter()
            .map(|entry| format!("- `{}` — {}", entry.id(), entry.text()))
            .collect::<Vec<_>>()
            .join("\n"),
    ));
    output.push_str("\n\n### Boundaries — Avoid Here\n\n");
    output.push_str(&or_none(
        &record
            .context()
            .boundaries()
            .iter()
            .map(|entry| {
                format!(
                    "- `{}` — {}{}",
                    entry.id(),
                    entry.text(),
                    entry
                        .owner()
                        .map_or_else(String::new, |owner| format!(" — owner `{owner}`"))
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
    ));
    output.push_str("\n\n### Context Rules\n\n");
    output.push_str(&or_none(
        &record
            .context()
            .ruleset()
            .rules()
            .map(|rule| format!("- `{}` — {}", rule.id(), rule.text()))
            .collect::<Vec<_>>()
            .join("\n"),
    ));
}

fn shared_attributions(
    records: &[&Record],
    nearest: &Record,
    repository: &Repository,
) -> Result<BTreeMap<RulesetId, BTreeSet<String>>, Error> {
    let mut shared = BTreeMap::<RulesetId, BTreeSet<String>>::new();
    for record in records {
        let scope = if record.context().id() == nearest.context().id() {
            "direct"
        } else {
            "inherited"
        };
        for id in record.context().ruleset().includes() {
            shared.entry(id.clone()).or_default().insert(format!(
                "{} ({scope}, direct composition)",
                record.context().id()
            ));
            for transitive in repository.shared().require(id)?.transitive() {
                shared
                    .entry(transitive.clone())
                    .or_default()
                    .insert(format!(
                        "{} ({scope}, transitive composition)",
                        record.context().id()
                    ));
            }
        }
    }
    Ok(shared)
}

fn render_shared_rulesets(
    output: &mut String,
    shared: &BTreeMap<RulesetId, BTreeSet<String>>,
    repository: &Repository,
) -> Result<(), Error> {
    output.push_str("\n\n## Effective Shared Rulesets\n\n");
    let shared_lines = shared
        .iter()
        .map(|(id, declarations)| {
            let summary = repository.shared().require(id)?;
            Ok(format!(
                "- `{}` — {} — source {} — {} Rules — digest `{}` — declared by {}",
                summary.id(),
                summary.purpose(),
                summary.source(),
                summary.rule_count(),
                summary.digest(),
                declarations.iter().cloned().collect::<Vec<_>>().join(", ")
            ))
        })
        .collect::<Result<Vec<_>, Error>>()?
        .join("\n");
    output.push_str(&or_none(&shared_lines));
    if !shared.is_empty() {
        output.push_str("\n\nUse `rapport ruleset show <RULESET_ID>` for complete shared Rules.");
    }
    Ok(())
}

fn render_required_signoffs(
    output: &mut String,
    declared: bool,
    nearest: &Record,
    repository: &Repository,
    path: &Utf8Path,
    repo_root: &Utf8Path,
) -> Result<(), Error> {
    output.push_str("\n\n## Required Build Signoffs\n\n");
    let signoff_lines = if declared {
        nearest
            .context()
            .signoffs()
            .iter()
            .map(|signoff| {
                format!(
                    "- `{}` — source `{}` — just {} — stage {} — resource {} — trigger {}",
                    signoff.id(),
                    nearest.context().id(),
                    signoff.target(),
                    signoff.stage(),
                    signoff.resource_group().unwrap_or("none"),
                    display(repo_root, nearest.directory())
                )
            })
            .collect::<Vec<_>>()
    } else {
        repository
            .applicable_signoffs(path)?
            .into_iter()
            .map(|matched| {
                format!(
                    "- `{}` — source `{}` — just {} — stage {} — resource {} — trigger {}",
                    matched.signoff.id(),
                    matched.record.context().id(),
                    matched.signoff.target(),
                    matched.signoff.stage(),
                    matched.signoff.resource_group().unwrap_or("none"),
                    matched.trigger
                )
            })
            .collect::<Vec<_>>()
    };
    output.push_str(&or_none(&signoff_lines.join("\n")));
    Ok(())
}

pub(super) fn display(root: &Utf8Path, path: &Utf8Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string()
}
pub(super) fn or_none(value: &str) -> String {
    if value.is_empty() {
        "none".to_owned()
    } else {
        value.to_owned()
    }
}
