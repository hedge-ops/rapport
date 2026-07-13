//! Context policy domain.
//!
//! Owns stable identities, architectural semantics, embedded Rules, Review
//! quality, and Build signoff declarations. Persistence and workflows remain
//! boundary concerns.

use super::Error;
use crate::shared_ruleset::{NewRule, RuleUpdate, Ruleset, RulesetId};
use rapport_command::ResourceKey;
use rapport_files::Utf8Path;
use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

pub(crate) const SCHEMA_VERSION: u16 = 1;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::Display)]
#[display("{_0}")]
pub(crate) struct ContextId(String);

impl ContextId {
    pub(crate) fn derive(path: &Utf8Path) -> Result<Self, Error> {
        if path.as_str().is_empty() || path == Utf8Path::new(".") {
            return Ok(Self("ROOT".to_owned()));
        }
        let value = path
            .components()
            .map(|component| {
                let value = component.as_str();
                let leading_dots = value.bytes().take_while(|byte| *byte == b'.').count();
                let mut id = "DOT_".repeat(leading_dots);
                id.push_str(
                    value[leading_dots..]
                        .chars()
                        .map(|character| {
                            if character.is_ascii_alphanumeric() {
                                character.to_ascii_uppercase()
                            } else {
                                '_'
                            }
                        })
                        .collect::<String>()
                        .trim_matches('_'),
                );
                id
            })
            .collect::<Vec<_>>()
            .join("_");
        Self::parse(value)
    }

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
            return Err(Error::InvalidContextId);
        }
        Ok(Self(value))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    fn entry_id(&self, kind: &str, number: u16) -> String {
        format!("{}_{kind}_{number:03}", self.0)
    }

    pub(crate) fn embedded_ruleset_id(&self) -> Result<RulesetId, Error> {
        RulesetId::parse(format!("{}_RULE", self.0)).map_err(Into::into)
    }
}

impl fmt::Debug for ContextId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("ContextId").field(&self.0).finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    id: String,
    text: String,
}

impl Entry {
    pub(crate) fn from_parts(id: String, text: String) -> Result<Self, Error> {
        Ok(Self {
            id,
            text: required(text)?,
        })
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn set_text(&mut self, text: String) -> Result<(), Error> {
        self.text = required(text)?;
        Ok(())
    }
}

impl fmt::Debug for Entry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextEntry")
            .field("id", &self.id)
            .field("text", &self.text)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Boundary {
    entry: Entry,
    owner: Option<ContextId>,
}

impl Boundary {
    pub(crate) fn from_parts(entry: Entry, owner: Option<ContextId>) -> Self {
        Self { entry, owner }
    }

    pub(crate) fn id(&self) -> &str {
        self.entry.id()
    }

    pub(crate) fn text(&self) -> &str {
        self.entry.text()
    }

    pub(crate) fn owner(&self) -> Option<&ContextId> {
        self.owner.as_ref()
    }

    pub(crate) fn update(
        &mut self,
        text: Option<String>,
        owner: BoundaryOwnerUpdate,
    ) -> Result<(), Error> {
        if let Some(text) = text {
            self.entry.set_text(text)?;
        }
        match owner {
            BoundaryOwnerUpdate::Preserve => {}
            BoundaryOwnerUpdate::Set(owner) => self.owner = Some(owner),
            BoundaryOwnerUpdate::Clear => self.owner = None,
        }
        Ok(())
    }
}

impl fmt::Debug for Boundary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Boundary")
            .field("entry", &self.entry)
            .field("owner", &self.owner)
            .finish()
    }
}

pub(crate) enum BoundaryOwnerUpdate {
    Preserve,
    Set(ContextId),
    Clear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, derive_more::Display)]
pub(crate) enum Grade {
    #[display("F")]
    F,
    #[display("D-")]
    DMinus,
    #[display("D")]
    D,
    #[display("D+")]
    DPlus,
    #[display("C-")]
    CMinus,
    #[display("C")]
    C,
    #[display("C+")]
    CPlus,
    #[display("B-")]
    BMinus,
    #[display("B")]
    B,
    #[display("B+")]
    BPlus,
    #[display("A-")]
    AMinus,
    #[display("A")]
    A,
    #[display("A+")]
    APlus,
}

