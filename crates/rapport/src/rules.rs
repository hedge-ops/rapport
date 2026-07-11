use crate::context::{Clock, CommandContext};
use crate::paths::RapportPaths;
use crate::project_context::{ProjectContextError, resolved_rules_for_paths};
use crate::state::{WorkStateError, WorkStateStore};
use crate::telemetry::{CommandEvent, CommandEventOutcome, TelemetryError, TelemetryWriter};
use crate::{RunHint, ViewBuilder};
use nonempty::nonempty;
use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::io::Write;
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

pub(crate) fn validate_repository(
    fs: &impl FileSystem,
    paths: &RapportPaths,
) -> RuleRepositoryValidation {
    let (rule_files, local_rule_count, mut problems) =
        match crate::ruleset::Catalog::discover_documents(fs, paths.repo_root()) {
            Ok(catalog) => (
                catalog
                    .entries()
                    .map(|entry| entry.source.clone())
                    .collect(),
                catalog
                    .entries()
                    .map(|entry| entry.document.rules.len())
                    .sum(),
                Vec::new(),
            ),
            Err(error) => (
                Vec::new(),
                0,
                vec![RuleValidationProblem {
                    detail: normalize_problem_detail(&error.to_string()),
                }],
            ),
        };
    if !rule_files.is_empty() {
        let gitignore = paths.repo_root().join(".gitignore");
        match fs.read_to_string(&gitignore) {
            Ok(contents) if contents.contains(".rapport/**") && contents.contains("!.rapport/rules/**") => {}
            Ok(_) => problems.push(RuleValidationProblem { detail: String::from(".gitignore does not preserve .rapport/rules/** as checked-in repository state; run `rapport init`") }),
            Err(source) => problems.push(RuleValidationProblem { detail: format!("could not validate ruleset .gitignore contract at `{gitignore}`: {source}") }),
        }
    }

    RuleRepositoryValidation {
        rule_files,
        local_rule_count,
        problems,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuleRepositoryValidation {
    rule_files: Vec<Utf8PathBuf>,
    local_rule_count: usize,
    problems: Vec<RuleValidationProblem>,
}

impl RuleRepositoryValidation {
    pub(crate) fn rule_file_count(&self) -> usize {
        self.rule_files.len()
    }

    pub(crate) fn local_rule_count(&self) -> usize {
        self.local_rule_count
    }

    pub(crate) fn problem_details(&self) -> impl Iterator<Item = &str> {
        self.problems.iter().map(|problem| problem.detail.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuleValidationProblem {
    detail: String,
}

fn normalize_problem_detail(detail: &str) -> String {
    detail.replace('\\', "/")
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

    /// Resolve context benchmarks for one repository path.
    ///
    /// # Errors
    ///
    /// Returns [`RulesError`] when project context cannot be resolved.
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

        let path = requested_path.to_string();
        let rules = resolved_rules_for_paths(fs, self.paths.repo_root(), &[path])?
            .into_iter()
            .map(|rule| Rule {
                id: rule.id,
                text: rule.text,
                rationale: rule.rationale,
                references: rule.references,
                avoid: rule.avoid,
                prefer: rule.prefer,
                source: Utf8PathBuf::from(rule.source),
            })
            .collect();

        Ok(PathRules::resolved(requested_path, rules))
    }

    /// Resolve context benchmarks for several repository paths as one work set.
    ///
    /// # Errors
    ///
    /// Returns [`RulesError`] when project context cannot be resolved or the
    /// combined work set exposes duplicate benchmark ids.
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
    pub rules: Vec<Rule>,
    pub unresolved: Option<UnresolvedReason>,
}

impl PathRules {
    fn resolved(requested_path: Utf8PathBuf, rules: Vec<Rule>) -> Self {
        Self {
            requested_path,
            rules,
            unresolved: None,
        }
    }

    fn unresolved(requested_path: Utf8PathBuf, reason: UnresolvedReason) -> Self {
        Self {
            requested_path,
            rules: Vec::new(),
            unresolved: Some(reason),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub id: String,
    pub text: String,
    pub rationale: Option<String>,
    pub(crate) references: Vec<crate::ruleset::RuleReference>,
    pub(crate) avoid: crate::ruleset::RuleExample,
    pub(crate) prefer: crate::ruleset::RuleExample,
    pub source: Utf8PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnresolvedReason {
    OutsideRepository,
}

impl fmt::Display for UnresolvedReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutsideRepository => f.write_str("path is outside the repository"),
        }
    }
}

#[derive(Debug)]
pub enum RulesError {
    Context(ProjectContextError),
    DuplicateRuleId {
        id: String,
        first_source: Utf8PathBuf,
        second_source: Utf8PathBuf,
    },
}

impl fmt::Display for RulesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Context(source) => write!(f, "project context error: {source}"),
            Self::DuplicateRuleId {
                id,
                first_source,
                second_source,
            } => write!(
                f,
                "duplicate rule id `{id}` in `{first_source}` and `{second_source}`"
            ),
        }
    }
}

impl Error for RulesError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Context(source) => Some(source),
            Self::DuplicateRuleId { .. } => None,
        }
    }
}

impl From<ProjectContextError> for RulesError {
    fn from(source: ProjectContextError) -> Self {
        Self::Context(source)
    }
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
        if let Some(reason) = resolution.unresolved {
            lines.push(format!(
                "`{}` -- unresolved: {reason}",
                resolution.requested_path
            ));
            continue;
        }
        lines.push(format!("path `{}`", resolution.requested_path));
        lines.extend(resolution.rules.iter().map(|rule| {
            format!(
                "`{}` -- {} ({})",
                rule.id,
                rule.text,
                resolver.display_path(&rule.source)
            )
        }));
    }

    ViewBuilder::new()
        .title("rapport work rules list")
        .section("Benchmarks", |b| b.items(lines))
        .next_actions(nonempty![RunHint::new("rapport work rules show <id>")])
        .build()
}

fn render_rule_show(resolver: &RuleResolver, rule: &Rule) -> String {
    let mut details = vec![
        ("id", rule.id.clone()),
        ("source", resolver.display_path(&rule.source)),
    ];
    if !rule.references.is_empty() {
        details.push((
            "references",
            rule.references
                .iter()
                .map(crate::ruleset::RuleReference::display)
                .collect::<Vec<_>>()
                .join(", "),
        ));
    }
    let mut builder = ViewBuilder::new()
        .title("rapport work rules show")
        .section("Rule", |b| b.entries(details))
        .section("Text", |b| b.items([rule.text.clone()]));
    if let Some(rationale) = &rule.rationale {
        builder = builder.section("Rationale", |b| b.items([rationale.clone()]));
    }
    builder = builder.section("Avoid", |b| {
        b.items([format!(
            "```{}\n{}\n```",
            rule.avoid.language, rule.avoid.text
        )])
    });
    builder = builder.section("Prefer", |b| {
        b.items([format!(
            "```{}\n{}\n```",
            rule.prefer.language, rule.prefer.text
        )])
    });
    builder
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
