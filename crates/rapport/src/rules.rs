use crate::context::{Clock, CommandContext};
use crate::paths::RapportPaths;
use crate::state::{WorkStateError, WorkStateStore};
use crate::telemetry::{CommandEvent, CommandEventOutcome, TelemetryError, TelemetryWriter};
use crate::{RunHint, ViewBuilder};
use nonempty::nonempty;
use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::io::{self, Write};
use std::process::ExitCode;

const SUCCESS: u8 = 0;
const FAILURE: u8 = 2;

pub fn list<F, C, O, E>(
    path: Option<&Utf8PathBuf>,
    arguments: Vec<String>,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> ExitCode
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let resolver = RuleResolver::new(context.paths.clone());
    let result = match path {
        Some(path) => {
            let path = requested_path_from_cwd(path, &context.cwd);
            match resolver.resolve_paths(context.fs, [&path]) {
                Ok(resolutions) => {
                    let _ = writeln!(context.out, "{}", render_rule_list(&resolver, &resolutions));
                    CommandResult::success()
                }
                Err(error) => {
                    let _ = writeln!(context.err, "{}", render_rules_error("list", &error));
                    CommandResult::failure()
                }
            }
        }
        None => match active_work_paths(context.fs, &context.paths) {
            Ok(Some(paths)) if paths.is_empty() => {
                let _ = writeln!(context.out, "{}", render_no_active_paths("list"));
                CommandResult::success()
            }
            Ok(Some(paths)) => match resolver.resolve_paths(context.fs, &paths) {
                Ok(resolutions) => {
                    let _ = writeln!(context.out, "{}", render_rule_list(&resolver, &resolutions));
                    CommandResult::success()
                }
                Err(error) => {
                    let _ = writeln!(context.err, "{}", render_rules_error("list", &error));
                    CommandResult::failure()
                }
            },
            Ok(None) => {
                let _ = writeln!(context.err, "{}", render_missing_work("list"));
                CommandResult::failure()
            }
            Err(error) => {
                let _ = writeln!(context.err, "{}", render_work_state_error("list", &error));
                CommandResult::failure()
            }
        },
    };
    finish("work rules list", arguments, context, result)
}

pub fn show<F, C, O, E>(
    id: &str,
    arguments: Vec<String>,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> ExitCode
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let resolver = RuleResolver::new(context.paths.clone());
    let result = match active_work_paths(context.fs, &context.paths) {
        Ok(Some(paths)) if paths.is_empty() => {
            let _ = writeln!(context.err, "{}", render_no_active_paths("show"));
            CommandResult::failure()
        }
        Ok(Some(paths)) => match resolver.resolve_paths(context.fs, &paths) {
            Ok(resolutions) => {
                if let Some(rule) = find_rule(&resolutions, id) {
                    let _ = writeln!(context.out, "{}", render_rule_show(&resolver, rule));
                    CommandResult::success()
                } else {
                    let _ = writeln!(context.err, "{}", render_missing_rule(id));
                    CommandResult::failure()
                }
            }
            Err(error) => {
                let _ = writeln!(context.err, "{}", render_rules_error("show", &error));
                CommandResult::failure()
            }
        },
        Ok(None) => {
            let _ = writeln!(context.err, "{}", render_missing_work("show"));
            CommandResult::failure()
        }
        Err(error) => {
            let _ = writeln!(context.err, "{}", render_work_state_error("show", &error));
            CommandResult::failure()
        }
    };
    finish("work rules show", arguments, context, result)
}

#[derive(Debug, Clone)]
pub struct RuleResolver {
    paths: RapportPaths,
}

impl RuleResolver {
    #[must_use]
    pub fn new(paths: RapportPaths) -> Self {
        Self { paths }
    }

