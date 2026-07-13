//! Shared Ruleset domain values.
//!
//! Owns validated identities, Rules, examples, references, and composition.
//! Serialization and repository layout remain boundary concerns.

use super::Error;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

pub(crate) const SCHEMA_VERSION: u16 = 1;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::Display)]
#[display("{_0}")]
pub(crate) struct RulesetId(String);

impl RulesetId {
    pub(crate) fn parse(value: impl Into<String>) -> Result<Self, Error> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.split('_').all(|part| {
                !part.is_empty()
                    && part
                        .bytes()
                        .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
            })
            && value.as_bytes().first().is_some_and(u8::is_ascii_uppercase);
        if !valid {
            return Err(Error::InvalidRulesetId);
        }
        Ok(Self(value))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn conventional_path(&self) -> String {
        format!("{}.toml", self.0.to_ascii_lowercase().replace('_', "/"))
    }
}

impl fmt::Debug for RulesetId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("RulesetId").field(&self.0).finish()
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::Display)]
#[display("{_0}")]
pub(crate) struct RuleId(String);

impl RuleId {
    pub(crate) fn parse(value: impl Into<String>, owner: &RulesetId) -> Result<Self, Error> {
        let value = value.into();
        let suffix = value
            .strip_prefix(owner.as_str())
            .and_then(|value| value.strip_prefix('_'));
        let valid = suffix.is_some_and(|suffix| {
            suffix.len() == 3 && suffix.bytes().all(|byte| byte.is_ascii_digit())
        });
        if !valid {
            return Err(Error::InvalidRuleId {
                ruleset: owner.to_string(),
            });
        }
        Ok(Self(value))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for RuleId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("RuleId").field(&self.0).finish()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExampleLanguage {
    Rust,
    Swift,
    Kotlin,
    Csharp,
    Xaml,
    Html,
    Javascript,
    Typescript,
    Toml,
    Json,
    Markdown,
    Shell,
    Text,
}

impl FromStr for ExampleLanguage {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "rust" => Ok(Self::Rust),
            "swift" => Ok(Self::Swift),
            "kotlin" => Ok(Self::Kotlin),
            "csharp" => Ok(Self::Csharp),
            "xaml" => Ok(Self::Xaml),
            "html" => Ok(Self::Html),
            "javascript" => Ok(Self::Javascript),
            "typescript" => Ok(Self::Typescript),
            "toml" => Ok(Self::Toml),
            "json" => Ok(Self::Json),
            "markdown" => Ok(Self::Markdown),
            "shell" => Ok(Self::Shell),
            "text" => Ok(Self::Text),
            _ => Err(Error::UnsupportedLanguage),
        }
    }
}

impl fmt::Debug for ExampleLanguage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl ExampleLanguage {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Swift => "swift",
            Self::Kotlin => "kotlin",
            Self::Csharp => "csharp",
            Self::Xaml => "xaml",
            Self::Html => "html",
            Self::Javascript => "javascript",
            Self::Typescript => "typescript",
            Self::Toml => "toml",
            Self::Json => "json",
            Self::Markdown => "markdown",
            Self::Shell => "shell",
            Self::Text => "text",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Example {
    language: ExampleLanguage,
    text: String,
}

impl Example {
    pub(crate) fn try_new(language: &str, text: impl Into<String>) -> Result<Self, Error> {
        Ok(Self {
            language: language.parse()?,
            text: required_text("example", text)?,
        })
    }

    pub(crate) fn language(&self) -> ExampleLanguage {
        self.language
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }
}

impl fmt::Debug for Example {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Example")
            .field("language", &self.language)
            .field("text_length", &self.text.len())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Reference {
    label: String,
    target: String,
}

impl Reference {
    pub(crate) fn parse(value: impl Into<String>) -> Result<Self, Error> {
        let value = value.into();
        let Some((label, target)) = value
            .strip_prefix('[')
            .and_then(|value| value.split_once("]("))
            .and_then(|(label, target)| target.strip_suffix(')').map(|target| (label, target)))
        else {
            return Err(Error::InvalidReference);
        };
        if label.trim().is_empty() || target.trim().is_empty() || target.contains(')') {
            return Err(Error::InvalidReference);
        }
        Ok(Self {
            label: label.trim().to_owned(),
            target: target.trim().to_owned(),
        })
    }

