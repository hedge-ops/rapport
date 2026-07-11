use crate::cli::{
    ContextCommand, ContextInitArgs, ContextListCommand, ContextOwnershipCommand,
    ContextPurposeCommand, ContextRuleAddArgs, ContextRuleCommand, ContextRuleUpdateArgs,
    ContextSignoffCommand, SignoffKindArg,
};
use crate::context::{Clock, CommandContext};
use crate::repository_files::find_named_files;
use crate::signoff_contract::{self, SignoffKind, SignoffRequest};
use crate::state::{ReviewGrade, ReviewGradeError};
use crate::telemetry::{CommandEvent, CommandEventOutcome, TelemetryError, TelemetryWriter};
use crate::{RunHint, ViewBuilder};
use dprint_plugin_markdown::{
    configuration::{ConfigurationBuilder, TextWrap},
    format_text,
};
use nonempty::nonempty;
use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::fmt::Write as _;
use std::io::{self, Write};
use std::process::ExitCode;

const SUCCESS: u8 = 0;
const FAILURE: u8 = 2;
const CONTEXT_FILE: &str = "context.toml";
const CONTEXT_LINE_WIDTH: u32 = 100;
const CONTEXT_SCHEMA_VERSION: u16 = 1;
const RULE_SCHEMA_VERSION: u16 = 1;

pub fn run<F, C, O, E>(
    command: &ContextCommand,
    arguments: Vec<String>,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> ExitCode
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let (command_name, result) = match command {
        ContextCommand::Show { path } => ("context show", show(path.as_ref(), context)),
        ContextCommand::Init(args) => ("context init", init(args, context)),
        ContextCommand::Purpose(args) => match &args.command {
            ContextPurposeCommand::Set(args) => ("context purpose set", set_purpose(args, context)),
        },
        ContextCommand::Ownership(args) => match &args.command {
            ContextOwnershipCommand::Owns(args) => match &args.command {
                ContextListCommand::Add { path, value } => (
                    "context ownership owns add",
                    add_list_value(path, ContextListTarget::Owns, value, context),
                ),
                ContextListCommand::Remove { path, value } => (
                    "context ownership owns remove",
                    remove_list_value(path, ContextListTarget::Owns, value, context),
                ),
            },
            ContextOwnershipCommand::Boundary(args) => match &args.command {
                ContextListCommand::Add { path, value } => (
                    "context ownership boundary add",
                    add_list_value(path, ContextListTarget::Boundary, value, context),
                ),
                ContextListCommand::Remove { path, value } => (
                    "context ownership boundary remove",
                    remove_list_value(path, ContextListTarget::Boundary, value, context),
                ),
            },
        },
        ContextCommand::Rule(args) => match &args.command {
            ContextRuleCommand::Include(args) => match &args.command {
                ContextListCommand::Add { path, value } => (
                    "context rule include add",
                    add_list_value(path, ContextListTarget::RuleInclude, value, context),
                ),
                ContextListCommand::Remove { path, value } => (
                    "context rule include remove",
                    remove_list_value(path, ContextListTarget::RuleInclude, value, context),
                ),
            },
            ContextRuleCommand::Add(args) => ("context rule add", add_rule(args, context)),
            ContextRuleCommand::Update(args) => ("context rule update", update_rule(args, context)),
            ContextRuleCommand::Remove { path, id } => {
                ("context rule remove", remove_rule(path, id, context))
            }
        },
        ContextCommand::Signoff(args) => match &args.command {
            ContextSignoffCommand::Add {
                path,
                kind,
                target,
                minimum_grade,
            } => (
                "context signoff add",
                add_signoff(
                    path,
                    *kind,
                    target.as_deref(),
                    minimum_grade.as_deref(),
                    context,
                ),
            ),
            ContextSignoffCommand::Remove { path, kind, target } => (
                "context signoff remove",
                remove_signoff(path, *kind, target.as_deref(), context),
            ),
            ContextSignoffCommand::Repair { path, kind, target } => (
                "context signoff repair",
                repair_signoff(path, *kind, target.as_deref(), context),
            ),
        },
        ContextCommand::Doctor { path } => ("context doctor", doctor(path.as_ref(), context)),
    };
    finish(command_name, arguments, context, result)
}

pub(crate) fn validate_repository(
    fs: &impl FileSystem,
    repo_root: &Utf8Path,
) -> ProjectContextRepositoryValidation {
    let context_files = match find_named_files(fs, repo_root, CONTEXT_FILE) {
        Ok(context_files) => context_files,
        Err(source) => {
            return ProjectContextRepositoryValidation {
                context_files: Vec::new(),
                signoff_count: 0,
                problems: vec![ProjectContextValidationProblem {
                    detail: format!(
                        "could not scan repository for `{CONTEXT_FILE}` files at `{repo_root}`: {source}"
                    ),
                }],
            };
        }
    };

    let store = ProjectContextStore::new(repo_root.to_path_buf());
    let resolver = ProjectContextResolver::new(store);
    let mut seen_problems = BTreeSet::new();
    let mut problems = Vec::new();

    for context_file in &context_files {
        let context_directory = context_file.parent().unwrap_or(repo_root);
        if let Err(error) = resolver.resolve(fs, context_directory) {
            let detail = normalize_problem_detail(&error.to_string());
            if seen_problems.insert(detail.clone()) {
                problems.push(ProjectContextValidationProblem { detail });
            }
        }
    }

    let mut requests = Vec::new();
    for context_file in &context_files {
        let Ok(document) = ProjectContextStore::load_context_file(fs, context_file) else {
            continue;
        };
        let directory = context_file.parent().unwrap_or(repo_root);
        for declaration in document.signoffs {
            match declaration.to_request(repo_root, directory) {
                Ok(request) => requests.push(request),
                Err(error) => {
                    let detail = normalize_problem_detail(&format!(
                        "invalid signoff declaration in `{}`: {error}",
                        context_file.strip_prefix(repo_root).unwrap_or(context_file)
                    ));
                    if seen_problems.insert(detail.clone()) {
                        problems.push(ProjectContextValidationProblem { detail });
                    }
                }
            }
        }
    }
    for detail in signoff_contract::validate(fs, repo_root, &requests) {
        let detail = normalize_problem_detail(&detail);
        if seen_problems.insert(detail.clone()) {
            problems.push(ProjectContextValidationProblem { detail });
        }
    }

    ProjectContextRepositoryValidation {
        context_files,
        signoff_count: requests.len(),
        problems,
    }
}

#[cfg(test)]
pub(crate) fn required_signoffs_for_paths(
    fs: &impl FileSystem,
    repo_root: &Utf8Path,
    paths: &[String],
) -> Result<Vec<String>, ProjectContextError> {
    required_signoff_requests_for_paths(fs, repo_root, paths).map(|requests| {
        requests
            .into_iter()
            .map(|request| request.qualified_target().to_string())
            .collect()
    })
}

#[cfg(test)]
pub(crate) fn required_signoff_requests_for_paths(
    fs: &impl FileSystem,
    repo_root: &Utf8Path,
    paths: &[String],
) -> Result<Vec<SignoffRequest>, ProjectContextError> {
    required_signoff_requirements_for_paths(fs, repo_root, paths).map(|requirements| {
        requirements
            .into_iter()
            .map(|requirement| requirement.request)
            .collect()
    })
}

pub(crate) fn required_signoff_requirements_for_paths(
    fs: &impl FileSystem,
    repo_root: &Utf8Path,
    paths: &[String],
) -> Result<Vec<SignoffRequirement>, ProjectContextError> {
    let resolver = ProjectContextResolver::new(ProjectContextStore::new(repo_root.to_path_buf()));
    let mut required: Vec<SignoffRequirement> = Vec::new();

    for path in paths {
        let effective = resolver.resolve(fs, &repo_root.join(path))?;
        for signoff in effective.signoffs {
            let directory = signoff.source.parent().unwrap_or(repo_root);
            let request = signoff
                .declaration
                .to_request(repo_root, directory)
                .map_err(|source| ProjectContextError::SignoffContract { source })?;
            if let Some(existing) = required
                .iter_mut()
                .find(|existing| existing.request.qualified_target() == request.qualified_target())
            {
                if !existing.paths.contains(path) {
                    existing.paths.push(path.clone());
                }
            } else {
                required.push(SignoffRequirement {
                    request,
                    paths: vec![path.clone()],
                });
            }
        }
    }

    Ok(required)
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct SignoffRequirement {
    pub(crate) request: SignoffRequest,
    pub(crate) paths: Vec<String>,
}

impl fmt::Debug for SignoffRequirement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SignoffRequirement")
            .field("request", &self.request)
            .field("path_count", &self.paths.len())
            .finish()
    }
}

pub(crate) fn resolved_rules_for_paths(
    fs: &impl FileSystem,
    repo_root: &Utf8Path,
    paths: &[String],
) -> Result<Vec<ResolvedContextRule>, ProjectContextError> {
    let resolver = ProjectContextResolver::new(ProjectContextStore::new(repo_root.to_path_buf()));
    let mut rules: Vec<ResolvedContextRule> = Vec::new();
    let mut seen: BTreeMap<String, Utf8PathBuf> = BTreeMap::new();
    for path in paths {
        let effective = resolver.resolve(fs, &repo_root.join(path))?;
        for rule in effective.rules {
            if let Some(first_source) = seen.get(&rule.id) {
                if first_source != &rule.source {
                    return Err(ProjectContextError::DuplicateRuleId {
                        id: rule.id,
                        first_source: first_source.clone(),
                        second_source: rule.source,
                    });
                }
                continue;
            }
            seen.insert(rule.id.clone(), rule.source.clone());
            rules.push(ResolvedContextRule {
                id: rule.id,
                text: rule.text,
                rationale: rule.rationale,
                references: rule.references,
                source: rule
                    .source
                    .strip_prefix(repo_root)
                    .unwrap_or(&rule.source)
                    .as_str()
                    .replace('\\', "/"),
            });
        }
    }
    Ok(rules)
}

#[derive(Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct ResolvedContextRule {
    pub(crate) id: String,
    pub(crate) text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) rationale: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) references: Vec<String>,
    pub(crate) source: String,
}