    /// Resolve reviewer-compatible rules for one repository path.
    ///
    /// # Errors
    ///
    /// Returns [`RulesError`] when a rule file cannot be read, parsed, or
    /// resolved without duplicate rule ids.
    pub fn resolve_path(
        &self,
        fs: &impl FileSystem,
        requested_path: &Utf8Path,
    ) -> Result<PathRules, RulesError> {
        let absolute_path = self.absolute_path(requested_path);
        let requested_path = self.repo_display_path(&absolute_path);
        if absolute_path.strip_prefix(self.paths.repo_root()).is_err() {
            return Ok(PathRules::unresolved(
                requested_path,
                UnresolvedReason::OutsideRepository,
            ));
        }

        let Some(owner) = self.find_owner_file(fs, &absolute_path) else {
            return Ok(PathRules::unresolved(
                requested_path,
                UnresolvedReason::NoOwner,
            ));
        };

        let mut loaded_files = BTreeSet::new();
        let mut seen_ids: BTreeMap<String, Utf8PathBuf> = BTreeMap::new();
        let mut rules = Vec::new();
        self.collect_rules_file(fs, &owner, &mut loaded_files, &mut seen_ids, &mut rules)?;

        Ok(PathRules::resolved(requested_path, owner, rules))
    }

    /// Resolve rules for several repository paths as one active-work set.
    ///
    /// # Errors
    ///
    /// Returns [`RulesError`] when a path's rule files cannot be read, parsed,
    /// or when the combined work set exposes duplicate rule ids.
    pub fn resolve_paths<I, P>(
        &self,
        fs: &impl FileSystem,
        paths: I,
    ) -> Result<Vec<PathRules>, RulesError>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Utf8Path>,
    {
        let mut resolutions = Vec::new();
        for path in paths {
            resolutions.push(self.resolve_path(fs, path.as_ref())?);
        }
        Self::validate_unique_rule_ids(&resolutions)?;
        Ok(resolutions)
    }

    #[must_use]
    pub fn display_path(&self, path: &Utf8Path) -> String {
        self.repo_display_path(path).to_string()
    }