impl Grade {
    pub(crate) const DEFAULT: Self = Self::B;
}

impl FromStr for Grade {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "A+" => Ok(Self::APlus),
            "A" => Ok(Self::A),
            "A-" => Ok(Self::AMinus),
            "B+" => Ok(Self::BPlus),
            "B" => Ok(Self::B),
            "B-" => Ok(Self::BMinus),
            "C+" => Ok(Self::CPlus),
            "C" => Ok(Self::C),
            "C-" => Ok(Self::CMinus),
            "D+" => Ok(Self::DPlus),
            "D" => Ok(Self::D),
            "D-" => Ok(Self::DMinus),
            "F" => Ok(Self::F),
            _ => Err(Error::InvalidGrade),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct BuildSignoff {
    id: String,
    target: String,
    stage: u32,
    resource_group: Option<String>,
    included_paths: Vec<String>,
}

impl BuildSignoff {
    pub(crate) fn try_new(
        context: &ContextId,
        target: String,
        stage: u32,
        resource_group: Option<String>,
        included_paths: Vec<String>,
    ) -> Result<Self, Error> {
        if target.is_empty()
            || !target
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
        {
            return Err(Error::InvalidTarget);
        }
        if let Some(group) = &resource_group {
            ResourceKey::new(group).map_err(|_| Error::InvalidResourceGroup)?;
        }
        let target_id = target
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character.to_ascii_uppercase()
                } else {
                    '_'
                }
            })
            .collect::<String>();
        Ok(Self {
            id: format!("{}_SIGNOFF_{target_id}", context.as_str()),
            target,
            stage,
            resource_group,
            included_paths,
        })
    }

    pub(crate) fn from_parts(
        id: String,
        target: String,
        stage: u32,
        resource_group: Option<String>,
        included_paths: Vec<String>,
    ) -> Self {
        Self {
            id,
            target,
            stage,
            resource_group,
            included_paths,
        }
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn target(&self) -> &str {
        &self.target
    }

    pub(crate) fn stage(&self) -> u32 {
        self.stage
    }

    pub(crate) fn resource_group(&self) -> Option<&str> {
        self.resource_group.as_deref()
    }

    pub(crate) fn included_paths(&self) -> &[String] {
        &self.included_paths
    }

    pub(crate) fn add_path(&mut self, path: String) -> Result<(), Error> {
        if self.included_paths.contains(&path) {
            return Err(Error::InvalidIncludedPath);
        }
        self.included_paths.push(path);
        self.included_paths.sort();
        Ok(())
    }

    pub(crate) fn remove_path(&mut self, path: &str) -> Result<(), Error> {
        let before = self.included_paths.len();
        self.included_paths.retain(|candidate| candidate != path);
        if before == self.included_paths.len() {
            return Err(Error::InvalidIncludedPath);
        }
        Ok(())
    }
}

impl fmt::Debug for BuildSignoff {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BuildSignoff")
            .field("id", &self.id)
            .field("target", &self.target)
            .field("stage", &self.stage)
            .field("resource_group", &self.resource_group)
            .field("included_path_count", &self.included_paths.len())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Context {
    id: ContextId,
    purpose: String,
    next_ownership: u16,
    next_boundary: u16,
    ownership: Vec<Entry>,
    boundaries: Vec<Boundary>,
    ruleset: Ruleset,
    minimum_grade: Option<Grade>,
    signoffs: Vec<BuildSignoff>,
}

impl Context {
    pub(crate) fn new(id: ContextId, purpose: String) -> Result<Self, Error> {
        let ruleset_id = id.embedded_ruleset_id()?;
        Ok(Self {
            id,
            purpose: required(purpose)?,
            next_ownership: 1,
            next_boundary: 1,
            ownership: Vec::new(),
            boundaries: Vec::new(),
            ruleset: Ruleset::try_new(
                ruleset_id.to_string(),
                "Context-owned architectural Rules.",
                None,
                Vec::new(),
                Vec::new(),
            )?,
            minimum_grade: None,
            signoffs: Vec::new(),
        })
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the constructor validates the complete versioned Context boundary record"
    )]
    pub(crate) fn from_parts(
        id: ContextId,
        purpose: String,
        next_ownership: u16,
        next_boundary: u16,
        ownership: Vec<Entry>,
        boundaries: Vec<Boundary>,
        ruleset: Ruleset,
        minimum_grade: Option<Grade>,
        signoffs: Vec<BuildSignoff>,
    ) -> Result<Self, Error> {
        Ok(Self {
            id,
            purpose: required(purpose)?,
            next_ownership,
            next_boundary,
            ownership,
            boundaries,
            ruleset,
            minimum_grade,
            signoffs,
        })
    }

