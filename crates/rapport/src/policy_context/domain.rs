//! Context architecture domain.
//!
//! Owns stable identities, component purpose, ownership, boundaries, and local
//! review benchmarks. Persistence remains a boundary concern.

use super::Error;
use crate::shared_ruleset::{NewRule, RuleUpdate, Ruleset, RulesetId};
use rapport_files::Utf8Path;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

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

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::Display)]
#[display("{_0}")]
pub(crate) struct DependencyName(String);

impl DependencyName {
    pub(crate) fn try_new(value: String, path: &Utf8Path, field: String) -> Result<Self, Error> {
        let valid = value.split('_').enumerate().all(|(index, part)| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
                && (index > 0 || part.as_bytes().first().is_some_and(u8::is_ascii_lowercase))
        });
        if !valid {
            return Err(Error::InvalidDependencyName {
                path: path.to_path_buf(),
                field,
                name: value,
            });
        }
        Ok(Self(value))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for DependencyName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("DependencyName")
            .field(&self.0)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct RepositoryPath {
    value: String,
    canonical: String,
}

impl RepositoryPath {
    pub(crate) fn try_new(value: String, path: &Utf8Path, field: String) -> Result<Self, Error> {
        let Some(canonical) = canonical_relative_path(&value) else {
            return Err(Error::InvalidRelativePath {
                path: path.to_path_buf(),
                field,
                value,
            });
        };
        Ok(Self { value, canonical })
    }

    pub(crate) fn as_path(&self) -> &Utf8Path {
        Utf8Path::new(&self.canonical)
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.value
    }
}

impl fmt::Display for RepositoryPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.value)
    }
}

impl fmt::Debug for RepositoryPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("RepositoryPath")
            .field(&self.value)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::Display)]
#[display("{_0}")]
pub(crate) struct ComponentRelativePath(String);