    fn absolute_path(&self, path: &Utf8Path) -> Utf8PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.paths.repo_root().join(path)
        }
    }

    fn repo_display_path(&self, path: &Utf8Path) -> Utf8PathBuf {
        match path.strip_prefix(self.paths.repo_root()) {
            Ok(relative) if relative.as_str().is_empty() => Utf8PathBuf::from("."),
            Ok(relative) => relative.to_path_buf(),
            Err(_) => path.to_path_buf(),
        }
    }

    fn find_owner_file(
        &self,
        fs: &impl FileSystem,
        absolute_path: &Utf8Path,
    ) -> Option<Utf8PathBuf> {
        let mut current = if fs.is_dir(absolute_path) {
            absolute_path.to_path_buf()
        } else {
            absolute_path.parent().map_or_else(
                || self.paths.repo_root().to_path_buf(),
                Utf8Path::to_path_buf,
            )
        };

        loop {
            let owner = current.join("rules.toml");
            if fs.is_file(&owner) {
                return Some(owner);
            }
            if current == self.paths.repo_root() || !current.pop() {
                return None;
            }
        }
    }

    fn collect_rules_file(
        &self,
        fs: &impl FileSystem,
        path: &Utf8Path,
        loaded_files: &mut BTreeSet<Utf8PathBuf>,
        seen_ids: &mut BTreeMap<String, Utf8PathBuf>,
        rules: &mut Vec<Rule>,
    ) -> Result<(), RulesError> {
        if !loaded_files.insert(path.to_path_buf()) {
            return Ok(());
        }

        let document = Self::load_document(fs, path)?;
        for include in document.includes {
            let include_path = self.resolve_include(path, &include)?;
            self.collect_rules_file(fs, &include_path, loaded_files, seen_ids, rules)?;
        }
        for rule in document.rules {
            if let Some(first_source) = seen_ids.get(&rule.id) {
                return Err(RulesError::DuplicateRuleId {
                    id: rule.id,
                    first_source: first_source.clone(),
                    second_source: path.to_path_buf(),
                });
            }
            seen_ids.insert(rule.id.clone(), path.to_path_buf());
            rules.push(Rule {
                id: rule.id,
                text: rule.text,
                references: rule.references,
                source: path.to_path_buf(),
            });
        }
        Ok(())
    }

    fn load_document(fs: &impl FileSystem, path: &Utf8Path) -> Result<RuleDocument, RulesError> {
        let contents = fs.read_to_string(path).map_err(|source| RulesError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        toml::from_str(&contents).map_err(|source| RulesError::Decode {
            path: path.to_path_buf(),
            source,
        })
    }

    fn resolve_include(&self, source: &Utf8Path, include: &str) -> Result<Utf8PathBuf, RulesError> {
        let resolved = if let Some(root_relative) = include.strip_prefix('/') {
            self.paths.repo_root().join(root_relative)
        } else {
            source.parent().map_or_else(
                || self.paths.repo_root().join(include),
                |parent| parent.join(include),
            )
        };

        if resolved.strip_prefix(self.paths.repo_root()).is_err() {
            return Err(RulesError::IncludeOutsideRepository {
                include: include.to_string(),
                source: source.to_path_buf(),
            });
        }
        Ok(resolved)
    }

    fn validate_unique_rule_ids(resolutions: &[PathRules]) -> Result<(), RulesError> {
        let mut seen_ids: BTreeMap<String, Utf8PathBuf> = BTreeMap::new();
        for rule in resolutions.iter().flat_map(|resolution| &resolution.rules) {
            if let Some(first_source) = seen_ids.get(&rule.id) {
                if first_source != &rule.source {
                    return Err(RulesError::DuplicateRuleId {
                        id: rule.id.clone(),
                        first_source: first_source.clone(),
                        second_source: rule.source.clone(),
                    });
                }
            } else {
                seen_ids.insert(rule.id.clone(), rule.source.clone());
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathRules {
    pub requested_path: Utf8PathBuf,
    pub owner: Option<Utf8PathBuf>,
    pub rules: Vec<Rule>,
    pub unresolved: Option<UnresolvedReason>,
}

impl PathRules {
    fn resolved(requested_path: Utf8PathBuf, owner: Utf8PathBuf, rules: Vec<Rule>) -> Self {
        Self {
            requested_path,
            owner: Some(owner),
            rules,
            unresolved: None,
        }
    }

    fn unresolved(requested_path: Utf8PathBuf, reason: UnresolvedReason) -> Self {
        Self {
            requested_path,
            owner: None,
            rules: Vec::new(),
            unresolved: Some(reason),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub id: String,
    pub text: String,
    pub references: Vec<String>,
    pub source: Utf8PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnresolvedReason {
    NoOwner,
    OutsideRepository,
}

impl fmt::Display for UnresolvedReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoOwner => f.write_str("no rules owner found"),
            Self::OutsideRepository => f.write_str("path is outside the repository"),
        }
    }
}

#[derive(Debug)]
pub enum RulesError {
    Io {
        path: Utf8PathBuf,
        source: io::Error,
    },
    Decode {
        path: Utf8PathBuf,
        source: toml::de::Error,
    },
    DuplicateRuleId {
        id: String,
        first_source: Utf8PathBuf,
        second_source: Utf8PathBuf,
    },
    IncludeOutsideRepository {
        include: String,
        source: Utf8PathBuf,
    },
}

impl fmt::Display for RulesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "rules filesystem error at `{path}`: {source}")
            }
            Self::Decode { path, source } => {
                write!(f, "rules parse error at `{path}`: {source}")
            }
            Self::DuplicateRuleId {
                id,
                first_source,
                second_source,
            } => write!(
                f,
                "duplicate rule id `{id}` in `{first_source}` and `{second_source}`"
            ),
            Self::IncludeOutsideRepository { include, source } => write!(
                f,
                "include `{include}` from `{source}` resolves outside the repository"
            ),
        }
    }
}