    pub(crate) fn id(&self) -> &ContextId {
        &self.id
    }
    pub(crate) fn purpose(&self) -> &str {
        &self.purpose
    }
    pub(crate) fn set_purpose(&mut self, value: String) -> Result<(), Error> {
        self.purpose = required(value)?;
        Ok(())
    }
    pub(crate) fn ownership(&self) -> &[Entry] {
        &self.ownership
    }
    pub(crate) fn boundaries(&self) -> &[Boundary] {
        &self.boundaries
    }
    pub(crate) fn ruleset(&self) -> &Ruleset {
        &self.ruleset
    }
    pub(crate) fn ruleset_mut(&mut self) -> &mut Ruleset {
        &mut self.ruleset
    }
    pub(crate) fn minimum_grade(&self) -> Option<Grade> {
        self.minimum_grade
    }
    pub(crate) fn set_minimum_grade(&mut self, grade: Option<Grade>) {
        self.minimum_grade = grade;
    }
    pub(crate) fn signoffs(&self) -> &[BuildSignoff] {
        &self.signoffs
    }
    pub(crate) fn signoffs_mut(&mut self) -> &mut Vec<BuildSignoff> {
        &mut self.signoffs
    }
    pub(crate) fn next_ownership(&self) -> u16 {
        self.next_ownership
    }
    pub(crate) fn next_boundary(&self) -> u16 {
        self.next_boundary
    }

    pub(crate) fn add_ownership(&mut self, text: String) -> Result<&Entry, Error> {
        let id = self.id.entry_id("OWNERSHIP", self.next_ownership);
        let entry = Entry::from_parts(id, text)?;
        self.next_ownership = self
            .next_ownership
            .checked_add(1)
            .ok_or_else(|| Error::InvalidEntryId(self.id.to_string()))?;
        self.ownership.push(entry);
        self.ownership.last().ok_or(Error::EmptyText)
    }

    pub(crate) fn ownership_mut(&mut self, id: &str) -> Result<&mut Entry, Error> {
        self.ownership
            .iter_mut()
            .find(|entry| entry.id() == id)
            .ok_or_else(|| Error::MissingEntry(id.to_owned()))
    }

    pub(crate) fn remove_ownership(&mut self, id: &str) -> Result<(), Error> {
        let before = self.ownership.len();
        self.ownership.retain(|entry| entry.id() != id);
        if before == self.ownership.len() {
            return Err(Error::MissingEntry(id.to_owned()));
        }
        Ok(())
    }

    pub(crate) fn add_boundary(
        &mut self,
        text: String,
        owner: Option<ContextId>,
    ) -> Result<&Boundary, Error> {
        let id = self.id.entry_id("BOUNDARY", self.next_boundary);
        let entry = Entry::from_parts(id, text)?;
        self.next_boundary = self
            .next_boundary
            .checked_add(1)
            .ok_or_else(|| Error::InvalidEntryId(self.id.to_string()))?;
        self.boundaries.push(Boundary::from_parts(entry, owner));
        self.boundaries.last().ok_or(Error::EmptyText)
    }

    pub(crate) fn boundary_mut(&mut self, id: &str) -> Result<&mut Boundary, Error> {
        self.boundaries
            .iter_mut()
            .find(|entry| entry.id() == id)
            .ok_or_else(|| Error::MissingEntry(id.to_owned()))
    }