impl ComponentRelativePath {
    pub(crate) fn try_new(value: String, path: &Utf8Path, field: String) -> Result<Self, Error> {
        if !valid_relative_path(&value) {
            return Err(Error::InvalidRelativePath {
                path: path.to_path_buf(),
                field,
                value,
            });
        }
        Ok(Self(value))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ComponentRelativePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("ComponentRelativePath")
            .field(&self.0)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct GeneratedOutput {
    tool: String,
    target: String,
}

impl GeneratedOutput {
    pub(crate) fn try_new(
        name: &DependencyName,
        tool: String,
        target: String,
        path: &Utf8Path,
    ) -> Result<Self, Error> {
        if tool.trim().is_empty() {
            return Err(Error::EmptyGeneratedOutputField {
                path: path.to_path_buf(),
                output: name.to_string(),
                field: "tool",
            });
        }
        if target.trim().is_empty() {
            return Err(Error::EmptyGeneratedOutputField {
                path: path.to_path_buf(),
                output: name.to_string(),
                field: "target",
            });
        }
        Ok(Self { tool, target })
    }

    pub(crate) fn tool(&self) -> &str {
        &self.tool
    }

    pub(crate) fn target(&self) -> &str {
        &self.target
    }
}

impl fmt::Debug for GeneratedOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GeneratedOutput")
            .field("tool", &self.tool)
            .field("target", &self.target)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct GeneratedInput {
    component: RepositoryPath,
    output: DependencyName,
}

impl GeneratedInput {
    pub(crate) fn from_parts(component: RepositoryPath, output: DependencyName) -> Self {
        Self { component, output }
    }

    pub(crate) fn component(&self) -> &RepositoryPath {
        &self.component
    }

    pub(crate) fn output(&self) -> &DependencyName {
        &self.output
    }
}

impl fmt::Debug for GeneratedInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GeneratedInput")
            .field("component", &self.component)
            .field("output", &self.output)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ContextDeclarations {
    pub(crate) components: Vec<RepositoryPath>,
    pub(crate) generated_outputs: BTreeMap<DependencyName, GeneratedOutput>,
    pub(crate) generated_inputs: BTreeMap<DependencyName, GeneratedInput>,
    pub(crate) kustomizations: Vec<ComponentRelativePath>,
}

impl ContextDeclarations {
    pub(crate) fn empty() -> Self {
        Self {
            components: Vec::new(),
            generated_outputs: BTreeMap::new(),
            generated_inputs: BTreeMap::new(),
            kustomizations: Vec::new(),
        }
    }
}

impl fmt::Debug for ContextDeclarations {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextDeclarations")
            .field("components", &self.components)
            .field("generated_outputs", &self.generated_outputs)
            .field("generated_inputs", &self.generated_inputs)
            .field("kustomizations", &self.kustomizations)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ContextEntries {
    pub(crate) next_ownership: u16,
    pub(crate) next_boundary: u16,
    pub(crate) ownership: Vec<Entry>,
    pub(crate) boundaries: Vec<Boundary>,
}

impl fmt::Debug for ContextEntries {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextEntries")
            .field("next_ownership", &self.next_ownership)
            .field("next_boundary", &self.next_boundary)
            .field("ownership", &self.ownership)
            .field("boundaries", &self.boundaries)
            .finish()
    }
}

fn valid_relative_path(value: &str) -> bool {
    canonical_relative_path(value).is_some()
}

fn canonical_relative_path(value: &str) -> Option<String> {
    if value.is_empty()
        || value.contains('\\')
        || Utf8Path::new(value).is_absolute()
        || value.as_bytes().get(1) == Some(&b':')
    {
        return None;
    }
    let components = Utf8Path::new(value)
        .components()
        .filter_map(|component| match component.as_str() {
            "." => None,
            ".." => Some(".."),
            component => Some(component),
        })
        .collect::<Vec<_>>();
    if components.contains(&"..") {
        return None;
    }
    Some(if components.is_empty() {
        ".".to_owned()
    } else {
        components.join("/")
    })
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

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Context {
    id: ContextId,
    purpose: String,
    component_type: Option<String>,
    namespaced: bool,
    components: Vec<RepositoryPath>,
    generated_outputs: BTreeMap<DependencyName, GeneratedOutput>,
    generated_inputs: BTreeMap<DependencyName, GeneratedInput>,
    kustomizations: Vec<ComponentRelativePath>,
    next_ownership: u16,
    next_boundary: u16,
    ownership: Vec<Entry>,
    boundaries: Vec<Boundary>,
    ruleset: Ruleset,
}

impl Context {
    pub(crate) fn new(id: ContextId, purpose: String) -> Result<Self, Error> {
        let ruleset_id = id.embedded_ruleset_id()?;
        let declarations = ContextDeclarations::empty();
        Ok(Self {
            id,
            purpose: required(purpose)?,
            component_type: None,
            namespaced: false,
            components: declarations.components,
            generated_outputs: declarations.generated_outputs,
            generated_inputs: declarations.generated_inputs,
            kustomizations: declarations.kustomizations,
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
        })
    }

    pub(crate) fn from_parts(
        id: ContextId,
        purpose: String,
        entries: ContextEntries,
        declarations: ContextDeclarations,
        ruleset: Ruleset,
    ) -> Result<Self, Error> {
        Ok(Self {
            id,
            purpose: required(purpose)?,
            component_type: None,
            namespaced: false,
            components: declarations.components,
            generated_outputs: declarations.generated_outputs,
            generated_inputs: declarations.generated_inputs,
            kustomizations: declarations.kustomizations,
            next_ownership: entries.next_ownership,
            next_boundary: entries.next_boundary,
            ownership: entries.ownership,
            boundaries: entries.boundaries,
            ruleset,
        })
    }

    pub(crate) fn id(&self) -> &ContextId {
        &self.id
    }
    pub(crate) fn component_type(&self) -> Option<&str> {
        self.component_type.as_deref()
    }
    pub(crate) fn namespaced(&self) -> bool {
        self.namespaced
    }
    pub(crate) fn set_schema(
        &mut self,
        namespaced: bool,
        component_type: Option<String>,
    ) -> Result<(), Error> {
        self.namespaced = namespaced;
        self.component_type = component_type.map(required).transpose()?;
        Ok(())
    }
    pub(crate) fn purpose(&self) -> &str {
        &self.purpose
    }

    pub(crate) fn components(&self) -> &[RepositoryPath] {
        &self.components
    }

    pub(crate) fn generated_outputs(&self) -> &BTreeMap<DependencyName, GeneratedOutput> {
        &self.generated_outputs
    }

    pub(crate) fn generated_inputs(&self) -> &BTreeMap<DependencyName, GeneratedInput> {
        &self.generated_inputs
    }

    pub(crate) fn kustomizations(&self) -> &[ComponentRelativePath] {
        &self.kustomizations
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

        Ok(())
    }
}

impl fmt::Debug for Context {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Context")
            .field("id", &self.id)
            .field("purpose", &self.purpose)
            .field("component_type", &self.component_type)
            .field("namespaced", &self.namespaced)
            .field("components", &self.components)
            .field("generated_outputs", &self.generated_outputs)
            .field("generated_inputs", &self.generated_inputs)
            .field("kustomizations", &self.kustomizations)
            .field("next_ownership", &self.next_ownership)
            .field("next_boundary", &self.next_boundary)
            .field("ownership", &self.ownership)
            .field("boundaries", &self.boundaries)
            .field("ruleset", &self.ruleset)
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