impl Error for RulesError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Decode { source, .. } => Some(source),
            Self::DuplicateRuleId { .. } | Self::IncludeOutsideRepository { .. } => None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleDocument {
    #[serde(default)]
    includes: Vec<String>,
    #[serde(default)]
    rules: Vec<RuleDefinition>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleDefinition {
    id: String,
    text: String,
    #[serde(default)]
    references: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
struct CommandResult {
    outcome: CommandEventOutcome,
    exit_code: u8,
}

impl CommandResult {
    fn success() -> Self {
        Self {
            outcome: CommandEventOutcome::Success,
            exit_code: SUCCESS,
        }
    }

    fn failure() -> Self {
        Self {
            outcome: CommandEventOutcome::Failure,
            exit_code: FAILURE,
        }
    }
}

fn requested_path_from_cwd(path: &Utf8Path, cwd: &Utf8Path) -> Utf8PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

fn active_work_paths(
    fs: &impl FileSystem,
    paths: &RapportPaths,
) -> Result<Option<Vec<Utf8PathBuf>>, WorkStateError> {
    WorkStateStore::new(paths.clone())
        .load(fs)
        .map(|state| state.map(|state| state.paths.into_iter().map(Utf8PathBuf::from).collect()))
}

fn find_rule<'rules>(resolutions: &'rules [PathRules], id: &str) -> Option<&'rules Rule> {
    resolutions
        .iter()
        .flat_map(|resolution| &resolution.rules)
        .find(|rule| rule.id == id)
}

fn finish<F, C, O, E>(
    command: &'static str,
    arguments: Vec<String>,
    context: &mut CommandContext<'_, F, C, O, E>,
    result: CommandResult,
) -> ExitCode
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let event = CommandEvent::new(
        context.clock.now_rfc3339(),
        arguments,
        command,
        result.outcome,
        result.exit_code,
    );
    match TelemetryWriter::new(context.paths.clone()).append(context.fs, &event) {
        Ok(()) => ExitCode::from(result.exit_code),
        Err(error) => {
            let _ = writeln!(context.err, "{}", render_telemetry_error(&error));
            ExitCode::FAILURE
        }
    }
}

fn render_rule_list(resolver: &RuleResolver, resolutions: &[PathRules]) -> String {
    let mut lines = Vec::new();
    for resolution in resolutions {
        match (&resolution.owner, resolution.unresolved) {
            (Some(owner), None) => {
                lines.push(format!(
                    "`{}` -- owner `{}`",
                    resolution.requested_path,
                    resolver.display_path(owner)
                ));
                lines.extend(resolution.rules.iter().map(|rule| {
                    format!(
                        "`{}` -- {} ({})",
                        rule.id,
                        rule.text,
                        resolver.display_path(&rule.source)
                    )
                }));
            }
            (None, Some(reason)) => {
                lines.push(format!(
                    "`{}` -- unresolved: {reason}",
                    resolution.requested_path
                ));
            }
            _ => {
                lines.push(format!(
                    "`{}` -- unresolved: invalid rule resolution",
                    resolution.requested_path
                ));
            }
        }
    }

    ViewBuilder::new()
        .title("rapport work rules list")
        .section("Rules", |b| b.items(lines))
        .next_actions(nonempty![RunHint::new("rapport work rules show <id>")])
        .build()
}

fn render_rule_show(resolver: &RuleResolver, rule: &Rule) -> String {
    let mut details = vec![
        ("id", rule.id.clone()),
        ("source", resolver.display_path(&rule.source)),
    ];
    if !rule.references.is_empty() {
        details.push(("references", rule.references.join(", ")));
    }

    ViewBuilder::new()
        .title("rapport work rules show")
        .section("Rule", |b| b.entries(details))
        .section("Text", |b| b.items([rule.text.clone()]))
        .next_actions(nonempty![RunHint::new("rapport work rules list")])
        .build()
}