impl fmt::Debug for ResolvedContextRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResolvedContextRule")
            .field("id", &RedactedContextText(&self.id))
            .field("text", &RedactedContextText(&self.text))
            .field("has_rationale", &self.rationale.is_some())
            .field("reference_count", &self.references.len())
            .field("source", &RedactedContextText(&self.source))
            .finish()
    }
}

struct RedactedContextText<'a>(&'a str);

impl fmt::Debug for RedactedContextText<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<redacted; {} bytes>", self.0.len())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectContextRepositoryValidation {
    context_files: Vec<Utf8PathBuf>,
    signoff_count: usize,
    problems: Vec<ProjectContextValidationProblem>,
}

impl ProjectContextRepositoryValidation {
    pub(crate) fn context_file_count(&self) -> usize {
        self.context_files.len()
    }

    pub(crate) fn signoff_count(&self) -> usize {
        self.signoff_count
    }

    pub(crate) fn problem_details(&self) -> impl Iterator<Item = &str> {
        self.problems.iter().map(|problem| problem.detail.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjectContextValidationProblem {
    detail: String,
}

fn normalize_problem_detail(detail: &str) -> String {
    detail.replace('\\', "/")
}

fn show<F, C, O, E>(
    path: Option<&Utf8PathBuf>,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> CommandResult
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let store = ProjectContextStore::new(context.repo_root.clone());
    let resolver = ProjectContextResolver::new(store);
    let requested_path = requested_path_from_cwd(path, &context.cwd);
    match resolver.resolve(context.fs, &requested_path) {
        Ok(effective) => {
            let _ = writeln!(
                context.out,
                "{}",
                render_context_show(&resolver, &effective)
            );
            CommandResult::success()
        }
        Err(error) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_context_error("show", "Could not resolve project context.", &error)
            );
            CommandResult::failure()
        }
    }
}

fn init<F, C, O, E>(
    args: &ContextInitArgs,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> CommandResult
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let purpose = match required_text("purpose", &args.purpose) {
        Ok(purpose) => purpose,
        Err(error) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_context_error("init", "Could not create project context.", &error)
            );
            return CommandResult::failure();
        }
    };
    let store = ProjectContextStore::new(context.repo_root.clone());
    let requested_path = requested_path_from_cwd(Some(&args.path), &context.cwd);
    match store.init(context.fs, &requested_path, purpose) {
        Ok(report) => {
            let _ = writeln!(context.out, "{}", render_edit("init", &store, &report));
            CommandResult::success()
        }
        Err(error) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_context_error("init", "Could not create project context.", &error)
            );
            CommandResult::failure()
        }
    }
}

fn set_purpose<F, C, O, E>(
    args: &crate::cli::ContextPurposeSetArgs,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> CommandResult
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let purpose = match required_text("purpose", &args.purpose) {
        Ok(purpose) => purpose,
        Err(error) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_context_error("purpose set", "Could not update project context.", &error)
            );
            return CommandResult::failure();
        }
    };
    let store = ProjectContextStore::new(context.repo_root.clone());
    let requested_path = requested_path_from_cwd(Some(&args.path), &context.cwd);
    match store.mutate(context.fs, &requested_path, |document| {
        document.purpose = purpose.to_string();
        Ok(EditStatus::Updated)
    }) {
        Ok(report) => {
            let _ = writeln!(
                context.out,
                "{}",
                render_edit("purpose set", &store, &report)
            );
            CommandResult::success()
        }
        Err(error) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_context_error("purpose set", "Could not update project context.", &error)
            );
            CommandResult::failure()
        }
    }
}

fn add_signoff<F, C, O, E>(
    path: &Utf8PathBuf,
    kind: SignoffKindArg,
    target: Option<&str>,
    minimum_grade: Option<&str>,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> CommandResult
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let kind = signoff_kind(kind);
    let target = match canonical_signoff_target(kind, target) {
        Ok(target) => target,
        Err(error) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_context_error(
                    "signoff add",
                    "Could not update the signoff contract.",
                    &error,
                )
            );
            return CommandResult::failure();
        }
    };
    let minimum_grade = match minimum_grade.map(str::parse).transpose() {
        Ok(grade) => grade,
        Err(error) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_context_error(
                    "signoff add",
                    "Could not update the signoff contract.",
                    &ProjectContextError::InvalidReviewGrade(error),
                )
            );
            return CommandResult::failure();
        }
    };
    edit_signoff(path, kind, target, minimum_grade, SignoffEdit::Add, context)
}

fn remove_signoff<F, C, O, E>(
    path: &Utf8PathBuf,
    kind: SignoffKindArg,
    target: Option<&str>,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> CommandResult
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let kind = signoff_kind(kind);
    let target = match canonical_signoff_target(kind, target) {
        Ok(target) => target,
        Err(error) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_context_error(
                    "signoff remove",
                    "Could not update the signoff contract.",
                    &error,
                )
            );
            return CommandResult::failure();
        }
    };
    edit_signoff(path, kind, target, None, SignoffEdit::Remove, context)
}

fn repair_signoff<F, C, O, E>(
    path: &Utf8PathBuf,
    kind: SignoffKindArg,
    target: Option<&str>,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> CommandResult
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let kind = signoff_kind(kind);
    let target = match canonical_signoff_target(kind, target) {
        Ok(target) => target,
        Err(error) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_context_error(
                    "signoff repair",
                    "Could not update the signoff contract.",
                    &error,
                )
            );
            return CommandResult::failure();
        }
    };
    edit_signoff(path, kind, target, None, SignoffEdit::Repair, context)
}

fn canonical_signoff_target(
    kind: SignoffKind,
    target: Option<&str>,
) -> Result<&str, ProjectContextError> {
    match (kind, target) {
        (SignoffKind::Build, Some(target)) => Ok(target),
        (SignoffKind::Build, None) => Err(ProjectContextError::MissingBuildTarget),
        (SignoffKind::Review, None) => Ok("review"),
        (SignoffKind::Review, Some(_)) => Err(ProjectContextError::UnexpectedReviewTarget),
    }
}

fn signoff_kind(kind: SignoffKindArg) -> SignoffKind {
    match kind {
        SignoffKindArg::Build => SignoffKind::Build,
        SignoffKindArg::Review => SignoffKind::Review,
    }
}

#[derive(Debug, Clone, Copy)]
enum SignoffEdit {
    Add,
    Remove,
    Repair,
}

impl SignoffEdit {
    fn command(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Remove => "remove",
            Self::Repair => "repair",
        }
    }
}

fn edit_signoff<F, C, O, E>(
    path: &Utf8PathBuf,
    kind: SignoffKind,
    target: &str,
    minimum_grade: Option<ReviewGrade>,
    edit: SignoffEdit,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> CommandResult
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let store = ProjectContextStore::new(context.repo_root.clone());
    let requested_path = requested_path_from_cwd(Some(path), &context.cwd);
    let result = apply_signoff_edit(
        context.fs,
        context.paths.repo_root(),
        &store,
        &requested_path,
        SignoffSelection {
            kind,
            target,
            minimum_grade,
        },
        edit,
    );

    match result {
        Ok((context_file, request)) => {
            let _ = writeln!(
                context.out,
                "{}",
                render_signoff_edit(edit.command(), &store, &context_file, &request)
            );
            CommandResult::success()
        }
        Err(error) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_context_error(
                    &format!("signoff {}", edit.command()),
                    "Could not update the signoff contract.",
                    &error,
                )
            );
            CommandResult::failure()
        }
    }
}

#[derive(Clone, Copy)]
struct SignoffSelection<'selection> {
    kind: SignoffKind,
    target: &'selection str,
    minimum_grade: Option<ReviewGrade>,
}

fn apply_signoff_edit(
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
    store: &ProjectContextStore,
    requested_path: &Utf8Path,
    selection: SignoffSelection<'_>,
    edit: SignoffEdit,
) -> Result<(Utf8PathBuf, SignoffRequest), ProjectContextError> {
    let context_file = store.context_file_for_path(fs, requested_path)?;
    if !fs.is_file(&context_file) {
        return Err(ProjectContextError::MissingContext { path: context_file });
    }
    let directory = context_file.parent().unwrap_or(repo_root);
    let request = SignoffRequest::new(
        repo_root,
        directory,
        selection.kind,
        selection.target,
        selection.minimum_grade,
    )
    .map_err(|source| ProjectContextError::SignoffContract { source })?;
    let target = request.target();
    match edit {
        SignoffEdit::Add => {
            apply_signoff_add(fs, repo_root, store, requested_path, target, &request)?;
        }
        SignoffEdit::Remove => {
            apply_signoff_remove(fs, store, requested_path, target, &request)?;
        }
        SignoffEdit::Repair => {
            store.mutate(fs, requested_path, |document| {
                if !document
                    .signoffs
                    .iter()
                    .any(|value| value.matches(selection.kind, target))
                {
                    return Err(ProjectContextError::MissingSignoff {
                        kind: request.kind(),
                        target: (request.kind() == SignoffKind::Build).then(|| target.to_string()),
                    });
                }
                Ok(EditStatus::Updated)
            })?;
            write_signoff_workflows(fs, repo_root, &request)?;
        }
    }
    Ok((context_file, request))
}

fn ensure_context_signoff_identities_available(
    fs: &impl FileSystem,
    repo_root: &Utf8Path,
    requested_context_file: &Utf8Path,
    requested_document: &ProjectContextFile,
) -> Result<(), ProjectContextError> {
    let context_files = find_named_files(fs, repo_root, CONTEXT_FILE).map_err(|source| {
        ProjectContextError::Io {
            path: repo_root.to_path_buf(),
            source,
        }
    })?;
    let mut seen = BTreeMap::<String, Utf8PathBuf>::new();
    for context_file in context_files {
        let loaded;
        let document = if context_file == requested_context_file {
            requested_document
        } else {
            loaded = ProjectContextStore::load_context_file(fs, &context_file)?;
            &loaded
        };
        let directory = context_file.parent().unwrap_or(repo_root);
        for declaration in &document.signoffs {
            let existing = declaration
                .to_request(repo_root, directory)
                .map_err(|source| ProjectContextError::SignoffContract { source })?;
            let identity = existing.qualified_target().to_string();
            if let Some(existing_context) = seen.insert(identity.clone(), context_file.clone()) {
                return Err(ProjectContextError::SignoffIdentityCollision {
                    identity,
                    existing_context,
                    requested_context: context_file,
                });
            }
        }
    }
    Ok(())
}

