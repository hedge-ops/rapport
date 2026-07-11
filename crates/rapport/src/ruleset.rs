use crate::cli::{RulesCommand, RulesIncludeCommand, RulesReferenceCommand, RulesRuleCommand};
use crate::context::{Clock, CommandContext};
use crate::repository_files::find_named_files;
use crate::telemetry::{CommandEvent, CommandEventOutcome, TelemetryWriter};
use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::{self, Write};
use std::process::ExitCode;

pub(crate) const VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RulesetDocument {
    pub(crate) version: u16,
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) includes: Vec<String>,
    #[serde(default)]
    pub(crate) rules: Vec<RuleDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EmbeddedRuleset {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) includes: Vec<String>,
    #[serde(default)]
    pub(crate) rules: Vec<RuleDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuleDefinition {
    pub(crate) id: String,
    pub(crate) text: String,
    #[serde(default)]
    pub(crate) rationale: Option<String>,
    #[serde(default)]
    pub(crate) references: Vec<RuleReference>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuleReference {
    pub(crate) kind: RuleReferenceKind,
    pub(crate) target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) label: Option<String>,
}

impl<'de> Deserialize<'de> for RuleReference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Input {
            Typed {
                kind: RuleReferenceKind,
                target: String,
                #[serde(default)]
                label: Option<String>,
            },
            Legacy(String),
        }
        match Input::deserialize(deserializer)? {
            Input::Typed {
                kind,
                target,
                label,
            } => Ok(Self {
                kind,
                target,
                label,
            }),
            Input::Legacy(target)
                if target.starts_with("http://") || target.starts_with("https://") =>
            {
                Ok(Self {
                    kind: RuleReferenceKind::External,
                    target,
                    label: None,
                })
            }
            Input::Legacy(target) => Ok(Self {
                kind: RuleReferenceKind::Repository,
                target: format!("/{}", target.trim_start_matches('/')),
                label: None,
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RuleReferenceKind {
    Repository,
    External,
}

impl fmt::Display for RuleReferenceKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Repository => "repository",
            Self::External => "external",
        })
    }
}

impl RuleReference {
    pub(crate) fn display(&self) -> String {
        self.label.as_ref().map_or_else(
            || format!("{}: {}", self.kind, self.target),
            |label| format!("{}: {} ({})", self.kind, label, self.target),
        )
    }
}

pub(crate) fn migrate_reference(
    fs: &impl FileSystem,
    repo_root: &Utf8Path,
    value: &str,
) -> Result<RuleReference, RulesetError> {
    let target = value.trim();
    if target.is_empty() || target.starts_with("legacy:") {
        return Err(RulesetError::InvalidReference(value.to_string()));
    }
    if target.starts_with("http://") || target.starts_with("https://") {
        let authority = target
            .split_once("://")
            .map(|(_, rest)| rest)
            .unwrap_or_default();
        let host = authority.split(['/', '?', '#']).next().unwrap_or_default();
        if host.is_empty() || host.contains(char::is_whitespace) {
            return Err(RulesetError::InvalidReference(value.to_string()));
        }
        return Ok(RuleReference {
            kind: RuleReferenceKind::External,
            target: target.to_string(),
            label: None,
        });
    }
    let relative = target.trim_start_matches('/');
    let relative = Utf8Path::new(relative);
    if relative.as_str().is_empty()
        || relative.is_absolute()
        || relative.as_str().split('/').any(|part| part == "..")
    {
        return Err(RulesetError::InvalidReference(value.to_string()));
    }
    let path = repo_root.join(relative);
    if !fs.is_file(&path) {
        return Err(RulesetError::MissingReference(path));
    }
    validate_repository_boundary(fs, repo_root, &path)?;
    Ok(RuleReference {
        kind: RuleReferenceKind::Repository,
        target: format!("/{relative}"),
        label: None,
    })
}