    pub(crate) fn markdown(&self) -> String {
        format!("[{}]({})", self.label, self.target)
    }
}

impl fmt::Debug for Reference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Reference")
            .field("label_length", &self.label.len())
            .field("target_length", &self.target.len())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Rule {
    id: RuleId,
    text: String,
    rationale: String,
    avoid: Example,
    prefer: Example,
    reference: Option<Reference>,
}

impl Rule {
    pub(crate) fn try_new(owner: &RulesetId, input: NewRule) -> Result<Self, Error> {
        Ok(Self {
            id: RuleId::parse(input.id, owner)?,
            text: required_text("Rule text", input.text)?,
            rationale: required_text("Rule rationale", input.rationale)?,
            avoid: Example::try_new(&input.avoid_language, input.avoid_example)?,
            prefer: Example::try_new(&input.prefer_language, input.prefer_example)?,
            reference: input.reference.map(Reference::parse).transpose()?,
        })
    }

    pub(crate) fn id(&self) -> &RuleId {
        &self.id
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn rationale(&self) -> &str {
        &self.rationale
    }

    pub(crate) fn avoid(&self) -> &Example {
        &self.avoid
    }

    pub(crate) fn prefer(&self) -> &Example {
        &self.prefer
    }

    pub(crate) fn reference(&self) -> Option<&Reference> {
        self.reference.as_ref()
    }

    pub(crate) fn update(&mut self, update: RuleUpdate) -> Result<(), Error> {
        if let Some(text) = update.text {
            self.text = required_text("Rule text", text)?;
        }
        if let Some(rationale) = update.rationale {
            self.rationale = required_text("Rule rationale", rationale)?;
        }
        if let Some(example) = update.avoid {
            self.avoid = Example::try_new(&example.language, example.text)?;
        }
        if let Some(example) = update.prefer {
            self.prefer = Example::try_new(&example.language, example.text)?;
        }
        match update.reference {
            ReferenceUpdate::Preserve => {}
            ReferenceUpdate::Set(reference) => {
                self.reference = Some(Reference::parse(reference)?);
            }
            ReferenceUpdate::Clear => self.reference = None,
        }
        Ok(())
    }
}

impl fmt::Debug for Rule {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Rule")
            .field("id", &self.id)
            .field("text_length", &self.text.len())
            .field("rationale_length", &self.rationale.len())
            .field("avoid", &self.avoid)
            .field("prefer", &self.prefer)
            .field("has_reference", &self.reference.is_some())
            .finish()
    }
}

pub(crate) struct NewRule {
    pub(crate) id: String,
    pub(crate) text: String,
    pub(crate) rationale: String,
    pub(crate) avoid_example: String,
    pub(crate) avoid_language: String,
    pub(crate) prefer_example: String,
    pub(crate) prefer_language: String,
    pub(crate) reference: Option<String>,
}

pub(crate) struct ExampleUpdate {
    pub(crate) language: String,
    pub(crate) text: String,
}

pub(crate) enum ReferenceUpdate {
    Preserve,
    Set(String),
    Clear,
}

pub(crate) struct RuleUpdate {
    pub(crate) text: Option<String>,
    pub(crate) rationale: Option<String>,
    pub(crate) avoid: Option<ExampleUpdate>,
    pub(crate) prefer: Option<ExampleUpdate>,
    pub(crate) reference: ReferenceUpdate,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Ruleset {
    id: RulesetId,
    purpose: String,
    catalog_version: Option<String>,
    includes: Vec<RulesetId>,
    rules: BTreeMap<RuleId, Rule>,
}

impl Ruleset {
    pub(crate) fn try_new(
        id: impl Into<String>,
        purpose: impl Into<String>,
        catalog_version: Option<String>,
        includes: Vec<String>,
        rules: Vec<NewRule>,
    ) -> Result<Self, Error> {
        let id = RulesetId::parse(id)?;
        let mut parsed_rules = BTreeMap::new();
        for rule in rules {
            let rule = Rule::try_new(&id, rule)?;
            let rule_id = rule.id().to_string();
            if parsed_rules.insert(rule.id.clone(), rule).is_some() {
                return Err(Error::DuplicateRule(rule_id));
            }
        }
        let includes = includes
            .into_iter()
            .map(RulesetId::parse)
            .collect::<Result<Vec<_>, _>>()?;
        if includes.iter().collect::<BTreeSet<_>>().len() != includes.len() {
            return Err(Error::DuplicateRuleset(id.to_string()));
        }
        Ok(Self {
            id,
            purpose: required_text("Ruleset purpose", purpose)?,
            catalog_version,
            includes,
            rules: parsed_rules,
        })
    }