fn apply_signoff_add(
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
    store: &ProjectContextStore,
    requested_path: &Utf8Path,
    target: &str,
    request: &SignoffRequest,
) -> Result<(), ProjectContextError> {
    store.mutate(fs, requested_path, |document| {
        if document
            .signoffs
            .iter()
            .any(|value| value.matches(request.kind(), target))
        {
            return Ok(EditStatus::Unchanged);
        }
        document.signoffs.push(ContextSignoffDeclaration::typed(
            request.kind(),
            target,
            request.minimum_grade(),
        ));
        Ok(EditStatus::Added)
    })?;
    write_signoff_workflows(fs, repo_root, request)
}

fn apply_signoff_remove(
    fs: &mut impl FileSystem,
    store: &ProjectContextStore,
    requested_path: &Utf8Path,
    target: &str,
    request: &SignoffRequest,
) -> Result<(), ProjectContextError> {
    store.mutate(fs, requested_path, |document| {
        let Some(index) = document
            .signoffs
            .iter()
            .position(|value| value.matches(request.kind(), target))
        else {
            return Err(ProjectContextError::MissingSignoff {
                kind: request.kind(),
                target: (request.kind() == SignoffKind::Build).then(|| target.to_string()),
            });
        };
        document.signoffs.remove(index);
        Ok(EditStatus::Removed)
    })?;
    if fs.is_file(request.workflow_path()) {
        fs.remove_file(request.workflow_path())
            .map_err(|source| ProjectContextError::Io {
                path: request.workflow_path().to_path_buf(),
                source,
            })?;
    }
    Ok(())
}

fn write_signoff_workflows(
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
    request: &SignoffRequest,
) -> Result<(), ProjectContextError> {
    signoff_contract::write_shared(fs, repo_root).map_err(|source| ProjectContextError::Io {
        path: repo_root.join(signoff_contract::SHARED_WORKFLOW),
        source,
    })?;
    signoff_contract::write_request(fs, repo_root, request).map_err(|source| {
        ProjectContextError::Io {
            path: request.workflow_path().to_path_buf(),
            source,
        }
    })
}

fn add_list_value<F, C, O, E>(
    path: &Utf8Path,
    target: ContextListTarget,
    value: &str,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> CommandResult
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let value = match required_text(target.field_name(), value) {
        Ok(value) => value,
        Err(error) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_context_error(
                    target.add_command(),
                    "Could not update project context.",
                    &error
                )
            );
            return CommandResult::failure();
        }
    };
    let store = ProjectContextStore::new(context.repo_root.clone());
    let requested_path = requested_path_from_cwd(Some(&path.to_path_buf()), &context.cwd);
    match store.mutate(context.fs, &requested_path, |document| {
        let values = target.values_mut(document);
        if values.iter().any(|existing| existing == value) {
            Ok(EditStatus::Unchanged)
        } else {
            values.push(value.to_string());
            Ok(EditStatus::Added)
        }
    }) {
        Ok(report) => {
            let _ = writeln!(
                context.out,
                "{}",
                render_edit(target.add_command(), &store, &report)
            );
            CommandResult::success()
        }
        Err(error) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_context_error(
                    target.add_command(),
                    "Could not update project context.",
                    &error
                )
            );
            CommandResult::failure()
        }
    }
}

fn remove_list_value<F, C, O, E>(
    path: &Utf8Path,
    target: ContextListTarget,
    value: &str,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> CommandResult
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let value = match required_text(target.field_name(), value) {
        Ok(value) => value,
        Err(error) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_context_error(
                    target.remove_command(),
                    "Could not update project context.",
                    &error
                )
            );
            return CommandResult::failure();
        }
    };
    let store = ProjectContextStore::new(context.repo_root.clone());
    let requested_path = requested_path_from_cwd(Some(&path.to_path_buf()), &context.cwd);
    match store.mutate(context.fs, &requested_path, |document| {
        let values = target.values_mut(document);
        let original_len = values.len();
        values.retain(|existing| existing != value);
        if values.len() == original_len {
            Ok(EditStatus::Unchanged)
        } else {
            Ok(EditStatus::Removed)
        }
    }) {
        Ok(report) => {
            let _ = writeln!(
                context.out,
                "{}",
                render_edit(target.remove_command(), &store, &report)
            );
            CommandResult::success()
        }
        Err(error) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_context_error(
                    target.remove_command(),
                    "Could not update project context.",
                    &error
                )
            );
            CommandResult::failure()
        }
    }
}

fn add_rule<F, C, O, E>(
    args: &ContextRuleAddArgs,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> CommandResult
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let id = match required_text("id", &args.id) {
        Ok(id) => id,
        Err(error) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_context_error("rule add", "Could not update project context.", &error)
            );
            return CommandResult::failure();
        }
    };
    let text = match required_text("text", &args.text) {
        Ok(text) => text,
        Err(error) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_context_error("rule add", "Could not update project context.", &error)
            );
            return CommandResult::failure();
        }
    };
    let references = match validate_references(&args.references) {
        Ok(references) => references,
        Err(error) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_context_error("rule add", "Could not update project context.", &error)
            );
            return CommandResult::failure();
        }
    };
    let rationale = args
        .rationale
        .as_deref()
        .and_then(|value| required_text("rationale", value).ok())
        .map(ToString::to_string);
    let store = ProjectContextStore::new(context.repo_root.clone());
    let requested_path = requested_path_from_cwd(Some(&args.path), &context.cwd);
    match store.mutate(context.fs, &requested_path, |document| {
        if document.rules.iter().any(|rule| rule.id == id) {
            return Err(ProjectContextError::DuplicateLocalRuleId { id: id.to_string() });
        }
        document.rules.push(ContextRuleDefinition {
            id: id.to_string(),
            text: text.to_string(),
            rationale: rationale.clone(),
            references: references.clone(),
        });
        Ok(EditStatus::Added)
    }) {
        Ok(report) => {
            let _ = writeln!(context.out, "{}", render_edit("rule add", &store, &report));
            CommandResult::success()
        }
        Err(error) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_context_error("rule add", "Could not update project context.", &error)
            );
            CommandResult::failure()
        }
    }
}

fn update_rule<F, C, O, E>(
    args: &ContextRuleUpdateArgs,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> CommandResult
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let id = match required_text("id", &args.id) {
        Ok(id) => id,
        Err(error) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_context_error("rule update", "Could not update project context.", &error)
            );
            return CommandResult::failure();
        }
    };
    let text = match required_text("text", &args.text) {
        Ok(text) => text,
        Err(error) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_context_error("rule update", "Could not update project context.", &error)
            );
            return CommandResult::failure();
        }
    };
    let rationale = args
        .rationale
        .as_deref()
        .and_then(|value| required_text("rationale", value).ok())
        .map(ToString::to_string);
    let store = ProjectContextStore::new(context.repo_root.clone());
    let requested_path = requested_path_from_cwd(Some(&args.path), &context.cwd);
    match store.mutate(context.fs, &requested_path, |document| {
        let Some(rule) = document.rules.iter_mut().find(|rule| rule.id == id) else {
            return Err(ProjectContextError::MissingInlineRule { id: id.to_string() });
        };
        rule.text = text.to_string();
        if let Some(rationale) = &rationale {
            rule.rationale = Some(rationale.clone());
        }
        Ok(EditStatus::Updated)
    }) {
        Ok(report) => {
            let _ = writeln!(
                context.out,
                "{}",
                render_edit("rule update", &store, &report)
            );
            CommandResult::success()
        }
        Err(error) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_context_error("rule update", "Could not update project context.", &error)
            );
            CommandResult::failure()
        }
    }
}

fn remove_rule<F, C, O, E>(
    path: &Utf8Path,
    id: &str,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> CommandResult
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let id = match required_text("id", id) {
        Ok(id) => id,
        Err(error) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_context_error("rule remove", "Could not update project context.", &error)
            );
            return CommandResult::failure();
        }
    };
    let store = ProjectContextStore::new(context.repo_root.clone());
    let requested_path = requested_path_from_cwd(Some(&path.to_path_buf()), &context.cwd);
    match store.mutate(context.fs, &requested_path, |document| {
        let original_len = document.rules.len();
        document.rules.retain(|rule| rule.id != id);
        if document.rules.len() == original_len {
            Err(ProjectContextError::MissingInlineRule { id: id.to_string() })
        } else {
            Ok(EditStatus::Removed)
        }
    }) {
        Ok(report) => {
            let _ = writeln!(
                context.out,
                "{}",
                render_edit("rule remove", &store, &report)
            );
            CommandResult::success()
        }
        Err(error) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_context_error("rule remove", "Could not update project context.", &error)
            );
            CommandResult::failure()
        }
    }
}