fn render_no_active_paths(command: &str) -> String {
    ViewBuilder::new()
        .title(format!("rapport work rules {command}"))
        .paragraph("Active work has no paths.")
        .paragraph("Add a path before asking for current-work rules.")
        .next_actions(nonempty![RunHint::new("rapport work add path <path>")])
        .build()
}

fn render_missing_work(command: &str) -> String {
    ViewBuilder::new()
        .title(format!("rapport work rules {command}"))
        .paragraph("No active work state found.")
        .paragraph("Start work before asking for current-work rules.")
        .next_actions(nonempty![RunHint::new(
            "rapport work start --title \"...\" --path <path>"
        )])
        .build()
}

fn render_missing_rule(id: &str) -> String {
    ViewBuilder::new()
        .title("rapport work rules show")
        .paragraph(format!(
            "Rule `{id}` is not applicable to the current work."
        ))
        .next_actions(nonempty![RunHint::new("rapport work rules list")])
        .build()
}

fn render_work_state_error(command: &str, error: &WorkStateError) -> String {
    ViewBuilder::new()
        .title(format!("rapport work rules {command}"))
        .paragraph("Could not read active work state.")
        .paragraph(error)
        .next_actions(nonempty![RunHint::new(
            "fix .rapport/work.toml or remove it before starting new work"
        )])
        .build()
}

fn render_rules_error(command: &str, error: &RulesError) -> String {
    ViewBuilder::new()
        .title(format!("rapport work rules {command}"))
        .paragraph("Could not resolve repository rules.")
        .paragraph(error)
        .next_actions(nonempty![RunHint::new("rapport work rules list <path>")])
        .build()
}