    pub(crate) fn id(&self) -> &RulesetId {
        &self.id
    }

    pub(crate) fn purpose(&self) -> &str {
        &self.purpose
    }

    pub(crate) fn set_purpose(&mut self, purpose: impl Into<String>) -> Result<(), Error> {
        self.purpose = required_text("Ruleset purpose", purpose)?;
        Ok(())
    }

    pub(crate) fn catalog_version(&self) -> Option<&str> {
        self.catalog_version.as_deref()
    }

    pub(crate) fn includes(&self) -> &[RulesetId] {
        &self.includes
    }

    pub(crate) fn rules(&self) -> impl Iterator<Item = &Rule> {
        self.rules.values()
    }

    pub(crate) fn compose(&mut self, included: RulesetId) {
        if !self.includes.contains(&included) {
            self.includes.push(included);
            self.includes.sort();
        }
    }

    pub(crate) fn uncompose(&mut self, included: &RulesetId) -> bool {
        let previous = self.includes.len();
        self.includes.retain(|candidate| candidate != included);
        previous != self.includes.len()
    }

    pub(crate) fn add_rule(&mut self, input: NewRule) -> Result<(), Error> {
        let rule = Rule::try_new(&self.id, input)?;
        if self.rules.contains_key(rule.id()) {
            return Err(Error::DuplicateRule(rule.id().to_string()));
        }
        self.rules.insert(rule.id.clone(), rule);
        Ok(())
    }

    pub(crate) fn update_rule(&mut self, id: &str, update: RuleUpdate) -> Result<(), Error> {
        let rule = self
            .rules
            .get_mut(&RuleId::parse(id, &self.id)?)
            .ok_or_else(|| Error::UnknownRule(id.to_owned()))?;
        rule.update(update)
    }

    pub(crate) fn remove_rule(&mut self, id: &str) -> Result<(), Error> {
        let id = RuleId::parse(id, &self.id)?;
        if self.rules.remove(&id).is_none() {
            return Err(Error::UnknownRule(id.to_string()));
        }
        Ok(())
    }
}

impl fmt::Debug for Ruleset {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Ruleset")
            .field("id", &self.id)
            .field("purpose_length", &self.purpose.len())
            .field("catalog_version", &self.catalog_version)
            .field("include_count", &self.includes.len())
            .field("rule_count", &self.rules.len())
            .finish()
    }
}

fn required_text(field: &'static str, value: impl Into<String>) -> Result<String, Error> {
    let value = value.into();
    if value.trim().is_empty() {
        Err(Error::EmptyText { field })
    } else {
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::{NewRule, Rule, RulesetId};
    use claims::{assert_err, assert_ok};
    use rstest::rstest;

    #[rstest]
    #[case::lowercase("rust_crate")]
    #[case::empty("")]
    #[case::double_separator("RUST__CRATE")]
    #[case::leading_digit("1_RUST")]
    fn ruleset_id_parse_should_reject_noncanonical_identifiers(#[case] value: &str) {
        assert_err!(RulesetId::parse(value));
    }

    #[test]
    fn rule_try_new_should_require_owner_namespace_and_complete_examples() {
        let owner = assert_ok!(RulesetId::parse("RUST_CRATE"));
        let rule = assert_ok!(Rule::try_new(
            &owner,
            NewRule {
                id: "RUST_CRATE_001".to_owned(),
                text: "Use a library crate.".to_owned(),
                rationale: "It keeps behavior reusable.".to_owned(),
                avoid_example: "fn main() {}".to_owned(),
                avoid_language: "rust".to_owned(),
                prefer_example: "pub fn run() {}".to_owned(),
                prefer_language: "rust".to_owned(),
                reference: Some("[Rust Book](https://doc.rust-lang.org/book/)".to_owned()),
            }
        ));

        assert_eq!(rule.id().as_str(), "RUST_CRATE_001");
        assert_eq!(
            rule.reference().map(super::Reference::markdown),
            Some("[Rust Book](https://doc.rust-lang.org/book/)".to_owned())
        );
    }
}