fn doctor<F, C, O, E>(
    path: Option<&Utf8PathBuf>,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> CommandResult
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let store = ProjectContextStore::new(context.repo_root.clone());
    let resolver = ProjectContextResolver::new(store);
    let requested_path = requested_path_from_cwd(path, &context.cwd);
    match resolver.resolve(context.fs, &requested_path) {
        Ok(effective) => {
            let _ = writeln!(
                context.out,
                "{}",
                render_context_doctor(&resolver, &effective)
            );
            CommandResult::success()
        }
        Err(error) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_context_error("doctor", "Context validation failed.", &error)
            );
            CommandResult::failure()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjectContextStore {
    repo_root: Utf8PathBuf,
}

impl ProjectContextStore {
    fn new(repo_root: Utf8PathBuf) -> Self {
        Self { repo_root }
    }

    fn init(
        &self,
        fs: &mut impl FileSystem,
        path: &Utf8Path,
        purpose: &str,
    ) -> Result<EditReport, ProjectContextError> {
        let context_file = self.context_file_for_path(fs, path)?;
        if fs.is_file(&context_file) {
            return Err(ProjectContextError::ContextAlreadyExists { path: context_file });
        }
        let document = ProjectContextFile::new(purpose);
        Self::save_context_file(fs, &context_file, &document)?;
        Ok(EditReport {
            context_file,
            status: EditStatus::Created,
        })
    }

    fn mutate(
        &self,
        fs: &mut impl FileSystem,
        path: &Utf8Path,
        mutation: impl FnOnce(&mut ProjectContextFile) -> Result<EditStatus, ProjectContextError>,
    ) -> Result<EditReport, ProjectContextError> {
        let context_file = self.context_file_for_path(fs, path)?;
        if !fs.is_file(&context_file) {
            return Err(ProjectContextError::MissingContext { path: context_file });
        }
        let mut document = Self::load_context_file(fs, &context_file)?;
        let directory = context_file.parent().unwrap_or(&self.repo_root);
        let legacy_requests = document
            .signoffs
            .iter()
            .filter(|declaration| matches!(declaration, ContextSignoffDeclaration::LegacyBuild(_)))
            .map(|declaration| {
                declaration
                    .to_request(&self.repo_root, directory)
                    .map_err(|source| ProjectContextError::SignoffContract { source })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let status = mutation(&mut document)?;
        ensure_context_signoff_identities_available(fs, &self.repo_root, &context_file, &document)?;
        Self::save_context_file(fs, &context_file, &document)?;
        for request in legacy_requests {
            if document
                .signoffs
                .iter()
                .any(|declaration| declaration.matches(request.kind(), request.target()))
            {
                write_signoff_workflows(fs, &self.repo_root, &request)?;
            }
            let legacy_path = request.legacy_workflow_path(&self.repo_root);
            if fs.is_file(&legacy_path)
                && !workflow_is_owned_by_declared_signoff(fs, &self.repo_root, &legacy_path)?
            {
                fs.remove_file(&legacy_path)
                    .map_err(|source| ProjectContextError::Io {
                        path: legacy_path,
                        source,
                    })?;
            }
        }
        Ok(EditReport {
            context_file,
            status,
        })
    }

    fn load_context_file(
        fs: &impl FileSystem,
        path: &Utf8Path,
    ) -> Result<ProjectContextFile, ProjectContextError> {
        let contents = fs
            .read_to_string(path)
            .map_err(|source| ProjectContextError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        let mut document = toml::from_str::<ProjectContextFile>(&contents).map_err(|source| {
            ProjectContextError::Decode {
                path: path.to_path_buf(),
                source,
            }
        })?;
        if document.version != CONTEXT_SCHEMA_VERSION {
            return Err(ProjectContextError::UnsupportedSchemaVersion {
                path: path.to_path_buf(),
                version: document.version,
            });
        }
        document.normalize();
        Ok(document)
    }

    fn save_context_file(
        fs: &mut impl FileSystem,
        path: &Utf8Path,
        document: &ProjectContextFile,
    ) -> Result<(), ProjectContextError> {
        let rendered = render_context_file(document)?;
        fs.write_string(path, rendered)
            .map_err(|source| ProjectContextError::Io {
                path: path.to_path_buf(),
                source,
            })
    }

    fn context_file_for_path(
        &self,
        fs: &impl FileSystem,
        path: &Utf8Path,
    ) -> Result<Utf8PathBuf, ProjectContextError> {
        Ok(self.context_directory(fs, path)?.join(CONTEXT_FILE))
    }

    fn context_directory(
        &self,
        fs: &impl FileSystem,
        path: &Utf8Path,
    ) -> Result<Utf8PathBuf, ProjectContextError> {
        let absolute_path = self.absolute_path(path);
        if absolute_path.strip_prefix(&self.repo_root).is_err() {
            return Err(ProjectContextError::OutsideRepository {
                path: absolute_path,
            });
        }
        if fs.is_file(&absolute_path) {
            Ok(absolute_path
                .parent()
                .map_or_else(|| self.repo_root.clone(), Utf8Path::to_path_buf))
        } else {
            Ok(absolute_path)
        }
    }

    fn context_files_to_directory(
        &self,
        directory: &Utf8Path,
    ) -> Result<Vec<Utf8PathBuf>, ProjectContextError> {
        let relative = directory.strip_prefix(&self.repo_root).map_err(|_| {
            ProjectContextError::OutsideRepository {
                path: directory.to_path_buf(),
            }
        })?;
        let mut context_files = Vec::new();
        let mut current = self.repo_root.clone();
        context_files.push(current.join(CONTEXT_FILE));
        for component in relative {
            if component.is_empty() || component == "." {
                continue;
            }
            current.push(component);
            context_files.push(current.join(CONTEXT_FILE));
        }
        Ok(context_files)
    }

    fn resolve_include(
        &self,
        source: &Utf8Path,
        include: &str,
    ) -> Result<Utf8PathBuf, ProjectContextError> {
        let resolved = if let Some(root_relative) = include.strip_prefix('/') {
            self.repo_root.join(root_relative)
        } else {
            source.parent().map_or_else(
                || self.repo_root.join(include),
                |parent| parent.join(include),
            )
        };

        if resolved.strip_prefix(&self.repo_root).is_err() {
            return Err(ProjectContextError::IncludeOutsideRepository {
                include: include.to_string(),
                source: source.to_path_buf(),
            });
        }
        Ok(resolved)
    }

    fn absolute_path(&self, path: &Utf8Path) -> Utf8PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.repo_root.join(path)
        }
    }

    fn display_path(&self, path: &Utf8Path) -> String {
        let display = path.strip_prefix(&self.repo_root).unwrap_or(path);
        if display.as_str().is_empty() {
            String::from(".")
        } else {
            display.to_string().replace('\\', "/")
        }
    }
}

fn workflow_is_owned_by_declared_signoff(
    fs: &impl FileSystem,
    repo_root: &Utf8Path,
    workflow_path: &Utf8Path,
) -> Result<bool, ProjectContextError> {
    let context_files = find_named_files(fs, repo_root, CONTEXT_FILE).map_err(|source| {
        ProjectContextError::Io {
            path: repo_root.to_path_buf(),
            source,
        }
    })?;
    for context_file in context_files {
        let document = ProjectContextStore::load_context_file(fs, &context_file)?;
        let directory = context_file.parent().unwrap_or(repo_root);
        for declaration in document.signoffs {
            let request = declaration
                .to_request(repo_root, directory)
                .map_err(|source| ProjectContextError::SignoffContract { source })?;
            if request.workflow_path() == workflow_path {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[derive(Debug, Clone)]
struct ProjectContextResolver {
    store: ProjectContextStore,
}

impl ProjectContextResolver {
    fn new(store: ProjectContextStore) -> Self {
        Self { store }
    }

    fn resolve(
        &self,
        fs: &impl FileSystem,
        path: &Utf8Path,
    ) -> Result<EffectiveProjectContext, ProjectContextError> {
        let target_directory = self.store.context_directory(fs, path)?;
        let mut effective = EffectiveProjectContext::new(target_directory.clone());
        let context_files = self.store.context_files_to_directory(&target_directory)?;
        let mut loaded_rule_libraries = BTreeSet::new();
        let mut seen_rule_ids = BTreeMap::new();

        for context_file in context_files {
            if !fs.is_file(&context_file) {
                continue;
            }
            let document = ProjectContextStore::load_context_file(fs, &context_file)?;
            effective.context_files.push(context_file.clone());
            effective.purpose = Some(ContextEntry::new(document.purpose, context_file.clone()));
            effective.owns.extend(
                document
                    .ownership
                    .owns
                    .into_iter()
                    .map(|value| ContextEntry::new(value, context_file.clone())),
            );
            effective.boundaries.extend(
                document
                    .ownership
                    .boundaries
                    .into_iter()
                    .map(|value| ContextEntry::new(value, context_file.clone())),
            );
            effective
                .signoffs
                .extend(
                    document
                        .signoffs
                        .into_iter()
                        .map(|declaration| EffectiveSignoff {
                            declaration,
                            source: context_file.clone(),
                        }),
                );

            for include in document.rule_includes {
                effective
                    .rule_includes
                    .push(ContextEntry::new(include.clone(), context_file.clone()));
                self.collect_rule_library(
                    fs,
                    &context_file,
                    &include,
                    &mut loaded_rule_libraries,
                    &mut seen_rule_ids,
                    &mut effective.rules,
                )?;
            }
            for rule in document.rules {
                insert_applicable_rule(
                    ApplicableRule::from_definition(rule, context_file.clone()),
                    &mut seen_rule_ids,
                    &mut effective.rules,
                )?;
            }
        }

        Ok(effective)
    }

    fn collect_rule_library(
        &self,
        fs: &impl FileSystem,
        source: &Utf8Path,
        include: &str,
        loaded_rule_libraries: &mut BTreeSet<Utf8PathBuf>,
        seen_rule_ids: &mut BTreeMap<String, Utf8PathBuf>,
        rules: &mut Vec<ApplicableRule>,
    ) -> Result<(), ProjectContextError> {
        let path = self.store.resolve_include(source, include)?;
        if !fs.is_file(&path) {
            return Err(ProjectContextError::MissingInclude {
                include: include.to_string(),
                source: source.to_path_buf(),
                resolved: path,
            });
        }
        if !loaded_rule_libraries.insert(path.clone()) {
            return Ok(());
        }

        let contents = fs
            .read_to_string(&path)
            .map_err(|source| ProjectContextError::Io {
                path: path.clone(),
                source,
            })?;
        let document = toml::from_str::<RuleLibraryDocument>(&contents).map_err(|source| {
            ProjectContextError::RuleDecode {
                path: path.clone(),
                source,
            }
        })?;
        if document.version != RULE_SCHEMA_VERSION {
            return Err(ProjectContextError::UnsupportedRuleSchemaVersion {
                path,
                version: document.version,
            });
        }
        for nested_include in document.includes {
            self.collect_rule_library(
                fs,
                &path,
                &nested_include,
                loaded_rule_libraries,
                seen_rule_ids,
                rules,
            )?;
        }
        for rule in document.rules {
            insert_applicable_rule(
                ApplicableRule::from_definition(rule, path.clone()),
                seen_rule_ids,
                rules,
            )?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EffectiveProjectContext {
    target_directory: Utf8PathBuf,
    context_files: Vec<Utf8PathBuf>,
    purpose: Option<ContextEntry>,
    owns: Vec<ContextEntry>,
    boundaries: Vec<ContextEntry>,
    signoffs: Vec<EffectiveSignoff>,
    rule_includes: Vec<ContextEntry>,
    rules: Vec<ApplicableRule>,
}

#[derive(Clone, PartialEq, Eq)]
struct EffectiveSignoff {
    declaration: ContextSignoffDeclaration,
    source: Utf8PathBuf,
}

impl fmt::Debug for EffectiveSignoff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EffectiveSignoff")
            .field("declaration", &self.declaration)
            .field("source", &RedactedContextText(self.source.as_str()))
            .finish()
    }
}

impl EffectiveProjectContext {
    fn new(target_directory: Utf8PathBuf) -> Self {
        Self {
            target_directory,
            context_files: Vec::new(),
            purpose: None,
            owns: Vec::new(),
            boundaries: Vec::new(),
            signoffs: Vec::new(),
            rule_includes: Vec::new(),
            rules: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ContextEntry {
    value: String,
    source: Utf8PathBuf,
}

impl ContextEntry {
    fn new(value: String, source: Utf8PathBuf) -> Self {
        Self { value, source }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ApplicableRule {
    id: String,
    text: String,
    rationale: Option<String>,
    references: Vec<String>,
    source: Utf8PathBuf,
}

impl ApplicableRule {
    fn from_definition(rule: ContextRuleDefinition, source: Utf8PathBuf) -> Self {
        Self {
            id: rule.id,
            text: rule.text,
            rationale: rule.rationale,
            references: rule.references,
            source,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContextListTarget {
    Owns,
    Boundary,
    RuleInclude,
}

impl ContextListTarget {
    fn values_mut(self, document: &mut ProjectContextFile) -> &mut Vec<String> {
        match self {
            Self::Owns => &mut document.ownership.owns,
            Self::Boundary => &mut document.ownership.boundaries,
            Self::RuleInclude => &mut document.rule_includes,
        }
    }

    fn field_name(self) -> &'static str {
        match self {
            Self::Owns => "ownership statement",
            Self::Boundary => "boundary statement",
            Self::RuleInclude => "rule include",
        }
    }

    fn add_command(self) -> &'static str {
        match self {
            Self::Owns => "ownership owns add",
            Self::Boundary => "ownership boundary add",
            Self::RuleInclude => "rule include add",
        }
    }

    fn remove_command(self) -> &'static str {
        match self {
            Self::Owns => "ownership owns remove",
            Self::Boundary => "ownership boundary remove",
            Self::RuleInclude => "rule include remove",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EditReport {
    context_file: Utf8PathBuf,
    status: EditStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditStatus {
    Created,
    Updated,
    Added,
    Removed,
    Unchanged,
}

impl fmt::Display for EditStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Created => f.write_str("created"),
            Self::Updated => f.write_str("updated"),
            Self::Added => f.write_str("added"),
            Self::Removed => f.write_str("removed"),
            Self::Unchanged => f.write_str("unchanged"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProjectContextFile {
    version: u16,
    purpose: String,
    #[serde(default)]
    signoffs: Vec<ContextSignoffDeclaration>,
    #[serde(default)]
    rule_includes: Vec<String>,
    #[serde(default)]
    ownership: ContextOwnership,
    #[serde(default)]
    rules: Vec<ContextRuleDefinition>,
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
enum ContextSignoffDeclaration {
    LegacyBuild(String),
    Typed(TypedContextSignoff),
}

impl fmt::Debug for ContextSignoffDeclaration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContextSignoffDeclaration")
            .field("kind", &self.kind())
            .field("has_target", &self.target().is_some())
            .field("minimum_grade", &self.minimum_grade())
            .finish()
    }
}

impl ContextSignoffDeclaration {
    fn typed(kind: SignoffKind, target: &str, minimum_grade: Option<ReviewGrade>) -> Self {
        Self::Typed(TypedContextSignoff {
            kind,
            target: match kind {
                SignoffKind::Build => Some(target.to_string()),
                SignoffKind::Review => None,
            },
            minimum_grade,
        })
    }

    fn kind(&self) -> SignoffKind {
        match self {
            Self::LegacyBuild(_) => SignoffKind::Build,
            Self::Typed(signoff) => signoff.kind,
        }
    }

    fn target(&self) -> Option<&str> {
        match self {
            Self::LegacyBuild(target) => Some(target),
            Self::Typed(signoff) => signoff.target.as_deref(),
        }
    }

    fn request_target(&self) -> &str {
        self.target().unwrap_or("review")
    }

    fn minimum_grade(&self) -> Option<ReviewGrade> {
        match self {
            Self::LegacyBuild(_) => None,
            Self::Typed(signoff) => signoff.minimum_grade,
        }
    }

    fn matches(&self, kind: SignoffKind, target: &str) -> bool {
        self.kind() == kind && self.request_target() == target
    }

    fn to_request(
        &self,
        repo_root: &Utf8Path,
        directory: &Utf8Path,
    ) -> Result<SignoffRequest, signoff_contract::SignoffContractError> {
        match (self.kind(), self.target()) {
            (SignoffKind::Build, None) => {
                return Err(signoff_contract::SignoffContractError::MissingBuildTarget);
            }
            (SignoffKind::Review, Some(_)) => {
                return Err(signoff_contract::SignoffContractError::UnexpectedReviewTarget);
            }
            (SignoffKind::Build, Some(_)) | (SignoffKind::Review, None) => {}
        }
        SignoffRequest::new(
            repo_root,
            directory,
            self.kind(),
            self.request_target(),
            self.minimum_grade(),
        )
    }

    fn display(&self) -> String {
        match (self.kind(), self.minimum_grade()) {
            (SignoffKind::Review, Some(grade)) => {
                format!("review (minimum grade {grade})")
            }
            (SignoffKind::Review, None) => String::from("review"),
            (SignoffKind::Build, _) => {
                format!("build {}", self.request_target())
            }
        }
    }
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TypedContextSignoff {
    kind: SignoffKind,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    minimum_grade: Option<ReviewGrade>,
}

impl fmt::Debug for TypedContextSignoff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TypedContextSignoff")
            .field("kind", &self.kind)
            .field("has_target", &self.target.is_some())
            .field("minimum_grade", &self.minimum_grade)
            .finish()
    }
}

impl ProjectContextFile {
    fn new(purpose: &str) -> Self {
        Self {
            version: CONTEXT_SCHEMA_VERSION,
            purpose: purpose.to_string(),
            signoffs: Vec::new(),
            rule_includes: Vec::new(),
            ownership: ContextOwnership::default(),
            rules: Vec::new(),
        }
    }

    fn normalize(&mut self) {
        self.rule_includes
            .append(&mut self.ownership.compat_rule_includes);
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ContextOwnership {
    #[serde(default)]
    owns: Vec<String>,
    #[serde(default)]
    boundaries: Vec<String>,
    #[serde(default, rename = "rule_includes", skip_serializing)]
    compat_rule_includes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ContextRuleDefinition {
    id: String,
    text: String,
    #[serde(default)]
    rationale: Option<String>,
    #[serde(default)]
    references: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleLibraryDocument {
    version: u16,
    #[serde(default)]
    includes: Vec<String>,
    #[serde(default)]
    rules: Vec<ContextRuleDefinition>,
}

#[derive(Debug)]
pub(crate) enum ProjectContextError {
    Io {
        path: Utf8PathBuf,
        source: io::Error,
    },
    Decode {
        path: Utf8PathBuf,
        source: toml::de::Error,
    },
    RuleDecode {
        path: Utf8PathBuf,
        source: toml::de::Error,
    },
    Encode {
        source: toml_edit::ser::Error,
    },
    ValueRepresentation {
        source: toml_edit::TomlError,
    },
    UnsupportedSchemaVersion {
        path: Utf8PathBuf,
        version: u16,
    },
    UnsupportedRuleSchemaVersion {
        path: Utf8PathBuf,
        version: u16,
    },
    ContextAlreadyExists {
        path: Utf8PathBuf,
    },
    MissingContext {
        path: Utf8PathBuf,
    },
    MissingInclude {
        include: String,
        source: Utf8PathBuf,
        resolved: Utf8PathBuf,
    },
    IncludeOutsideRepository {
        include: String,
        source: Utf8PathBuf,
    },
    DuplicateRuleId {
        id: String,
        first_source: Utf8PathBuf,
        second_source: Utf8PathBuf,
    },
    DuplicateLocalRuleId {
        id: String,
    },
    MissingInlineRule {
        id: String,
    },
    MissingSignoff {
        kind: SignoffKind,
        target: Option<String>,
    },
    MissingBuildTarget,
    UnexpectedReviewTarget,
    SignoffIdentityCollision {
        identity: String,
        existing_context: Utf8PathBuf,
        requested_context: Utf8PathBuf,
    },
    SignoffContract {
        source: signoff_contract::SignoffContractError,
    },
    InvalidReviewGrade(ReviewGradeError),
    OutsideRepository {
        path: Utf8PathBuf,
    },
    EmptyField {
        field: &'static str,
    },
}

impl fmt::Display for ProjectContextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "context filesystem error at `{path}`: {source}")
            }
            Self::Decode { path, source } => {
                write!(f, "context parse error at `{path}`: {source}")
            }
            Self::RuleDecode { path, source } => {
                write!(f, "rules parse error at `{path}`: {source}")
            }
            Self::Encode { .. } => f.write_str("could not encode the context document as TOML"),
            Self::ValueRepresentation { .. } => {
                f.write_str("could not encode a context value as TOML")
            }
            Self::UnsupportedSchemaVersion { path, version } => write!(
                f,
                "unsupported context schema version `{version}` at `{path}`; supported version is `{CONTEXT_SCHEMA_VERSION}`"
            ),
            Self::UnsupportedRuleSchemaVersion { path, version } => write!(
                f,
                "unsupported rules schema version `{version}` at `{path}`; supported version is `{RULE_SCHEMA_VERSION}`"
            ),
            Self::ContextAlreadyExists { path } => {
                write!(f, "`{path}` already exists.")
            }
            Self::MissingContext { path } => {
                write!(f, "No context file found at `{path}`.")
            }
            Self::MissingInclude {
                include,
                source,
                resolved,
            } => write!(
                f,
                "rule include `{include}` from `{source}` does not exist at `{resolved}`"
            ),
            Self::IncludeOutsideRepository { include, source } => write!(
                f,
                "rule include `{include}` from `{source}` resolves outside the repository"
            ),
            Self::DuplicateRuleId {
                id,
                first_source,
                second_source,
            } => write!(
                f,
                "duplicate rule id `{id}` in `{first_source}` and `{second_source}`"
            ),
            Self::DuplicateLocalRuleId { id } => {
                write!(f, "inline rule `{id}` already exists in this context.")
            }
            Self::MissingInlineRule { id } => {
                write!(f, "inline rule `{id}` does not exist in this context.")
            }
            Self::MissingSignoff {
                kind: SignoffKind::Build,
                target: Some(target),
            } => write!(
                f,
                "build signoff target `{target}` does not exist in this context."
            ),
            Self::MissingSignoff {
                kind: SignoffKind::Review,
                ..
            } => f.write_str("review signoff does not exist in this context."),
            Self::MissingSignoff { kind, .. } => {
                write!(f, "{kind} signoff does not exist in this context.")
            }
            Self::MissingBuildTarget => {
                f.write_str("build signoffs require a target, such as `ci`.")
            }
            Self::UnexpectedReviewTarget => {
                f.write_str("review signoffs do not accept a target or profile.")
            }
            Self::SignoffIdentityCollision {
                identity,
                existing_context,
                requested_context,
            } => write!(
                f,
                "signoff identity `{identity}` in `{requested_context}` collides with a declaration in `{existing_context}`"
            ),
            Self::SignoffContract { source } => write!(f, "{source}"),
            Self::InvalidReviewGrade(source) => write!(f, "{source}"),
            Self::OutsideRepository { path } => {
                write!(f, "`{path}` is outside the repository.")
            }
            Self::EmptyField { field } => {
                write!(f, "`{field}` cannot be empty.")
            }
        }
    }
}

impl Error for ProjectContextError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Decode { source, .. } | Self::RuleDecode { source, .. } => Some(source),
            Self::Encode { source } => Some(source),
            Self::ValueRepresentation { source } => Some(source),
            Self::SignoffContract { source } => Some(source),
            Self::InvalidReviewGrade(source) => Some(source),
            Self::UnsupportedSchemaVersion { .. }
            | Self::UnsupportedRuleSchemaVersion { .. }
            | Self::ContextAlreadyExists { .. }
            | Self::MissingContext { .. }
            | Self::MissingInclude { .. }
            | Self::IncludeOutsideRepository { .. }
            | Self::DuplicateRuleId { .. }
            | Self::DuplicateLocalRuleId { .. }
            | Self::MissingInlineRule { .. }
            | Self::MissingSignoff { .. }
            | Self::MissingBuildTarget
            | Self::UnexpectedReviewTarget
            | Self::SignoffIdentityCollision { .. }
            | Self::OutsideRepository { .. }
            | Self::EmptyField { .. } => None,
        }
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

fn requested_path_from_cwd(path: Option<&Utf8PathBuf>, cwd: &Utf8Path) -> Utf8PathBuf {
    match path {
        None => cwd.to_path_buf(),
        Some(path) if path.as_str() == "." => cwd.to_path_buf(),
        Some(path) if path.is_absolute() => path.clone(),
        Some(path) => cwd.join(path),
    }
}

fn required_text<'value>(
    field: &'static str,
    value: &'value str,
) -> Result<&'value str, ProjectContextError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Err(ProjectContextError::EmptyField { field })
    } else {
        Ok(trimmed)
    }
}

fn validate_references(references: &[String]) -> Result<Vec<String>, ProjectContextError> {
    references
        .iter()
        .map(|reference| required_text("reference", reference).map(ToString::to_string))
        .collect()
}

fn insert_applicable_rule(
    rule: ApplicableRule,
    seen_rule_ids: &mut BTreeMap<String, Utf8PathBuf>,
    rules: &mut Vec<ApplicableRule>,
) -> Result<(), ProjectContextError> {
    if let Some(first_source) = seen_rule_ids.get(&rule.id) {
        return Err(ProjectContextError::DuplicateRuleId {
            id: rule.id,
            first_source: first_source.clone(),
            second_source: rule.source,
        });
    }
    seen_rule_ids.insert(rule.id.clone(), rule.source.clone());
    rules.push(rule);
    Ok(())
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

fn render_context_show(
    resolver: &ProjectContextResolver,
    effective: &EffectiveProjectContext,
) -> String {
    let store = &resolver.store;
    let context_files = if effective.context_files.is_empty() {
        vec![String::from("No context.toml files apply.")]
    } else {
        effective
            .context_files
            .iter()
            .map(|path| store.display_path(path))
            .collect()
    };

    ViewBuilder::new()
        .title("rapport context show")
        .section("Context", |b| {
            b.entries([
                ("path", store.display_path(&effective.target_directory)),
                ("files", context_files.join(", ")),
            ])
        })
        .section("Purpose", |b| {
            b.items(render_optional_entry(
                store,
                effective.purpose.as_ref(),
                "No purpose found.",
            ))
        })
        .section("Ownership", |b| {
            b.items(render_entries(
                store,
                &effective.owns,
                "No ownership statements.",
            ))
        })
        .section("Boundaries", |b| {
            b.items(render_entries(
                store,
                &effective.boundaries,
                "No boundary statements.",
            ))
        })
        .section("Signoffs", |b| {
            b.items(render_signoffs(
                store,
                &effective.signoffs,
                "No signoffs required.",
            ))
        })
        .section("Rule Includes", |b| {
            b.items(render_entries(
                store,
                &effective.rule_includes,
                "No rule includes.",
            ))
        })
        .section("Benchmarks", |b| {
            b.items(render_rules(store, &effective.rules))
        })
        .next_actions(nonempty![RunHint::new(format!(
            "rapport context doctor {}",
            store.display_path(&effective.target_directory)
        ))])
        .build()
}

fn render_context_doctor(
    resolver: &ProjectContextResolver,
    effective: &EffectiveProjectContext,
) -> String {
    let store = &resolver.store;
    ViewBuilder::new()
        .title("rapport context doctor")
        .section("Doctor", |b| {
            b.entries([
                ("status", String::from("pass")),
                ("path", store.display_path(&effective.target_directory)),
                ("contexts", effective.context_files.len().to_string()),
                ("signoffs", effective.signoffs.len().to_string()),
                ("benchmarks", effective.rules.len().to_string()),
            ])
        })
        .next_actions(nonempty![RunHint::new(format!(
            "rapport context show {}",
            store.display_path(&effective.target_directory)
        ))])
        .build()
}

fn render_edit(command: &str, store: &ProjectContextStore, report: &EditReport) -> String {
    ViewBuilder::new()
        .title(format!("rapport context {command}"))
        .section("Context", |b| {
            b.entries([
                ("status", report.status.to_string()),
                ("file", store.display_path(&report.context_file)),
            ])
        })
        .next_actions(nonempty![RunHint::new(format!(
            "rapport context show {}",
            store.display_path(
                report
                    .context_file
                    .parent()
                    .unwrap_or_else(|| store.repo_root.as_path())
            )
        ))])
        .build()
}

fn render_signoff_edit(
    command: &str,
    store: &ProjectContextStore,
    context_file: &Utf8Path,
    request: &SignoffRequest,
) -> String {
    let mut entries = vec![
        ("kind", request.kind().to_string()),
        ("status", format!("signoff: {}", request.qualified_target())),
        ("context", store.display_path(context_file)),
        ("workflow", store.display_path(request.workflow_path())),
    ];
    if request.kind() == SignoffKind::Build {
        entries.insert(1, ("target", request.target().to_string()));
    }
    ViewBuilder::new()
        .title(format!("rapport context signoff {command}"))
        .section("Signoff", |b| b.entries(entries))
        .next_actions(nonempty![RunHint::new("rapport doctor")])
        .build()
}

fn render_context_error(command: &str, summary: &str, error: &ProjectContextError) -> String {
    ViewBuilder::new()
        .title(format!("rapport context {command}"))
        .paragraph(summary)
        .paragraph(error)
        .next_actions(nonempty![RunHint::new("rapport context doctor <path>")])
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

fn render_optional_entry(
    store: &ProjectContextStore,
    entry: Option<&ContextEntry>,
    empty_message: &str,
) -> Vec<String> {
    entry.map_or_else(
        || vec![empty_message.to_string()],
        |entry| vec![format_entry(store, entry)],
    )
}

fn render_entries(
    store: &ProjectContextStore,
    entries: &[ContextEntry],
    empty_message: &str,
) -> Vec<String> {
    if entries.is_empty() {
        vec![empty_message.to_string()]
    } else {
        entries
            .iter()
            .map(|entry| format_entry(store, entry))
            .collect()
    }
}

fn render_signoffs(
    store: &ProjectContextStore,
    signoffs: &[EffectiveSignoff],
    empty_message: &str,
) -> Vec<String> {
    if signoffs.is_empty() {
        vec![empty_message.to_string()]
    } else {
        signoffs
            .iter()
            .map(|signoff| {
                format!(
                    "{} ({})",
                    signoff.declaration.display(),
                    store.display_path(&signoff.source)
                )
            })
            .collect()
    }
}

fn render_rules(store: &ProjectContextStore, rules: &[ApplicableRule]) -> Vec<String> {
    if rules.is_empty() {
        return vec![String::from("No applicable benchmarks.")];
    }
    rules
        .iter()
        .map(|rule| {
            let mut line = format!(
                "`{}` -- {} ({})",
                rule.id,
                single_line(&rule.text),
                store.display_path(&rule.source)
            );
            if let Some(rationale) = &rule.rationale {
                line.push_str("; rationale: ");
                line.push_str(&single_line(rationale));
            }
            if !rule.references.is_empty() {
                line.push_str("; references: ");
                line.push_str(&rule.references.join(", "));
            }
            line
        })
        .collect()
}

fn format_entry(store: &ProjectContextStore, entry: &ContextEntry) -> String {
    format!(
        "{} ({})",
        single_line(&entry.value),
        store.display_path(&entry.source)
    )
}

fn single_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn render_context_file(document: &ProjectContextFile) -> Result<String, ProjectContextError> {
    let canonical = canonical_context_file(document);
    let mut output = toml_edit::ser::to_document(&canonical)
        .map_err(|source| ProjectContextError::Encode { source })?;
    rebuild_structural_tables(&mut output, &canonical);
    style_prose_item(&mut output["purpose"], "purpose", &canonical.purpose)?;
    style_prose_array(&mut output["ownership"]["owns"], &canonical.ownership.owns)?;
    style_prose_array(
        &mut output["ownership"]["boundaries"],
        &canonical.ownership.boundaries,
    )?;
    if let Some(rules) = output["rules"].as_array_of_tables_mut() {
        for (table, rule) in rules.iter_mut().zip(&canonical.rules) {
            style_prose_item(&mut table["text"], "text", &rule.text)?;
            if let Some(rationale) = &rule.rationale {
                style_prose_item(&mut table["rationale"], "rationale", rationale)?;
            }
        }
    }
    Ok(output.to_string())
}

fn rebuild_structural_tables(output: &mut toml_edit::DocumentMut, document: &ProjectContextFile) {
    if let Some(toml_edit::Item::Value(toml_edit::Value::InlineTable(ownership))) =
        output.as_table_mut().remove("ownership")
    {
        output["ownership"] = toml_edit::Item::Table(ownership.into_table());
    }

    if !document.signoffs.is_empty() {
        let mut signoffs = toml_edit::ArrayOfTables::new();
        for signoff in &document.signoffs {
            let mut table = toml_edit::Table::new();
            table["kind"] = toml_edit::value(signoff.kind().to_string());
            if let Some(target) = signoff.target() {
                table["target"] = toml_edit::value(target);
            }
            if let Some(minimum_grade) = signoff.minimum_grade() {
                table["minimum_grade"] = toml_edit::value(minimum_grade.to_string());
            }
            signoffs.push(table);
        }
        output["signoffs"] = toml_edit::Item::ArrayOfTables(signoffs);
    }

    output.as_table_mut().remove("rules");
    if !document.rules.is_empty() {
        let mut rules = toml_edit::ArrayOfTables::new();
        for rule in &document.rules {
            let mut table = toml_edit::Table::new();
            table["id"] = toml_edit::value(&rule.id);
            table["text"] = toml_edit::value(&rule.text);
            if let Some(rationale) = &rule.rationale {
                table["rationale"] = toml_edit::value(rationale);
            }
            let mut references = toml_edit::Array::new();
            for reference in &rule.references {
                references.push(reference);
            }
            table["references"] = toml_edit::value(references);
            rules.push(table);
        }
        output["rules"] = toml_edit::Item::ArrayOfTables(rules);
    }
}

fn canonical_context_file(document: &ProjectContextFile) -> ProjectContextFile {
    let mut canonical = document.clone();
    canonical.purpose = format_markdown(&canonical.purpose, CONTEXT_LINE_WIDTH);
    canonical.ownership.owns = canonical
        .ownership
        .owns
        .iter()
        .map(|value| format_markdown(value, CONTEXT_LINE_WIDTH - 2))
        .collect();
    canonical.ownership.boundaries = canonical
        .ownership
        .boundaries
        .iter()
        .map(|value| format_markdown(value, CONTEXT_LINE_WIDTH - 2))
        .collect();
    for rule in &mut canonical.rules {
        rule.text = format_markdown(&rule.text, CONTEXT_LINE_WIDTH);
        rule.rationale = rule
            .rationale
            .as_deref()
            .map(|value| format_markdown(value, CONTEXT_LINE_WIDTH));
    }
    canonical
}

fn style_prose_item(
    item: &mut toml_edit::Item,
    key: &str,
    value: &str,
) -> Result<(), ProjectContextError> {
    let inline = toml_string(value);
    if !value.contains('\n') && key.len() + 3 + inline.len() <= CONTEXT_LINE_WIDTH as usize {
        return Ok(());
    }
    let mut formatted = parse_toml_value(&toml_multiline_string(value))?;
    formatted.decor_mut().set_prefix(" ");
    *item = toml_edit::Item::Value(formatted);
    Ok(())
}

fn style_prose_array(
    item: &mut toml_edit::Item,
    values: &[String],
) -> Result<(), ProjectContextError> {
    let Some(array) = item.as_array_mut() else {
        return Ok(());
    };
    let expanded = values.iter().any(|value| {
        value.contains('\n') || toml_string(value).len() + 3 > CONTEXT_LINE_WIDTH as usize
    });
    for (index, value) in values.iter().enumerate() {
        let representation =
            if value.contains('\n') || toml_string(value).len() + 3 > CONTEXT_LINE_WIDTH as usize {
                toml_multiline_string(value)
            } else {
                toml_string(value)
            };
        let mut formatted = parse_toml_value(&representation)?;
        if expanded {
            formatted.decor_mut().set_prefix("\n  ");
        }
        array.replace_formatted(index, formatted);
    }
    if expanded {
        array.set_trailing_comma(true);
        array.set_trailing("\n");
    }
    Ok(())
}

fn parse_toml_value(representation: &str) -> Result<toml_edit::Value, ProjectContextError> {
    representation
        .parse()
        .map_err(|source| ProjectContextError::ValueRepresentation { source })
}

fn format_markdown(value: &str, line_width: u32) -> String {
    let configuration = ConfigurationBuilder::new()
        .line_width(line_width)
        .text_wrap(TextWrap::Always)
        .build();
    let formatted = match format_text(value, &configuration, |_, _, _| Ok(None)) {
        Ok(Some(formatted)) => formatted,
        Ok(None) | Err(_) => value.to_string(),
    };
    formatted
        .strip_suffix('\n')
        .unwrap_or(&formatted)
        .to_string()
}

fn toml_multiline_string(value: &str) -> String {
    if !value.contains("'''")
        && value
            .chars()
            .all(|character| !character.is_control() || character == '\n' || character == '\t')
    {
        return format!("'''\n{value}'''");
    }

    let mut escaped = String::from("\"\"\"\n");
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push('\n'),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push('\t'),
            character if character.is_control() => {
                let _ = write!(&mut escaped, "\\u{:04X}", u32::from(character));
            }
            character => escaped.push(character),
        }
    }
    escaped.push_str("\"\"\"");
    escaped
}

fn toml_string(value: &str) -> String {
    let mut escaped = String::from("\"");
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                let _ = write!(&mut escaped, "\\u{:04X}", u32::from(character));
            }
            character => escaped.push(character),
        }
    }
    escaped.push('"');
    escaped
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use rapport_files::InMemoryFileSystem;

    #[test]
    fn init_writes_canonical_context_file() {
        let mut fs = repo_fs();
        let store = ProjectContextStore::new(Utf8PathBuf::from("/repo"));

        let report = store
            .init(
                &mut fs,
                Utf8Path::new("/repo/app/core/domain"),
                "Owns workspace rules.",
            )
            .unwrap();

        assert_eq!(report.status, EditStatus::Created);
        assert_eq!(
            fs.read_to_string("/repo/app/core/domain/context.toml")
                .unwrap(),
            "version = 1\npurpose = \"Owns workspace rules.\"\nsignoffs = []\nrule_includes = []\n\n[ownership]\nowns = []\nboundaries = []\n"
        );
    }

    #[test]
    fn format_markdown_should_wrap_prose_without_formatting_fenced_code() {
        let source = "This paragraph contains enough ordinary prose that the Markdown formatter must wrap it across several lines while preserving its meaning.\n\n```rust\nlet deliberately_long_identifier = something_that_must_not_be_wrapped();\n```";

        let formatted = format_markdown(source, 60);
        let paragraph = formatted.split("\n\n").next().unwrap();

        assert!(paragraph.lines().count() > 1);
        assert!(paragraph.lines().all(|line| line.len() <= 60));
        assert!(formatted.contains(
            "```rust\nlet deliberately_long_identifier = something_that_must_not_be_wrapped();\n```"
        ));
        assert_eq!(format_markdown(&formatted, 60), formatted);
    }

    #[test]
    fn render_context_file_should_write_readable_canonical_markdown() {
        let purpose = "The shared client-facing view of the domain, serialized between the app and the API. This is the API contract; it is not the universal domain model for backend services or persistence.";
        let mut document = ProjectContextFile::new(purpose);
        document.ownership.owns.push(String::from(
            "The public API contract, including `WorkspaceDocument`, and the behavior clients may rely on when exchanging domain data.",
        ));
        document.rules.push(ContextRuleDefinition {
            id: String::from("CONTEXT-001"),
            text: String::from(
                "Keep fenced examples intact.\n\n```rust\nlet deliberately_long_identifier = something_that_must_not_be_wrapped();\n```",
            ),
            rationale: Some(String::from(
                "Readable context makes Git diffs useful to reviewers without asking agents to edit Rapport-owned files directly.",
            )),
            references: vec![String::from(
                "https://example.com/an/indivisible/reference/that/is/allowed/to/exceed/the/formatter/line/width",
            )],
        });

        let rendered = render_context_file(&document).unwrap();
        let decoded: ProjectContextFile = toml::from_str(&rendered).unwrap();
        let rendered_again = render_context_file(&decoded).unwrap();

        assert!(rendered.contains("purpose = '''\n"));
        assert!(rendered.contains("```rust\nlet deliberately_long_identifier"));
        assert_eq!(rendered_again, rendered);
        assert_eq!(
            decoded.purpose,
            format_markdown(purpose, CONTEXT_LINE_WIDTH)
        );
        assert!(decoded.rules[0].text.contains("\n\n```rust\n"));
        assert_eq!(
            decoded.ownership.owns[0],
            format_markdown(&document.ownership.owns[0], CONTEXT_LINE_WIDTH - 2)
        );
        for line in rendered.lines().filter(|line| {
            !line.contains("something_that_must_not_be_wrapped")
                && !line.contains("https://example.com/")
        }) {
            assert!(
                line.len() <= CONTEXT_LINE_WIDTH as usize,
                "expecting formatter-controlled line to fit: {line}"
            );
        }
    }

    #[test]
    fn render_context_file_should_preserve_toml_sensitive_markdown() {
        let purpose = "Use `code`, apostrophes, \"quotes\", and Windows paths such as `C:\\\\workspace`.\n\nA literal delimiter looks like ''' inside prose.";
        let document = ProjectContextFile::new(purpose);

        let rendered = render_context_file(&document).unwrap();
        let decoded: ProjectContextFile = toml::from_str(&rendered).unwrap();

        assert!(rendered.contains("purpose = \"\"\"\n"));
        assert_eq!(
            decoded.purpose,
            format_markdown(purpose, CONTEXT_LINE_WIDTH)
        );
    }

    #[test]
    fn resolver_accumulates_ancestor_contexts_and_rule_includes() {
        let mut fs = repo_fs();
        fs.write_string(
            "/repo/rules/rust.toml",
            r#"
version = 1

[[rules]]
id = "RUST-001"
text = "Use rustfmt."
"#,
        )
        .unwrap();
        fs.write_string(
            "/repo/context.toml",
            r#"
version = 1
purpose = "Root purpose"
signoffs = ["shared"]
rule_includes = ["/rules/rust.toml"]

[ownership]
owns = ["Root ownership"]
boundaries = []
"#,
        )
        .unwrap();
        fs.write_string(
            "/repo/app/core/context.toml",
            r#"
version = 1
purpose = "Core purpose"
signoffs = ["apple"]
rule_includes = []

[ownership]
owns = ["Core ownership"]
boundaries = ["Persistence lives elsewhere."]

[[rules]]
id = "CORE-001"
text = "Keep core boring."
"#,
        )
        .unwrap();
        let resolver =
            ProjectContextResolver::new(ProjectContextStore::new(Utf8PathBuf::from("/repo")));

        let effective = resolver
            .resolve(&fs, Utf8Path::new("/repo/app/core/domain"))
            .unwrap();

        assert_eq!(effective.context_files.len(), 2);
        assert_eq!(
            effective.purpose.unwrap().value,
            String::from("Core purpose")
        );
        assert_eq!(effective.owns.len(), 2);
        assert_eq!(effective.boundaries.len(), 1);
        assert_eq!(
            effective
                .signoffs
                .iter()
                .map(|signoff| signoff.declaration.target())
                .collect::<Vec<_>>(),
            vec![Some("shared"), Some("apple")]
        );
        assert_eq!(
            effective
                .rules
                .iter()
                .map(|rule| rule.id.as_str())
                .collect::<Vec<_>>(),
            vec!["RUST-001", "CORE-001"]
        );
    }

    #[test]
    fn required_signoffs_unions_contexts_for_active_paths() {
        let mut fs = repo_fs();
        fs.add_file("/repo/apple/app.rs");
        fs.add_file("/repo/windows/app.rs");
        fs.write_string(
            "/repo/context.toml",
            "version = 1\npurpose = \"Shared\"\nsignoffs = [\"shared\"]\n",
        )
        .unwrap();
        fs.write_string(
            "/repo/apple/context.toml",
            "version = 1\npurpose = \"Apple\"\nsignoffs = [\"ci\"]\n",
        )
        .unwrap();
        fs.write_string(
            "/repo/windows/context.toml",
            "version = 1\npurpose = \"Windows\"\nsignoffs = [\"ci\"]\n",
        )
        .unwrap();

        let required = required_signoffs_for_paths(
            &fs,
            Utf8Path::new("/repo"),
            &[String::from("apple/app.rs"), String::from("windows/app.rs")],
        )
        .unwrap();

        assert_eq!(
            required,
            vec!["root-build-shared", "apple-build-ci", "windows-build-ci"]
        );
    }

    #[test]
    fn typed_signoffs_inherit_build_and_child_review_with_distinct_paths() {
        let mut fs = repo_fs();
        fs.add_file("/repo/app/apple/something/else/file.rs");
        fs.write_string(
            "/repo/app/apple/context.toml",
            r#"version = 1
purpose = "Apple"

[[signoffs]]
kind = "build"
target = "ci"
"#,
        )
        .unwrap();
        fs.write_string(
            "/repo/app/apple/something/else/context.toml",
            r#"version = 1
purpose = "Nested Apple"

[[signoffs]]
kind = "review"
minimum_grade = "A-"
"#,
        )
        .unwrap();

        let requirements = required_signoff_requirements_for_paths(
            &fs,
            Utf8Path::new("/repo"),
            &[String::from("app/apple/something/else/file.rs")],
        )
        .unwrap();

        assert_eq!(requirements.len(), 2);
        assert_eq!(
            requirements[0].request.qualified_target(),
            "app-apple-build-ci"
        );
        assert_eq!(
            requirements[1].request.qualified_target(),
            "app-apple-something-else-review"
        );
        assert_eq!(requirements[0].paths, requirements[1].paths);
    }

    #[test]
    fn resolver_reports_missing_rule_include() {
        let mut fs = repo_fs();
        fs.write_string(
            "/repo/context.toml",
            r#"
version = 1
purpose = "Root purpose"
rule_includes = ["/rules/missing.toml"]

[ownership]
owns = []
boundaries = []
"#,
        )
        .unwrap();
        let resolver =
            ProjectContextResolver::new(ProjectContextStore::new(Utf8PathBuf::from("/repo")));

        let error = resolver
            .resolve(&fs, Utf8Path::new("/repo/app"))
            .unwrap_err();

        assert!(error.to_string().contains("does not exist"));
        assert!(error.to_string().contains("/rules/missing.toml"));
    }

    #[test]
    fn resolver_reports_duplicate_rule_ids() {
        let mut fs = repo_fs();
        fs.write_string(
            "/repo/context.toml",
            r#"
version = 1
purpose = "Root purpose"
rule_includes = []

[ownership]
owns = []
boundaries = []

[[rules]]
id = "DUP-001"
text = "First."
"#,
        )
        .unwrap();
        fs.write_string(
            "/repo/app/context.toml",
            r#"
version = 1
purpose = "App purpose"
rule_includes = []

[ownership]
owns = []
boundaries = []

[[rules]]
id = "DUP-001"
text = "Second."
"#,
        )
        .unwrap();
        let resolver =
            ProjectContextResolver::new(ProjectContextStore::new(Utf8PathBuf::from("/repo")));

        let error = resolver
            .resolve(&fs, Utf8Path::new("/repo/app"))
            .unwrap_err();

        assert!(error.to_string().contains("duplicate rule id `DUP-001`"));
    }

    #[test]
    fn context_file_accepts_proposed_rule_includes_after_ownership() {
        let mut fs = repo_fs();
        fs.write_string(
            "/repo/context.toml",
            r#"
version = 1
purpose = "Root purpose"

[ownership]
owns = []
boundaries = []
rule_includes = ["/rules/rust.toml"]
"#,
        )
        .unwrap();
        fs.write_string(
            "/repo/rules/rust.toml",
            r#"
version = 1

[[rules]]
id = "RUST-001"
text = "Use rustfmt."
"#,
        )
        .unwrap();
        let resolver =
            ProjectContextResolver::new(ProjectContextStore::new(Utf8PathBuf::from("/repo")));

        let effective = resolver.resolve(&fs, Utf8Path::new("/repo")).unwrap();

        assert_eq!(effective.rule_includes[0].value, "/rules/rust.toml");
        assert_eq!(effective.rules[0].id, "RUST-001");
    }

    #[test]
    fn resolver_reports_unsupported_rule_library_version() {
        let mut fs = repo_fs();
        fs.write_string(
            "/repo/context.toml",
            r#"
version = 1
purpose = "Root purpose"
rule_includes = ["/rules/rust.toml"]

[ownership]
owns = []
boundaries = []
"#,
        )
        .unwrap();
        fs.write_string(
            "/repo/rules/rust.toml",
            r#"
version = 2

[[rules]]
id = "RUST-001"
text = "Use rustfmt."
"#,
        )
        .unwrap();
        let resolver =
            ProjectContextResolver::new(ProjectContextStore::new(Utf8PathBuf::from("/repo")));

        let error = resolver.resolve(&fs, Utf8Path::new("/repo")).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("unsupported rules schema version `2`")
        );
    }

    #[test]
    fn resolved_rule_debug_redacts_repository_authored_content() {
        let rule = ResolvedContextRule {
            id: String::from("PRIVATE-RULE-ID"),
            text: String::from("private rule text"),
            rationale: Some(String::from("private rationale")),
            references: vec![String::from("private reference")],
            source: String::from("private/rules.toml"),
        };

        let debug = format!("{rule:?}");

        for private_value in [
            "PRIVATE-RULE-ID",
            "private rule text",
            "private rationale",
            "private reference",
            "private/rules.toml",
        ] {
            assert!(!debug.contains(private_value));
        }
        assert!(debug.contains("<redacted;"));
        assert!(debug.contains("reference_count: 1"));

        let requirement = SignoffRequirement {
            request: SignoffRequest::new(
                Utf8Path::new("/repo"),
                Utf8Path::new("/repo/private/component"),
                SignoffKind::Build,
                "private-target",
                None,
            )
            .unwrap(),
            paths: vec![String::from("private/component/file.rs")],
        };
        let requirement_debug = format!("{requirement:?}");
        assert!(!requirement_debug.contains("private/component"));
        assert!(!requirement_debug.contains("private-target"));
        assert!(requirement_debug.contains("path_count: 1"));
    }

    fn repo_fs() -> InMemoryFileSystem {
        let mut fs = InMemoryFileSystem::default();
        fs.add_directory("/repo/.git");
        fs
    }
}