fn validate_repository_boundary(
    fs: &impl FileSystem,
    repo_root: &Utf8Path,
    path: &Utf8Path,
) -> Result<(), RulesetError> {
    let canonical_root = fs
        .canonicalize(repo_root)
        .map_err(|source| RulesetError::Io {
            path: repo_root.to_path_buf(),
            source,
        })?;
    let canonical_path = fs.canonicalize(path).map_err(|source| RulesetError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err(RulesetError::EscapingReference(path.to_path_buf()));
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub(crate) struct CatalogEntry {
    pub(crate) document: RulesetDocument,
    pub(crate) source: Utf8PathBuf,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Catalog {
    entries: BTreeMap<String, CatalogEntry>,
    legacy_sources: Vec<Utf8PathBuf>,
}

impl Catalog {
    pub(crate) fn discover_repository(
        fs: &impl FileSystem,
        repo_root: &Utf8Path,
    ) -> Result<Self, RulesetError> {
        let mut catalog = Self::discover_documents(fs, repo_root)?;
        let mut has_embedded = false;
        for context_file in
            find_named_files(fs, repo_root, "context.toml").map_err(|source| RulesetError::Io {
                path: repo_root.to_path_buf(),
                source,
            })?
        {
            let contents = fs
                .read_to_string(&context_file)
                .map_err(|source| RulesetError::Io {
                    path: context_file.clone(),
                    source,
                })?;
            let Ok(carrier) = toml::from_str::<ContextRulesetCarrier>(&contents) else {
                continue;
            };
            if let Some(ruleset) = carrier.ruleset {
                has_embedded = true;
                catalog.insert_embedded(ruleset, context_file)?;
            } else if !carrier.rule_includes.is_empty() || !carrier.rules.is_empty() {
                catalog.legacy_sources.push(context_file);
            }
        }
        if has_embedded && let Some(source) = catalog.legacy_sources.first() {
            return Err(RulesetError::MixedLegacySources(source.clone()));
        }
        catalog.validate_references(fs, repo_root)?;
        catalog.validate()?;
        Ok(catalog)
    }

    pub(crate) fn discover_documents(
        fs: &impl FileSystem,
        repo_root: &Utf8Path,
    ) -> Result<Self, RulesetError> {
        let root = repo_root.join(".rapport/rules");
        let mut files = Vec::new();
        if fs.is_dir(&root) {
            collect_toml(fs, &root, &mut files)
                .map_err(|source| RulesetError::Io { path: root, source })?;
        }
        files.sort();
        let mut entries: BTreeMap<String, CatalogEntry> = BTreeMap::new();
        for source in files {
            let document = load(fs, &source)?;
            validate_local(&document.id, &document.rules, &source)?;
            if let Some(first) = entries.get(&document.id) {
                return Err(RulesetError::DuplicateRuleset {
                    id: document.id,
                    first: first.source.clone(),
                    second: source,
                });
            }
            entries.insert(document.id.clone(), CatalogEntry { document, source });
        }
        Ok(Self {
            entries,
            legacy_sources: Vec::new(),
        })
    }

    pub(crate) fn entries(&self) -> impl Iterator<Item = &CatalogEntry> {
        self.entries.values()
    }

    pub(crate) fn get(&self, id: &str) -> Option<&CatalogEntry> {
        self.entries.get(id)
    }

    pub(crate) fn insert_embedded(
        &mut self,
        document: EmbeddedRuleset,
        source: Utf8PathBuf,
    ) -> Result<(), RulesetError> {
        if let Some(legacy_source) = self.legacy_sources.first() {
            return Err(RulesetError::MixedLegacySources(legacy_source.clone()));
        }
        validate_local(&document.id, &document.rules, &source)?;
        if let Some(first) = self.entries.get(&document.id) {
            return Err(RulesetError::DuplicateRuleset {
                id: document.id,
                first: first.source.clone(),
                second: source,
            });
        }
        let document = RulesetDocument {
            version: VERSION,
            id: document.id,
            includes: document.includes,
            rules: document.rules,
        };
        self.entries
            .insert(document.id.clone(), CatalogEntry { document, source });
        Ok(())
    }

    pub(crate) fn replace_embedded(
        &mut self,
        document: EmbeddedRuleset,
        source: Utf8PathBuf,
    ) -> Result<(), RulesetError> {
        self.entries.retain(|_, entry| entry.source != source);
        self.legacy_sources.retain(|legacy| legacy != &source);
        self.insert_embedded(document, source)?;
        self.validate()
    }

    pub(crate) fn validate(&self) -> Result<(), RulesetError> {
        self.validate_graph()
    }

    fn validate_references(
        &self,
        fs: &impl FileSystem,
        repo_root: &Utf8Path,
    ) -> Result<(), RulesetError> {
        for entry in self.entries.values() {
            for rule in &entry.document.rules {
                for reference in &rule.references {
                    if reference.kind == RuleReferenceKind::Repository {
                        let path = repo_root.join(reference.target.trim_start_matches('/'));
                        if !fs.is_file(&path) {
                            return Err(RulesetError::MissingReference(path));
                        }
                        validate_repository_boundary(fs, repo_root, &path)?;
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) fn resolve(&self, id: &str) -> Result<Vec<ResolvedRule>, RulesetError> {
        let mut visited = BTreeSet::new();
        let mut stack = Vec::new();
        let mut rules = Vec::new();
        self.collect(id, &mut visited, &mut stack, &mut rules)?;
        Ok(rules)
    }

    fn collect(
        &self,
        id: &str,
        visited: &mut BTreeSet<String>,
        stack: &mut Vec<String>,
        rules: &mut Vec<ResolvedRule>,
    ) -> Result<(), RulesetError> {
        if visited.contains(id) {
            return Ok(());
        }
        if let Some(index) = stack.iter().position(|item| item == id) {
            let mut cycle = stack[index..].to_vec();
            cycle.push(id.to_string());
            return Err(RulesetError::Cycle(cycle));
        }
        let entry = self
            .entries
            .get(id)
            .ok_or_else(|| RulesetError::MissingInclude {
                owner: stack.last().cloned().unwrap_or_else(|| id.to_string()),
                included: id.to_string(),
            })?;
        stack.push(id.to_string());
        for included in &entry.document.includes {
            self.collect(included, visited, stack, rules)?;
        }
        stack.pop();
        visited.insert(id.to_string());
        rules.extend(
            entry
                .document
                .rules
                .iter()
                .cloned()
                .map(|rule| ResolvedRule {
                    rule,
                    source: entry.source.clone(),
                }),
        );
        Ok(())
    }

    fn validate_graph(&self) -> Result<(), RulesetError> {
        for entry in self.entries.values() {
            for included in &entry.document.includes {
                if !self.entries.contains_key(included) {
                    return Err(RulesetError::MissingInclude {
                        owner: entry.document.id.clone(),
                        included: included.clone(),
                    });
                }
            }
            self.resolve(&entry.document.id)?;
        }
        let mut rules = BTreeMap::<String, Utf8PathBuf>::new();
        for entry in self.entries.values() {
            for rule in &entry.document.rules {
                if let Some(first) = rules.insert(rule.id.clone(), entry.source.clone()) {
                    return Err(RulesetError::DuplicateRule {
                        id: rule.id.clone(),
                        first,
                        second: entry.source.clone(),
                    });
                }
            }
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct ContextRulesetCarrier {
    #[serde(default)]
    ruleset: Option<EmbeddedRuleset>,
    #[serde(default)]
    rule_includes: Vec<String>,
    #[serde(default)]
    rules: Vec<RuleDefinition>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedRule {
    pub(crate) rule: RuleDefinition,
    pub(crate) source: Utf8PathBuf,
}

pub(crate) fn validate_local(
    ruleset_id: &str,
    rules: &[RuleDefinition],
    source: &Utf8Path,
) -> Result<(), RulesetError> {
    if ruleset_id.trim().is_empty() {
        return Err(RulesetError::MissingId(source.to_path_buf()));
    }
    let mut separator = None;
    for rule in rules {
        let mut targets = BTreeSet::new();
        for reference in &rule.references {
            if !targets.insert(&reference.target) {
                return Err(RulesetError::DuplicateReference(reference.target.clone()));
            }
            let valid = match reference.kind {
                RuleReferenceKind::External => {
                    (reference.target.starts_with("http://")
                        || reference.target.starts_with("https://"))
                        && reference.target.split_once("://").is_some_and(|(_, rest)| {
                            rest.split(['/', '?', '#']).next().is_some_and(|host| {
                                !host.is_empty() && !host.contains(char::is_whitespace)
                            })
                        })
                }
                RuleReferenceKind::Repository => {
                    reference.target.starts_with('/')
                        && reference.target.len() > 1
                        && !reference.target.split('/').any(|part| part == "..")
                }
            };
            if !valid {
                return Err(RulesetError::InvalidReference(reference.target.clone()));
            }
        }
        let suffix = rule
            .id
            .strip_prefix(ruleset_id)
            .ok_or_else(|| RulesetError::Namespace {
                ruleset: ruleset_id.to_string(),
                rule: rule.id.clone(),
            })?;
        let current = suffix
            .chars()
            .next()
            .filter(|value| matches!(value, '-' | '_'))
            .ok_or_else(|| RulesetError::Namespace {
                ruleset: ruleset_id.to_string(),
                rule: rule.id.clone(),
            })?;
        if separator.is_some_and(|expected| expected != current) {
            return Err(RulesetError::Separator {
                ruleset: ruleset_id.to_string(),
            });
        }
        separator = Some(current);
    }
    Ok(())
}

fn collect_toml(
    fs: &impl FileSystem,
    directory: &Utf8Path,
    files: &mut Vec<Utf8PathBuf>,
) -> io::Result<()> {
    for entry in fs.read_dir(directory)? {
        if fs.is_dir(&entry) {
            collect_toml(fs, &entry, files)?;
        } else if entry.extension() == Some("toml") {
            files.push(entry);
        }
    }
    Ok(())
}

fn load(fs: &impl FileSystem, path: &Utf8Path) -> Result<RulesetDocument, RulesetError> {
    let contents = fs.read_to_string(path).map_err(|source| RulesetError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let document: RulesetDocument =
        toml::from_str(&contents).map_err(|source| RulesetError::Decode {
            path: path.to_path_buf(),
            source,
        })?;
    if document.version != VERSION {
        return Err(RulesetError::Version {
            path: path.to_path_buf(),
            version: document.version,
        });
    }
    Ok(document)
}

fn render(document: &RulesetDocument) -> Result<String, RulesetError> {
    toml_edit::ser::to_document(document)
        .map(|document| document.to_string())
        .map_err(RulesetError::Encode)
}

pub(crate) fn run<F, C, O, E>(
    command: &RulesCommand,
    arguments: Vec<String>,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> ExitCode
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let result = execute(command, context);
    match result {
        Ok(message) => {
            let _ = writeln!(context.out, "{message}");
            finish(arguments, context, true)
        }
        Err(error) => {
            let _ = writeln!(context.err, "# rapport rules\n\n{error}");
            finish(arguments, context, false)
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "the command dispatcher keeps all rules CLI routing visible together"
)]
fn execute<F, C, O, E>(
    command: &RulesCommand,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> Result<String, RulesetError>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    match command {
        RulesCommand::List => {
            let catalog = Catalog::discover_repository(context.fs, &context.repo_root)?;
            let lines = catalog
                .entries()
                .map(|entry| {
                    format!(
                        "- `{}` -- {}",
                        entry.document.id,
                        display(&context.repo_root, &entry.source)
                    )
                })
                .collect::<Vec<_>>();
            Ok(format!(
                "# rapport rules list\n\n## Rulesets\n\n{}",
                if lines.is_empty() {
                    String::from("No rulesets discovered.")
                } else {
                    lines.join("\n")
                }
            ))
        }
        RulesCommand::Show { id } => {
            let catalog = Catalog::discover_repository(context.fs, &context.repo_root)?;
            let entry = catalog
                .get(id)
                .ok_or_else(|| RulesetError::Unknown(id.clone()))?;
            let rules = entry
                .document
                .rules
                .iter()
                .map(|rule| format!("- `{}` -- {}", rule.id, rule.text))
                .collect::<Vec<_>>();
            Ok(format!(
                "# rapport rules show\n\n- `id` -- {}\n- `source` -- {}\n- `includes` -- {}\n\n## Rules\n\n{}",
                id,
                display(&context.repo_root, &entry.source),
                entry.document.includes.join(", "),
                if rules.is_empty() {
                    String::from("No local rules.")
                } else {
                    rules.join("\n")
                }
            ))
        }
        RulesCommand::Init(args) => {
            let relative = if args.path.extension() == Some("toml") {
                args.path.clone()
            } else {
                args.path.with_extension("toml")
            };
            if relative.is_absolute() || relative.as_str().split('/').any(|part| part == "..") {
                return Err(RulesetError::InvalidPath(relative));
            }
            let path = context.repo_root.join(".rapport/rules").join(relative);
            if context.fs.exists(&path) {
                return Err(RulesetError::Exists(path));
            }
            let document = RulesetDocument {
                version: VERSION,
                id: args.id.clone(),
                includes: Vec::new(),
                rules: Vec::new(),
            };
            validate_local(&document.id, &document.rules, &path)?;
            let mut catalog = Catalog::discover_repository(context.fs, &context.repo_root)?;
            catalog.insert_embedded(
                EmbeddedRuleset {
                    id: document.id.clone(),
                    includes: Vec::new(),
                    rules: Vec::new(),
                },
                path.clone(),
            )?;
            catalog.validate()?;
            if let Some(parent) = path.parent() {
                context
                    .fs
                    .create_dir_all(parent)
                    .map_err(|source| RulesetError::Io {
                        path: parent.to_path_buf(),
                        source,
                    })?;
            }
            context
                .fs
                .write_string(&path, render(&document)?)
                .map_err(|source| RulesetError::Io {
                    path: path.clone(),
                    source,
                })?;
            Ok(format!(
                "# rapport rules init\n\n- `id` -- {}\n- `path` -- {}",
                args.id,
                display(&context.repo_root, &path)
            ))
        }
        RulesCommand::Include(args) => mutate(command, args.command.ruleset(), context),
        RulesCommand::Rule(args) => match &args.command {
            RulesRuleCommand::Reference(reference) => match &reference.command {
                RulesReferenceCommand::List { ruleset, id } => {
                    let catalog = Catalog::discover_repository(context.fs, &context.repo_root)?;
                    let rule = catalog
                        .get(ruleset)
                        .ok_or_else(|| RulesetError::Unknown(ruleset.clone()))?
                        .document
                        .rules
                        .iter()
                        .find(|rule| rule.id == *id)
                        .ok_or_else(|| RulesetError::UnknownRule(id.clone()))?;
                    let items = if rule.references.is_empty() {
                        String::from("- No references.")
                    } else {
                        rule.references
                            .iter()
                            .map(|reference| format!("- {}", reference.display()))
                            .collect::<Vec<_>>()
                            .join("\n")
                    };
                    Ok(format!("# rapport rules rule reference list\n\n{items}"))
                }
                _ => mutate(command, args.command.ruleset(), context),
            },
            _ => mutate(command, args.command.ruleset(), context),
        },
    }
}

trait TargetRuleset {
    fn ruleset(&self) -> &str;
}
impl TargetRuleset for RulesIncludeCommand {
    fn ruleset(&self) -> &str {
        match self {
            Self::Add { ruleset, .. } | Self::Remove { ruleset, .. } => ruleset,
        }
    }
}
impl TargetRuleset for RulesRuleCommand {
    fn ruleset(&self) -> &str {
        match self {
            Self::Add(args) => &args.ruleset,
            Self::Update(args) => &args.ruleset,
            Self::Remove { ruleset, .. } => ruleset,
            Self::Reference(args) => match &args.command {
                RulesReferenceCommand::List { ruleset, .. }
                | RulesReferenceCommand::Remove { ruleset, .. } => ruleset,
                RulesReferenceCommand::Add(args) => &args.ruleset,
            },
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "ruleset mutations share one validate-before-write transaction"
)]
fn mutate<F, C, O, E>(
    command: &RulesCommand,
    id: &str,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> Result<String, RulesetError>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let catalog = Catalog::discover_repository(context.fs, &context.repo_root)?;
    let entry = catalog
        .get(id)
        .ok_or_else(|| RulesetError::Unknown(id.to_string()))?;
    if entry.source.file_name() == Some("context.toml") {
        return Err(RulesetError::EmbeddedMutation(id.to_string()));
    }
    let path = entry.source.clone();
    let mut document = entry.document.clone();
    match command {
        RulesCommand::Include(args) => match &args.command {
            RulesIncludeCommand::Add { included, .. } => {
                if !catalog.entries.contains_key(included) {
                    return Err(RulesetError::Unknown(included.clone()));
                }
                if !document.includes.contains(included) {
                    document.includes.push(included.clone());
                }
            }
            RulesIncludeCommand::Remove { included, .. } => {
                document.includes.retain(|value| value != included);
            }
        },
        RulesCommand::Rule(args) => match &args.command {
            RulesRuleCommand::Add(args) => {
                if document.rules.iter().any(|rule| rule.id == args.id) {
                    return Err(RulesetError::DuplicateRule {
                        id: args.id.clone(),
                        first: path.clone(),
                        second: path.clone(),
                    });
                }
                document.rules.push(RuleDefinition {
                    id: args.id.clone(),
                    text: args.text.clone(),
                    rationale: args.rationale.clone(),
                    references: args
                        .references
                        .iter()
                        .map(|reference| {
                            migrate_reference(context.fs, &context.repo_root, reference)
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                });
            }
            RulesRuleCommand::Update(args) => {
                let rule = document
                    .rules
                    .iter_mut()
                    .find(|rule| rule.id == args.id)
                    .ok_or_else(|| RulesetError::UnknownRule(args.id.clone()))?;
                if let Some(text) = &args.text {
                    rule.text.clone_from(text);
                }
                if let Some(rationale) = &args.rationale {
                    rule.rationale = Some(rationale.clone());
                } else if args.clear_rationale {
                    rule.rationale = None;
                }
                if !args.references.is_empty() {
                    rule.references = args
                        .references
                        .iter()
                        .map(|reference| {
                            migrate_reference(context.fs, &context.repo_root, reference)
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                } else if args.clear_references {
                    rule.references.clear();
                }
            }
            RulesRuleCommand::Remove { id, .. } => {
                let before = document.rules.len();
                document.rules.retain(|rule| rule.id != *id);
                if before == document.rules.len() {
                    return Err(RulesetError::UnknownRule(id.clone()));
                }
            }
            RulesRuleCommand::Reference(args) => match &args.command {
                RulesReferenceCommand::List { .. } => unreachable!(),
                RulesReferenceCommand::Add(args) => {
                    let rule = document
                        .rules
                        .iter_mut()
                        .find(|rule| rule.id == args.id)
                        .ok_or_else(|| RulesetError::UnknownRule(args.id.clone()))?;
                    let source = args
                        .repository
                        .as_deref()
                        .or(args.external.as_deref())
                        .ok_or_else(|| RulesetError::InvalidReference(String::new()))?;
                    let mut reference = migrate_reference(context.fs, &context.repo_root, source)?;
                    if args.repository.is_some() && reference.kind != RuleReferenceKind::Repository
                        || args.external.is_some() && reference.kind != RuleReferenceKind::External
                    {
                        return Err(RulesetError::InvalidReference(source.to_string()));
                    }
                    reference.label.clone_from(&args.label);
                    if rule
                        .references
                        .iter()
                        .any(|existing| existing.target == reference.target)
                    {
                        return Err(RulesetError::DuplicateReference(reference.target));
                    }
                    rule.references.push(reference);
                }
                RulesReferenceCommand::Remove { id, target, .. } => {
                    let rule = document
                        .rules
                        .iter_mut()
                        .find(|rule| rule.id == *id)
                        .ok_or_else(|| RulesetError::UnknownRule(id.clone()))?;
                    let before = rule.references.len();
                    rule.references
                        .retain(|reference| reference.target != *target);
                    if before == rule.references.len() {
                        return Err(RulesetError::UnknownReference(target.clone()));
                    }
                }
            },
        },
        _ => unreachable!(),
    }
    validate_local(&document.id, &document.rules, &path)?;
    let mut candidate = catalog.clone();
    candidate.entries.insert(
        document.id.clone(),
        CatalogEntry {
            document: document.clone(),
            source: path.clone(),
        },
    );
    candidate.validate()?;
    context
        .fs
        .write_string(&path, render(&document)?)
        .map_err(|source| RulesetError::Io {
            path: path.clone(),
            source,
        })?;
    Ok(format!(
        "# rapport rules\n\n- `ruleset` -- {id}\n- `path` -- {}",
        display(&context.repo_root, &path)
    ))
}

fn finish<F, C, O, E>(
    arguments: Vec<String>,
    context: &mut CommandContext<'_, F, C, O, E>,
    success: bool,
) -> ExitCode
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let code = if success { 0 } else { 2 };
    let event = CommandEvent::new(
        context.clock.now_rfc3339(),
        arguments,
        "rules",
        if success {
            CommandEventOutcome::Success
        } else {
            CommandEventOutcome::Failure
        },
        code,
    );
    if TelemetryWriter::new(context.paths.clone())
        .append(context.fs, &event)
        .is_err()
    {
        ExitCode::FAILURE
    } else {
        ExitCode::from(code)
    }
}

fn display(root: &Utf8Path, path: &Utf8Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string()
}

#[derive(Debug)]
pub(crate) enum RulesetError {
    Io {
        path: Utf8PathBuf,
        source: io::Error,
    },
    Decode {
        path: Utf8PathBuf,
        source: toml::de::Error,
    },
    Encode(toml_edit::ser::Error),
    Version {
        path: Utf8PathBuf,
        version: u16,
    },
    MissingId(Utf8PathBuf),
    DuplicateRuleset {
        id: String,
        first: Utf8PathBuf,
        second: Utf8PathBuf,
    },
    DuplicateRule {
        id: String,
        first: Utf8PathBuf,
        second: Utf8PathBuf,
    },
    MissingInclude {
        owner: String,
        included: String,
    },
    Cycle(Vec<String>),
    Namespace {
        ruleset: String,
        rule: String,
    },
    Separator {
        ruleset: String,
    },
    Unknown(String),
    UnknownRule(String),
    Exists(Utf8PathBuf),
    InvalidPath(Utf8PathBuf),
    EmbeddedMutation(String),
    MixedLegacySources(Utf8PathBuf),
    InvalidReference(String),
    MissingReference(Utf8PathBuf),
    DuplicateReference(String),
    UnknownReference(String),
    EscapingReference(Utf8PathBuf),
}

impl fmt::Display for RulesetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "ruleset filesystem error at `{path}`: {source}")
            }
            Self::Decode { path, source } => write!(f, "ruleset parse error at `{path}`: {source}"),
            Self::Encode(source) => write!(f, "could not encode ruleset: {source}"),
            Self::Version { path, version } => write!(
                f,
                "unsupported ruleset schema version `{version}` at `{path}`; supported version is {VERSION}"
            ),
            Self::MissingId(path) => write!(f, "ruleset at `{path}` has no id"),
            Self::DuplicateRuleset { id, first, second } => {
                write!(f, "duplicate ruleset id `{id}` in `{first}` and `{second}`")
            }
            Self::DuplicateRule { id, first, second } => {
                write!(f, "duplicate rule id `{id}` in `{first}` and `{second}`")
            }
            Self::MissingInclude { owner, included } => {
                write!(f, "ruleset `{owner}` includes missing ruleset `{included}`")
            }
            Self::Cycle(ids) => write!(f, "ruleset include cycle: {}", ids.join(" -> ")),
            Self::Namespace { ruleset, rule } => write!(
                f,
                "rule `{rule}` must begin with `{ruleset}-` or `{ruleset}_`"
            ),
            Self::Separator { ruleset } => write!(
                f,
                "ruleset `{ruleset}` mixes `-` and `_` namespace separators"
            ),
            Self::Unknown(id) => write!(f, "ruleset `{id}` was not found"),
            Self::UnknownRule(id) => write!(f, "rule `{id}` was not found"),
            Self::Exists(path) => write!(f, "ruleset file `{path}` already exists"),
            Self::InvalidPath(path) => write!(
                f,
                "ruleset path `{path}` must be relative and remain under .rapport/rules"
            ),
            Self::EmbeddedMutation(id) => write!(
                f,
                "ruleset `{id}` is embedded in context.toml; use `rapport context rule` commands"
            ),
            Self::MixedLegacySources(path) => write!(
                f,
                "legacy rule sources in `{path}` cannot coexist with embedded rulesets; convert the context before resolving repository rules"
            ),
            Self::InvalidReference(target) => write!(
                f,
                "rule reference `{target}` must be an HTTP(S) URL or an existing repository file"
            ),
            Self::MissingReference(path) => {
                write!(f, "rule reference file `{path}` does not exist")
            }
            Self::DuplicateReference(target) => {
                write!(f, "rule reference target `{target}` already exists")
            }
            Self::UnknownReference(target) => {
                write!(f, "rule reference target `{target}` was not found")
            }
            Self::EscapingReference(path) => write!(
                f,
                "rule reference file `{path}` resolves outside the repository"
            ),
        }
    }
}

impl std::error::Error for RulesetError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Decode { source, .. } => Some(source),
            Self::Encode(source) => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use rapport_files::InMemoryFileSystem;

    fn ruleset(id: &str, includes: &[&str], rule: Option<&str>) -> String {
        let includes = includes
            .iter()
            .map(|value| format!("\"{value}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let rule = rule.map_or_else(String::new, |rule| {
            format!("\n[[rules]]\nid = \"{rule}\"\ntext = \"Rule text.\"\n")
        });
        format!("version = 1\nid = \"{id}\"\nincludes = [{includes}]\n{rule}")
    }

    #[test]
    fn catalog_discovers_nested_rulesets_and_resolves_transitive_includes() {
        let mut fs = InMemoryFileSystem::default();
        fs.write_string(
            "/repo/.rapport/rules/base.toml",
            ruleset("BASE", &[], Some("BASE-001")),
        )
        .unwrap();
        fs.write_string(
            "/repo/.rapport/rules/rust/crate.toml",
            ruleset("CRATE", &["BASE"], Some("CRATE-001")),
        )
        .unwrap();

        let catalog = Catalog::discover_repository(&fs, Utf8Path::new("/repo")).unwrap();
        let resolved = catalog.resolve("CRATE").unwrap();

        assert_eq!(catalog.entries().count(), 2);
        assert_eq!(
            resolved
                .iter()
                .map(|rule| rule.rule.id.as_str())
                .collect::<Vec<_>>(),
            vec!["BASE-001", "CRATE-001"]
        );
    }

    #[test]
    fn catalog_rejects_include_cycles() {
        let mut fs = InMemoryFileSystem::default();
        fs.write_string("/repo/.rapport/rules/a.toml", ruleset("A", &["B"], None))
            .unwrap();
        fs.write_string("/repo/.rapport/rules/b.toml", ruleset("B", &["A"], None))
            .unwrap();

        let error = Catalog::discover_repository(&fs, Utf8Path::new("/repo")).unwrap_err();

        assert!(error.to_string().contains("A -> B -> A"));
    }

    #[test]
    fn validation_rejects_rules_outside_the_declaring_namespace() {
        let error = validate_local(
            "RUST",
            &[RuleDefinition {
                id: String::from("TEST-001"),
                text: String::from("Text"),
                rationale: None,
                references: Vec::new(),
            }],
            Utf8Path::new("/repo/.rapport/rules/rust.toml"),
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("must begin with `RUST-` or `RUST_`")
        );
    }

    #[test]
    fn legacy_external_reference_migrates_to_typed_record() {
        let document: RulesetDocument = toml::from_str(
            "version = 1\nid = \"RUST\"\nincludes = []\n[[rules]]\nid = \"RUST-001\"\ntext = \"Text\"\nreferences = [\"https://example.com/source\"]\n",
        )
        .unwrap();

        assert_eq!(
            document.rules[0].references[0].kind,
            RuleReferenceKind::External
        );
        assert_eq!(
            document.rules[0].references[0].target,
            "https://example.com/source"
        );
    }

    #[test]
    fn repository_reference_migration_requires_an_existing_file() {
        let mut fs = InMemoryFileSystem::default();
        fs.write_string("/repo/docs/decision.md", "decision")
            .unwrap();

        let reference = migrate_reference(&fs, Utf8Path::new("/repo"), "docs/decision.md").unwrap();
        assert_eq!(reference.kind, RuleReferenceKind::Repository);
        assert_eq!(reference.target, "/docs/decision.md");
        assert!(migrate_reference(&fs, Utf8Path::new("/repo"), "docs/missing.md").is_err());
        assert!(migrate_reference(&fs, Utf8Path::new("/repo"), "../outside.md").is_err());
    }

    #[test]
    fn validation_rejects_malformed_and_duplicate_reference_targets() {
        let reference = RuleReference {
            kind: RuleReferenceKind::External,
            target: String::from("https:///missing-host"),
            label: None,
        };
        let rule = RuleDefinition {
            id: String::from("RUST-001"),
            text: String::from("Text"),
            rationale: None,
            references: vec![reference.clone(), reference],
        };

        assert!(validate_local("RUST", &[rule], Utf8Path::new("rules.toml")).is_err());
    }
}
