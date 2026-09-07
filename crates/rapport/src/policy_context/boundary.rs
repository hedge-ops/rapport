//! Context TOML boundary.
//!
//! This module owns canonical Context TOML conversion; domain values own identity and semantic validation.

use super::Error;
use super::domain::{
    Boundary, ComponentRelativePath, Context, ContextDeclarations, ContextEntries, ContextId,
    DependencyName, Entry, GeneratedInput, GeneratedOutput, RepositoryPath, SCHEMA_VERSION,
};
use crate::shared_ruleset::{NewRule, Reference, Ruleset, RulesetId};
use rapport_files::Utf8Path;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextFile {
    #[serde(default = "schema_version")]
    version: u16,
    id: Option<String>,
    namespace: Option<String>,
    #[serde(rename = "type")]
    component_type: Option<String>,
    purpose: String,
    #[serde(default)]
    components: Vec<String>,
    #[serde(default)]
    generated_outputs: BTreeMap<String, GeneratedOutputFile>,
    #[serde(default)]
    generated_inputs: BTreeMap<String, GeneratedInputFile>,
    #[serde(default)]
    kustomizations: Vec<String>,
    next_ownership: Option<u16>,
    next_boundary: Option<u16>,
    #[serde(default)]
    ownership: BTreeMap<String, EntryFile>,
    #[serde(default)]
    boundaries: BTreeMap<String, BoundaryFile>,
    #[serde(default)]
    ruleset: EmbeddedRulesetFile,
}

const fn schema_version() -> u16 {
    SCHEMA_VERSION
}