fn render_telemetry_error(error: &TelemetryError) -> String {
    ViewBuilder::new()
        .title("rapport telemetry")
        .paragraph("Command completed, but telemetry could not be written.")
        .paragraph(error)
        .next_actions(nonempty![RunHint::new("rapport work status")])
        .build()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use rapport_files::InMemoryFileSystem;

    #[test]
    fn resolves_nearest_owner_includes_and_local_rules() {
        let mut fs = repo_fs();
        fs.write_string(
            "/repo/rules/rust.toml",
            r#"
[[rules]]
id = "RUST-ORG-003"
text = "Keep lib.rs small."
"#,
        )
        .unwrap();
        fs.write_string(
            "/repo/crates/rapport/rules.toml",
            r#"
includes = ["/rules/rust.toml"]

[[rules]]
id = "RAPPORT-001"
text = "Keep the CLI boring."
"#,
        )
        .unwrap();
        fs.add_file("/repo/crates/rapport/src/lib.rs");
        let resolver = RuleResolver::new(RapportPaths::new("/repo"));

        let resolution = resolver
            .resolve_path(&fs, Utf8Path::new("crates/rapport/src/lib.rs"))
            .unwrap();

        assert_eq!(
            resolution.owner,
            Some(Utf8PathBuf::from("/repo/crates/rapport/rules.toml"))
        );
        assert_eq!(
            rule_ids(&resolution),
            vec![String::from("RUST-ORG-003"), String::from("RAPPORT-001")]
        );
    }

    #[test]
    fn nearest_owner_wins_without_implicit_parent_inheritance() {
        let mut fs = repo_fs();
        fs.write_string(
            "/repo/rules.toml",
            r#"
[[rules]]
id = "ROOT-001"
text = "Root rule."
"#,
        )
        .unwrap();
        fs.write_string(
            "/repo/crates/rapport/rules.toml",
            r#"
[[rules]]
id = "LOCAL-001"
text = "Local rule."
"#,
        )
        .unwrap();
        let resolver = RuleResolver::new(RapportPaths::new("/repo"));

        let resolution = resolver
            .resolve_path(&fs, Utf8Path::new("crates/rapport/src/lib.rs"))
            .unwrap();

        assert_eq!(rule_ids(&resolution), vec![String::from("LOCAL-001")]);
    }

    #[test]
    fn repeated_includes_are_loaded_once() {
        let mut fs = repo_fs();
        fs.write_string(
            "/repo/rules/rust.toml",
            r#"
[[rules]]
id = "RUST-001"
text = "Rust rule."
"#,
        )
        .unwrap();
        fs.write_string(
            "/repo/rules.toml",
            r#"
includes = ["/rules/rust.toml", "/rules/rust.toml"]
"#,
        )
        .unwrap();
        let resolver = RuleResolver::new(RapportPaths::new("/repo"));

        let resolution = resolver
            .resolve_path(&fs, Utf8Path::new("crates/rapport/src/lib.rs"))
            .unwrap();

        assert_eq!(rule_ids(&resolution), vec![String::from("RUST-001")]);
    }

    #[test]
    fn duplicate_rule_ids_fail_clearly() {
        let mut fs = repo_fs();
        fs.write_string(
            "/repo/rules/a.toml",
            r#"
[[rules]]
id = "DUP-001"
text = "First rule."
"#,
        )
        .unwrap();
        fs.write_string(
            "/repo/rules/b.toml",
            r#"
[[rules]]
id = "DUP-001"
text = "Second rule."
"#,
        )
        .unwrap();
        fs.write_string(
            "/repo/rules.toml",
            r#"
includes = ["/rules/a.toml", "/rules/b.toml"]
"#,
        )
        .unwrap();
        let resolver = RuleResolver::new(RapportPaths::new("/repo"));

        let error = resolver
            .resolve_path(&fs, Utf8Path::new("crates/rapport/src/lib.rs"))
            .unwrap_err();

        assert!(error.to_string().contains("duplicate rule id `DUP-001`"));
    }

    #[test]
    fn missing_owner_reports_unresolved_state() {
        let fs = repo_fs();
        let resolver = RuleResolver::new(RapportPaths::new("/repo"));

        let resolution = resolver
            .resolve_path(&fs, Utf8Path::new("crates/rapport/src/lib.rs"))
            .unwrap();

        assert_eq!(resolution.unresolved, Some(UnresolvedReason::NoOwner));
        assert!(resolution.rules.is_empty());
    }

    #[test]
    fn duplicate_ids_across_current_work_fail_clearly() {
        let mut fs = repo_fs();
        fs.write_string(
            "/repo/crates/one/rules.toml",
            r#"
[[rules]]
id = "LOCAL-001"
text = "One."
"#,
        )
        .unwrap();
        fs.write_string(
            "/repo/crates/two/rules.toml",
            r#"
[[rules]]
id = "LOCAL-001"
text = "Two."
"#,
        )
        .unwrap();
        let resolver = RuleResolver::new(RapportPaths::new("/repo"));

        let error = resolver
            .resolve_paths(
                &fs,
                [
                    Utf8Path::new("crates/one/src/lib.rs"),
                    Utf8Path::new("crates/two/src/lib.rs"),
                ],
            )
            .unwrap_err();

        assert!(error.to_string().contains("duplicate rule id `LOCAL-001`"));
    }

    #[test]
    fn baseline_rule_files_parse() {
        toml::from_str::<RuleDocument>(include_str!("../../../rules.toml")).unwrap();
        toml::from_str::<RuleDocument>(include_str!("../../../rules/rust.toml")).unwrap();
        toml::from_str::<RuleDocument>(include_str!("../../../rules/testing.toml")).unwrap();
    }

    fn repo_fs() -> InMemoryFileSystem {
        let mut fs = InMemoryFileSystem::default();
        fs.add_directory("/repo/.git");
        fs
    }

    fn rule_ids(resolution: &PathRules) -> Vec<String> {
        resolution
            .rules
            .iter()
            .map(|rule| rule.id.clone())
            .collect()
    }
}
