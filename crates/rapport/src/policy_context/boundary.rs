//! Context TOML boundary.
//!
//! This module owns canonical Context TOML conversion; domain values own identity and semantic validation.

use super::Error;
use super::domain::{Boundary, BuildSignoff, Context, ContextId, Entry, Grade, SCHEMA_VERSION};
use crate::shared_ruleset::{NewRule, Reference, Ruleset, RulesetId};
use rapport_files::Utf8Path;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::str::FromStr;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextFile {
    version: u16,
    id: String,
    purpose: String,
    #[serde(default = "one")]
    next_ownership: u16,
    #[serde(default = "one")]
    next_boundary: u16,
    #[serde(default)]
    ownership: BTreeMap<String, EntryFile>,
    #[serde(default)]
    boundaries: BTreeMap<String, BoundaryFile>,
    #[serde(default)]
    ruleset: EmbeddedRulesetFile,
    review: Option<ReviewFile>,
    #[serde(default)]
    signoffs: Vec<SignoffFile>,
}

const fn one() -> u16 {
    1
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryFile {
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BoundaryFile {
    text: String,
    owner: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct EmbeddedRulesetFile {
    #[serde(default)]
    includes: Vec<String>,
    #[serde(default)]
    rules: BTreeMap<String, RuleFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleFile {
    text: String,
    rationale: String,
    avoid: ExampleFile,
    prefer: ExampleFile,
    reference: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExampleFile {
    language: String,
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewFile {
    minimum_grade: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SignoffFile {
    id: String,
    target: String,
    #[serde(default)]
    stage: u32,
    resource_group: Option<String>,
    #[serde(default)]
    include: Vec<String>,
}

pub(super) fn parse(contents: &str, path: &Utf8Path) -> Result<Context, Error> {
    let file: ContextFile = toml::from_str(contents).map_err(|source| Error::Decode {
        path: path.to_path_buf(),
        source,
    })?;
    if file.version != SCHEMA_VERSION {
        return Err(Error::SchemaVersion {
            path: path.to_path_buf(),
            version: file.version,
        });
    }
    let id = ContextId::parse(file.id)?;
    let ownership = file
        .ownership
        .into_iter()
        .map(|(entry_id, entry)| Entry::from_parts(entry_id, entry.text))
        .collect::<Result<Vec<_>, _>>()?;
    let boundaries = file
        .boundaries
        .into_iter()
        .map(|(entry_id, entry)| {
            Ok(Boundary::from_parts(
                Entry::from_parts(entry_id, entry.text)?,
                entry.owner.map(ContextId::parse).transpose()?,
            ))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let ruleset_id = id.embedded_ruleset_id()?;
    let rules = file
        .ruleset
        .rules
        .into_iter()
        .map(|(rule_id, rule)| NewRule {
            id: rule_id,
            text: rule.text,
            rationale: rule.rationale,
            avoid_example: rule.avoid.text,
            avoid_language: rule.avoid.language,
            prefer_example: rule.prefer.text,
            prefer_language: rule.prefer.language,
            reference: rule.reference,
        })
        .collect();
    let ruleset = Ruleset::try_new(
        ruleset_id.to_string(),
        "Context-owned architectural Rules.",
        None,
        file.ruleset.includes,
        rules,
    )?;
    let minimum_grade = file
        .review
        .map(|review| Grade::from_str(&review.minimum_grade))
        .transpose()?;
    let signoffs = file
        .signoffs
        .into_iter()
        .map(|signoff| {
            let candidate = BuildSignoff::try_new(
                &id,
                signoff.target.clone(),
                signoff.stage,
                signoff.resource_group.clone(),
                signoff.include.clone(),
            )?;
            if candidate.id() != signoff.id {
                return Err(Error::MissingSignoff(signoff.id));
            }
            Ok(BuildSignoff::from_parts(
                signoff.id,
                signoff.target,
                signoff.stage,
                signoff.resource_group,
                signoff.include,
            ))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    Context::from_parts(
        id,
        file.purpose,
        file.next_ownership,
        file.next_boundary,
        ownership,
        boundaries,
        ruleset,
        minimum_grade,
        signoffs,
    )
}

#[derive(Serialize)]
struct ContextFileRef<'context> {
    version: u16,
    id: &'context str,
    purpose: &'context str,
    next_ownership: u16,
    next_boundary: u16,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    ownership: BTreeMap<&'context str, EntryFileRef<'context>>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    boundaries: BTreeMap<&'context str, BoundaryFileRef<'context>>,
    ruleset: EmbeddedRulesetFileRef<'context>,
    #[serde(skip_serializing_if = "Option::is_none")]
    review: Option<ReviewFileRef>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    signoffs: Vec<SignoffFileRef<'context>>,
}

#[derive(Serialize)]
struct EntryFileRef<'context> {
    text: &'context str,
}

#[derive(Serialize)]
struct BoundaryFileRef<'context> {
    text: &'context str,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner: Option<&'context str>,
}

#[derive(Serialize)]
struct EmbeddedRulesetFileRef<'context> {
    includes: Vec<&'context str>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    rules: BTreeMap<&'context str, RuleFileRef<'context>>,
}

#[derive(Serialize)]
struct RuleFileRef<'context> {
    text: &'context str,
    rationale: &'context str,
    avoid: ExampleFileRef<'context>,
    prefer: ExampleFileRef<'context>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reference: Option<String>,
}

#[derive(Serialize)]
struct ExampleFileRef<'context> {
    language: &'context str,
    text: &'context str,
}

#[derive(Serialize)]
struct ReviewFileRef {
    minimum_grade: String,
}

#[derive(Serialize)]
struct SignoffFileRef<'context> {
    id: &'context str,
    target: &'context str,
    stage: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    resource_group: Option<&'context str>,
    include: &'context [String],
}

pub(super) fn render(context: &Context) -> Result<String, Error> {
    let ownership = context
        .ownership()
        .iter()
        .map(|entry| (entry.id(), EntryFileRef { text: entry.text() }))
        .collect();
    let boundaries = context
        .boundaries()
        .iter()
        .map(|entry| {
            (
                entry.id(),
                BoundaryFileRef {
                    text: entry.text(),
                    owner: entry.owner().map(ContextId::as_str),
                },
            )
        })
        .collect();
    let rules = context
        .ruleset()
        .rules()
        .map(|rule| {
            (
                rule.id().as_str(),
                RuleFileRef {
                    text: rule.text(),
                    rationale: rule.rationale(),
                    avoid: ExampleFileRef {
                        language: rule.avoid().language().as_str(),
                        text: rule.avoid().text(),
                    },
                    prefer: ExampleFileRef {
                        language: rule.prefer().language().as_str(),
                        text: rule.prefer().text(),
                    },
                    reference: rule.reference().map(Reference::markdown),
                },
            )
        })
        .collect();
    let file = ContextFileRef {
        version: SCHEMA_VERSION,
        id: context.id().as_str(),
        purpose: context.purpose(),
        next_ownership: context.next_ownership(),
        next_boundary: context.next_boundary(),
        ownership,
        boundaries,
        ruleset: EmbeddedRulesetFileRef {
            includes: context
                .ruleset()
                .includes()
                .iter()
                .map(RulesetId::as_str)
                .collect(),
            rules,
        },
        review: context.minimum_grade().map(|grade| ReviewFileRef {
            minimum_grade: grade.to_string(),
        }),
        signoffs: context
            .signoffs()
            .iter()
            .map(|signoff| SignoffFileRef {
                id: signoff.id(),
                target: signoff.target(),
                stage: signoff.stage(),
                resource_group: signoff.resource_group(),
                include: signoff.included_paths(),
            })
            .collect(),
    };
    toml_edit::ser::to_string_pretty(&file).map_err(Error::Encode)
}