fn next_entry(explicit: Option<u16>, ids: impl Iterator<Item = String>) -> Result<u16, Error> {
    if let Some(next) = explicit {
        return Ok(next);
    }
    let maximum = ids
        .filter_map(|id| {
            id.rsplit_once('_')
                .and_then(|(_, suffix)| suffix.parse::<u16>().ok())
        })
        .max()
        .unwrap_or(0);
    maximum
        .checked_add(1)
        .ok_or_else(|| Error::InvalidEntryId(maximum.to_string()))
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GeneratedOutputFile {
    tool: String,
    target: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GeneratedInputFile {
    component: String,
    output: String,
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

pub(super) fn parse(contents: &str, path: &Utf8Path) -> Result<Context, Error> {
    if uses_legacy_schema(contents) {
        return Err(Error::LegacySchema {
            path: path.to_path_buf(),
        });
    }
    reject_lifecycle_fields(contents, path)?;
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
    let namespaced = file.namespace.is_some();
    let ((Some(identity), None) | (None, Some(identity))) = (file.id, file.namespace) else {
        return Err(Error::SchemaIdentity {
            path: path.to_path_buf(),
        });
    };
    let id = ContextId::parse(identity)?;
    let entries = parse_entries(
        file.next_ownership,
        file.next_boundary,
        file.ownership,
        file.boundaries,
    )?;
    let ruleset_id = if namespaced {
        RulesetId::parse(id.as_str())?
    } else {
        id.embedded_ruleset_id()?
    };
    let declarations = parse_declarations(
        file.components,
        file.generated_outputs,
        file.generated_inputs,
        file.kustomizations,
        path,
    )?;
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
    let mut context = Context::from_parts(id, file.purpose, entries, declarations, ruleset)?;
    context.set_schema(namespaced, file.component_type)?;
    context.validate_identities()?;
    Ok(context)
}

fn parse_entries(
    next_ownership: Option<u16>,
    next_boundary: Option<u16>,
    ownership: BTreeMap<String, EntryFile>,
    boundaries: BTreeMap<String, BoundaryFile>,
) -> Result<ContextEntries, Error> {
    let next_ownership = next_entry(next_ownership, ownership.keys().cloned())?;
    let next_boundary = next_entry(next_boundary, boundaries.keys().cloned())?;
    let ownership = ownership
        .into_iter()
        .map(|(entry_id, entry)| Entry::from_parts(entry_id, entry.text))
        .collect::<Result<Vec<_>, _>>()?;
    let boundaries = boundaries
        .into_iter()
        .map(|(entry_id, entry)| {
            Ok(Boundary::from_parts(
                Entry::from_parts(entry_id, entry.text)?,
                entry.owner.map(ContextId::parse).transpose()?,
            ))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    Ok(ContextEntries {
        next_ownership,
        next_boundary,
        ownership,
        boundaries,
    })
}

fn parse_declarations(
    components: Vec<String>,
    generated_outputs: BTreeMap<String, GeneratedOutputFile>,
    generated_inputs: BTreeMap<String, GeneratedInputFile>,
    kustomizations: Vec<String>,
    path: &Utf8Path,
) -> Result<ContextDeclarations, Error> {
    let components = components
        .into_iter()
        .map(|component| RepositoryPath::try_new(component, path, "components".to_owned()))
        .collect::<Result<Vec<_>, Error>>()?;
    let generated_outputs = generated_outputs
        .into_iter()
        .map(|(name, output)| {
            let name = DependencyName::try_new(name, path, "generated_outputs".to_owned())?;
            let output = GeneratedOutput::try_new(&name, output.tool, output.target, path)?;
            Ok((name, output))
        })
        .collect::<Result<BTreeMap<_, _>, Error>>()?;
    let generated_inputs = generated_inputs
        .into_iter()
        .map(|(name, input)| {
            let input_name =
                DependencyName::try_new(name.clone(), path, "generated_inputs".to_owned())?;
            let component = RepositoryPath::try_new(
                input.component,
                path,
                format!("generated_inputs.{name}.component"),
            )?;
            let output = DependencyName::try_new(
                input.output,
                path,
                format!("generated_inputs.{name}.output"),
            )?;
            Ok((input_name, GeneratedInput::from_parts(component, output)))
        })
        .collect::<Result<BTreeMap<_, _>, Error>>()?;
    let kustomizations = kustomizations
        .into_iter()
        .map(|target| ComponentRelativePath::try_new(target, path, "kustomizations".to_owned()))
        .collect::<Result<Vec<_>, Error>>()?;
    Ok(ContextDeclarations {
        components,
        generated_outputs,
        generated_inputs,
        kustomizations,
    })
}

fn reject_lifecycle_fields(contents: &str, path: &Utf8Path) -> Result<(), Error> {
    if let Ok(value) = toml::from_str::<toml::Value>(contents) {
        for field in ["review", "signoffs"] {
            if value.get(field).is_some() {
                return Err(Error::LifecycleField {
                    path: path.to_path_buf(),
                    field,
                });
            }
        }
    }
    Ok(())
}

fn uses_legacy_schema(contents: &str) -> bool {
    let Ok(value) = toml::from_str::<toml::Value>(contents) else {
        return false;
    };
    let Some(context) = value.as_table() else {
        return false;
    };
    let legacy_ownership = context
        .get("ownership")
        .and_then(toml::Value::as_table)
        .is_some_and(|ownership| {
            ownership.get("owns").is_some_and(toml::Value::is_array)
                || ownership
                    .get("boundaries")
                    .is_some_and(toml::Value::is_array)
        });
    let legacy_signoff = context
        .get("signoffs")
        .and_then(toml::Value::as_array)
        .is_some_and(|signoffs| {
            signoffs.iter().any(|signoff| {
                signoff
                    .as_table()
                    .is_some_and(|signoff| signoff.contains_key("kind"))
            })
        });
    context.contains_key("rule_includes")
        || context.get("rules").is_some_and(toml::Value::is_array)
        || legacy_ownership
        || legacy_signoff
}

#[derive(Serialize)]
struct ContextFileRef<'context> {
    version: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<&'context str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    namespace: Option<&'context str>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    component_type: Option<&'context str>,
    purpose: &'context str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    components: Vec<&'context str>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    generated_outputs: BTreeMap<&'context str, GeneratedOutputFileRef<'context>>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    generated_inputs: BTreeMap<&'context str, GeneratedInputFileRef<'context>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    kustomizations: Vec<&'context str>,
    next_ownership: u16,
    next_boundary: u16,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    ownership: BTreeMap<&'context str, EntryFileRef<'context>>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    boundaries: BTreeMap<&'context str, BoundaryFileRef<'context>>,
    ruleset: EmbeddedRulesetFileRef<'context>,
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
struct GeneratedOutputFileRef<'context> {
    tool: &'context str,
    target: &'context str,
}

#[derive(Serialize)]
struct GeneratedInputFileRef<'context> {
    component: &'context str,
    output: &'context str,
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
        id: (!context.namespaced()).then(|| context.id().as_str()),
        namespace: context.namespaced().then(|| context.id().as_str()),
        component_type: context.component_type(),
        purpose: context.purpose(),
        components: context
            .components()
            .iter()
            .map(RepositoryPath::as_str)
            .collect(),
        generated_outputs: context
            .generated_outputs()
            .iter()
            .map(|(name, output)| {
                (
                    name.as_str(),
                    GeneratedOutputFileRef {
                        tool: output.tool(),
                        target: output.target(),
                    },
                )
            })
            .collect(),
        generated_inputs: context
            .generated_inputs()
            .iter()
            .map(|(name, input)| {
                (
                    name.as_str(),
                    GeneratedInputFileRef {
                        component: input.component().as_str(),
                        output: input.output().as_str(),
                    },
                )
            })
            .collect(),
        kustomizations: context
            .kustomizations()
            .iter()
            .map(ComponentRelativePath::as_str)
            .collect(),
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
    };
    toml_edit::ser::to_string_pretty(&file).map_err(Error::Encode)
}

#[cfg(test)]
mod tests {
    use super::{parse, render};
    use crate::policy_context::Error;
    use claims::{assert_err, assert_ok};
    use rapport_files::Utf8Path;
    use rstest::rstest;

    #[test]
    fn parse_should_identify_legacy_context_schema() {
        let legacy = r#"
version = 1
purpose = "Legacy policy."
rule_includes = ["/rules/rust.toml"]

[ownership]
owns = ["Legacy ownership."]
boundaries = ["Legacy boundary."]

[[signoffs]]
kind = "build"
target = "ci"
"#;

        let error = assert_err!(parse(legacy, Utf8Path::new("/repo/context.toml")));

        assert!(
            matches!(error, Error::LegacySchema { path } if path == Utf8Path::new("/repo/context.toml"))
        );
    }

    #[test]
    fn parse_and_render_should_preserve_component_declarations() {
        let contents = r#"
version = 1
namespace = "VIEW"
purpose = "Owns the view."
components = ["app/core/shared", "app/core/view"]
kustomizations = [".", "overlays/staging"]

[generated_outputs.facet_swift]
tool = "facet_generate"
target = "swift"

[generated_inputs.app]
component = "app/core/shared"
output = "facet_swift"

[ruleset]
includes = []
"#;

        let context = assert_ok!(parse(
            contents,
            Utf8Path::new("/repo/app/core/view/context.toml")
        ));
        let rendered = assert_ok!(render(&context));
        let document: toml::Value = assert_ok!(toml::from_str(&rendered));

        assert_eq!(
            document["components"],
            toml::Value::Array(vec![
                toml::Value::String("app/core/shared".to_owned()),
                toml::Value::String("app/core/view".to_owned()),
            ])
        );
        assert_eq!(
            document["kustomizations"],
            toml::Value::Array(vec![
                toml::Value::String(".".to_owned()),
                toml::Value::String("overlays/staging".to_owned()),
            ])
        );
        assert_eq!(
            document["generated_outputs"]["facet_swift"]["tool"].as_str(),
            Some("facet_generate")
        );
        assert_eq!(
            document["generated_outputs"]["facet_swift"]["target"].as_str(),
            Some("swift")
        );
        assert_eq!(
            document["generated_inputs"]["app"]["component"].as_str(),
            Some("app/core/shared")
        );
        assert_eq!(
            document["generated_inputs"]["app"]["output"].as_str(),
            Some("facet_swift")
        );
    }

    #[rstest]
    #[case::absolute_component(
        "components = ['/components/shared']",
        "components",
        "/components/shared"
    )]
    #[case::parent_kustomization(
        "kustomizations = ['../overlays/staging']",
        "kustomizations",
        "../overlays/staging"
    )]
    #[case::parent_producer(
        "[generated_inputs.app]\ncomponent = '../shared'\noutput = 'facet_swift'",
        "generated_inputs.app.component",
        "../shared"
    )]
    fn parse_should_reject_non_relative_declaration_paths(
        #[case] declaration: &str,
        #[case] field: &str,
        #[case] value: &str,
    ) {
        let contents = format!(
            "version = 1\nnamespace = 'VIEW'\npurpose = 'Owns the view.'\n{declaration}\n[ruleset]\nincludes = []"
        );

        let error = assert_err!(parse(&contents, Utf8Path::new("/repo/context.toml")));

        assert!(matches!(
            error,
            Error::InvalidRelativePath {
                path,
                field: actual_field,
                value: actual_value,
            } if path == Utf8Path::new("/repo/context.toml")
                && actual_field == field
                && actual_value == value
        ));
    }

    #[rstest]
    #[case::uppercase_output(
        "[generated_outputs.FacetSwift]\ntool = 'facet_generate'\ntarget = 'swift'"
    )]
    #[case::uppercase_input(
        "[generated_inputs.App]\ncomponent = 'app/core/shared'\noutput = 'facet_swift'"
    )]
    fn parse_should_reject_noncanonical_dependency_names(#[case] declaration: &str) {
        let contents = format!(
            "version = 1\nnamespace = 'VIEW'\npurpose = 'Owns the view.'\n{declaration}\n[ruleset]\nincludes = []"
        );

        let error = assert_err!(parse(&contents, Utf8Path::new("/repo/context.toml")));

        assert!(
            matches!(error, Error::InvalidDependencyName { path, .. } if path == Utf8Path::new("/repo/context.toml"))
        );
    }

    #[rstest]
    #[case::empty_tool("", "swift", "tool")]
    #[case::empty_target("facet_generate", "", "target")]
    fn parse_should_require_generated_output_fields(
        #[case] tool: &str,
        #[case] target: &str,
        #[case] field: &str,
    ) {
        let contents = format!(
            "version = 1\nnamespace = 'VIEW'\npurpose = 'Owns the view.'\n[generated_outputs.facet_swift]\ntool = '{tool}'\ntarget = '{target}'\n[ruleset]\nincludes = []"
        );

        let error = assert_err!(parse(&contents, Utf8Path::new("/repo/context.toml")));

        assert!(matches!(
            error,
            Error::EmptyGeneratedOutputField {
                path,
                output,
                field: actual_field,
            } if path == Utf8Path::new("/repo/context.toml")
                && output == "facet_swift"
                && actual_field == field
        ));
    }
}