    pub(crate) fn remove_boundary(&mut self, id: &str) -> Result<(), Error> {
        let before = self.boundaries.len();
        self.boundaries.retain(|entry| entry.id() != id);
        if before == self.boundaries.len() {
            return Err(Error::MissingEntry(id.to_owned()));
        }
        Ok(())
    }

    pub(crate) fn add_rule(&mut self, input: NewRule) -> Result<(), Error> {
        self.ruleset.add_rule(input).map_err(Into::into)
    }
    pub(crate) fn update_rule(&mut self, id: &str, update: RuleUpdate) -> Result<(), Error> {
        self.ruleset.update_rule(id, update).map_err(Into::into)
    }
    pub(crate) fn remove_rule(&mut self, id: &str) -> Result<(), Error> {
        self.ruleset.remove_rule(id).map_err(Into::into)
    }

    pub(crate) fn validate_identities(&self) -> Result<(), Error> {
        validate_entry_ids(
            &self.id,
            "OWNERSHIP",
            self.next_ownership,
            self.ownership.iter().map(Entry::id),
        )?;
        validate_entry_ids(
            &self.id,
            "BOUNDARY",
            self.next_boundary,
            self.boundaries.iter().map(Boundary::id),
        )?;

        let mut signoff_ids = BTreeSet::new();
        for signoff in &self.signoffs {
            if !signoff_ids.insert(signoff.id()) {
                return Err(Error::DuplicateSignoff(signoff.id().to_owned()));
            }
            let expected = Self::signoff_identity(&self.id, signoff)?;
            if expected != signoff.id() {
                return Err(Error::MissingSignoff(signoff.id().to_owned()));
            }
            let unique_paths = signoff.included_paths().iter().collect::<BTreeSet<_>>();
            if unique_paths.len() != signoff.included_paths().len() {
                return Err(Error::InvalidIncludedPath);
            }
        }
        Ok(())
    }

    fn signoff_identity(id: &ContextId, signoff: &BuildSignoff) -> Result<String, Error> {
        Ok(BuildSignoff::try_new(
            id,
            signoff.target().to_owned(),
            signoff.stage(),
            signoff.resource_group().map(str::to_owned),
            signoff.included_paths().to_vec(),
        )?
        .id()
        .to_owned())
    }
}

impl fmt::Debug for Context {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Context")
            .field("id", &self.id)
            .field("purpose", &self.purpose)
            .field("next_ownership", &self.next_ownership)
            .field("next_boundary", &self.next_boundary)
            .field("ownership", &self.ownership)
            .field("boundaries", &self.boundaries)
            .field("ruleset", &self.ruleset)
            .field("minimum_grade", &self.minimum_grade)
            .field("signoffs", &self.signoffs)
            .finish()
    }
}

fn required(value: String) -> Result<String, Error> {
    if value.trim().is_empty() {
        Err(Error::EmptyText)
    } else {
        Ok(value)
    }
}

fn validate_entry_ids<'entry>(
    context: &ContextId,
    kind: &str,
    next: u16,
    ids: impl Iterator<Item = &'entry str>,
) -> Result<(), Error> {
    if next == 0 {
        return Err(Error::InvalidEntryId(context.to_string()));
    }
    let prefix = format!("{}_{kind}_", context.as_str());
    let mut seen = BTreeSet::new();
    let mut maximum = 0;
    for id in ids {
        let suffix = id
            .strip_prefix(&prefix)
            .filter(|suffix| suffix.len() >= 3 && suffix.bytes().all(|byte| byte.is_ascii_digit()))
            .and_then(|suffix| suffix.parse::<u16>().ok())
            .filter(|number| context.entry_id(kind, *number) == id)
            .ok_or_else(|| Error::InvalidEntryId(id.to_owned()))?;
        if suffix == 0 || !seen.insert(suffix) {
            return Err(Error::InvalidEntryId(id.to_owned()));
        }
        maximum = maximum.max(suffix);
    }
    if next <= maximum {
        return Err(Error::InvalidEntryId(context.to_string()));
    }
    Ok(())
}
