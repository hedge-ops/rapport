//! TOML boundary for shared Rulesets.
//!
//! This module owns canonical TOML decoding and rendering; domain validation remains with Ruleset values.

use super::Error;
use super::domain::{NewRule, Ruleset, SCHEMA_VERSION};
use rapport_files::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RulesetFile {
    version: u16,
    id: String,
    purpose: Option<String>,
    catalog_version: Option<String>,
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

pub(super) fn parse_repository(contents: &str, path: &Utf8Path) -> Result<Ruleset, Error> {
    let mut file = decode(contents, path)?;
    if file.catalog_version.is_some() {
        return Err(Error::CatalogOwned(file.id));
    }
    let purpose = file.purpose.take().ok_or(Error::EmptyText {
        field: "Ruleset purpose",
    })?;
    into_domain(file, purpose, None)
}

pub(super) fn parse_catalog(
    contents: &str,
    path: &Utf8Path,
    purpose: &str,
    catalog_version: &str,
) -> Result<Ruleset, Error> {
    let file = decode(contents, path)?;
    into_domain(file, purpose.to_owned(), Some(catalog_version.to_owned()))
}

fn decode(contents: &str, path: &Utf8Path) -> Result<RulesetFile, Error> {
    let file: RulesetFile = toml::from_str(contents).map_err(|source| Error::Decode {
        path: path.to_path_buf(),
        source,
    })?;
    if file.version != SCHEMA_VERSION {
        return Err(Error::SchemaVersion {
            path: path.to_path_buf(),
            version: file.version,
        });
    }
    Ok(file)
}

fn into_domain(
    file: RulesetFile,
    purpose: String,
    catalog_version: Option<String>,
) -> Result<Ruleset, Error> {
    let rules = file
        .rules
        .into_iter()
        .map(|(id, rule)| NewRule {
            id,
            text: rule.text,
            rationale: rule.rationale,
            avoid_example: rule.avoid.text,
            avoid_language: rule.avoid.language,
            prefer_example: rule.prefer.text,
            prefer_language: rule.prefer.language,
            reference: rule.reference,
        })
        .collect();
    Ruleset::try_new(file.id, purpose, catalog_version, file.includes, rules)
}

#[derive(Serialize)]
struct RulesetFileRef<'ruleset> {
    version: u16,
    id: &'ruleset str,
    purpose: &'ruleset str,
    #[serde(skip_serializing_if = "Option::is_none")]
    catalog_version: Option<&'ruleset str>,
    includes: Vec<&'ruleset str>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    rules: BTreeMap<&'ruleset str, RuleFileRef<'ruleset>>,
}

#[derive(Serialize)]
struct RuleFileRef<'ruleset> {
    text: &'ruleset str,
    rationale: &'ruleset str,
    avoid: ExampleFileRef<'ruleset>,
    prefer: ExampleFileRef<'ruleset>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reference: Option<String>,
}

#[derive(Serialize)]
struct ExampleFileRef<'ruleset> {
    language: &'ruleset str,
    text: &'ruleset str,
}

pub(super) fn render(ruleset: &Ruleset) -> Result<String, Error> {
    let rules = ruleset
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
                    reference: rule.reference().map(super::domain::Reference::markdown),
                },
            )
        })
        .collect();
    let file = RulesetFileRef {
        version: SCHEMA_VERSION,
        id: ruleset.id().as_str(),
        purpose: ruleset.purpose(),
        catalog_version: ruleset.catalog_version(),
        includes: ruleset
            .includes()
            .iter()
            .map(super::domain::RulesetId::as_str)
            .collect(),
        rules,
    };
    toml_edit::ser::to_string_pretty(&file).map_err(Error::Encode)
}

pub(super) fn path_for_repository_ruleset(root: &Utf8Path, ruleset: &Ruleset) -> Utf8PathBuf {
    root.join(".rapport/rules")
        .join(ruleset.id().conventional_path())
}

#[cfg(test)]
mod tests {
    use super::{parse_catalog, parse_repository, render};
    use crate::shared_ruleset::Error;
    use claims::{assert_err, assert_ok};
    use pretty_assertions::assert_eq;

    #[test]
    fn render_should_round_trip_a_complete_repository_ruleset() {
        let input = r#"
version = 1
id = "CODE"
purpose = "Shared coding expectations."
includes = []

[rules.CODE_001]
text = "Prefer explicit names."
rationale = "Names carry intent."
reference = "[Naming](https://example.com/naming)"

[rules.CODE_001.avoid]
language = "rust"
text = "let x = value;"

[rules.CODE_001.prefer]
language = "rust"
text = "let person_count = value;"
"#;
        let ruleset = assert_ok!(parse_repository(
            input,
            "/repo/.rapport/rules/code.toml".into()
        ));
        let rendered = assert_ok!(render(&ruleset));
        let round_trip = assert_ok!(parse_repository(
            &rendered,
            "/repo/.rapport/rules/code.toml".into()
        ));

        assert_eq!(
            round_trip, ruleset,
            "expecting canonical TOML to preserve the complete Ruleset"
        );
    }

    #[test]
    fn parse_catalog_should_reject_rule_without_authored_rationale() {
        let input = r#"
version = 1
id = "CODE"
includes = []

[rules.CODE_001]
text = "Prefer explicit names."

[rules.CODE_001.avoid]
language = "rust"
text = "let x = value;"

[rules.CODE_001.prefer]
language = "rust"
text = "let person_count = value;"
"#;

        let error = assert_err!(parse_catalog(
            input,
            "/catalog/code.toml".into(),
            "Shared coding expectations.",
            "1.0.0",
        ));

        let Error::Decode { source, .. } = error else {
            panic!("expecting a catalog decode error for a missing rationale");
        };
        assert!(source.to_string().contains("missing field `rationale`"));
    }
}
