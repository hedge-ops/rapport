use crate::cli::{
    ContextCommand, ContextInitArgs, ContextListCommand, ContextOwnershipCommand,
    ContextPurposeCommand, ContextRuleAddArgs, ContextRuleCommand, ContextRuleUpdateArgs,
};
use crate::context::{Clock, CommandContext};
use crate::repository_files::find_named_files;
use crate::telemetry::{CommandEvent, CommandEventOutcome, TelemetryError, TelemetryWriter};
use crate::{RunHint, ViewBuilder};
use nonempty::nonempty;
use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::fmt::Write as _;
use std::io::{self, Write};
use std::process::ExitCode;

const SUCCESS: u8 = 0;
const FAILURE: u8 = 2;
const CONTEXT_FILE: &str = "context.toml";
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

    ProjectContextRepositoryValidation {
        context_files,
        problems,
    }
}

pub(crate) fn required_signoffs_for_paths(
    fs: &impl FileSystem,
    repo_root: &Utf8Path,
    paths: &[String],
) -> Result<Vec<String>, ProjectContextError> {
    let resolver = ProjectContextResolver::new(ProjectContextStore::new(repo_root.to_path_buf()));
    let mut required = Vec::new();
    let mut seen = BTreeSet::new();

    for path in paths {
        let effective = resolver.resolve(fs, &repo_root.join(path))?;
        for signoff in effective.signoffs {
            if seen.insert(signoff.value.clone()) {
                required.push(signoff.value);
            }
        }
    }

    Ok(required)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectContextRepositoryValidation {
    context_files: Vec<Utf8PathBuf>,
    problems: Vec<ProjectContextValidationProblem>,
}

impl ProjectContextRepositoryValidation {
    pub(crate) fn context_file_count(&self) -> usize {
        self.context_files.len()
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
        let status = mutation(&mut document)?;
        Self::save_context_file(fs, &context_file, &document)?;
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
        fs.write_string(path, render_context_file(document))
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
            effective.signoffs.extend(
                document
                    .signoffs
                    .into_iter()
                    .map(|value| ContextEntry::new(value, context_file.clone())),
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
    signoffs: Vec<ContextEntry>,
    rule_includes: Vec<ContextEntry>,
    rules: Vec<ApplicableRule>,
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectContextFile {
    version: u16,
    purpose: String,
    #[serde(default)]
    signoffs: Vec<String>,
    #[serde(default)]
    rule_includes: Vec<String>,
    #[serde(default)]
    ownership: ContextOwnership,
    #[serde(default)]
    rules: Vec<ContextRuleDefinition>,
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

#[derive(Debug, Default, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextOwnership {
    #[serde(default)]
    owns: Vec<String>,
    #[serde(default)]
    boundaries: Vec<String>,
    #[serde(default, rename = "rule_includes")]
    compat_rule_includes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
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
            Self::UnsupportedSchemaVersion { .. }
            | Self::UnsupportedRuleSchemaVersion { .. }
            | Self::ContextAlreadyExists { .. }
            | Self::MissingContext { .. }
            | Self::MissingInclude { .. }
            | Self::IncludeOutsideRepository { .. }
            | Self::DuplicateRuleId { .. }
            | Self::DuplicateLocalRuleId { .. }
            | Self::MissingInlineRule { .. }
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
            b.items(render_entries(
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

fn render_context_file(document: &ProjectContextFile) -> String {
    let mut output = String::new();
    let _ = writeln!(&mut output, "version = {}", document.version);
    let _ = writeln!(&mut output);
    let _ = writeln!(&mut output, "purpose = {}", toml_string(&document.purpose));
    let _ = writeln!(&mut output);
    push_string_array(&mut output, "signoffs", &document.signoffs);
    let _ = writeln!(&mut output);
    push_string_array(&mut output, "rule_includes", &document.rule_includes);
    let _ = writeln!(&mut output);
    let _ = writeln!(&mut output, "[ownership]");
    push_string_array(&mut output, "owns", &document.ownership.owns);
    push_string_array(&mut output, "boundaries", &document.ownership.boundaries);

    for rule in &document.rules {
        let _ = writeln!(&mut output);
        let _ = writeln!(&mut output, "[[rules]]");
        let _ = writeln!(&mut output, "id = {}", toml_string(&rule.id));
        let _ = writeln!(&mut output, "text = {}", toml_string(&rule.text));
        if let Some(rationale) = &rule.rationale {
            let _ = writeln!(&mut output, "rationale = {}", toml_string(rationale));
        }
        push_string_array(&mut output, "references", &rule.references);
    }

    output
}

fn push_string_array(output: &mut String, key: &str, values: &[String]) {
    if values.is_empty() {
        let _ = writeln!(output, "{key} = []");
        return;
    }
    let _ = writeln!(output, "{key} = [");
    for value in values {
        let _ = writeln!(output, "  {},", toml_string(value));
    }
    let _ = writeln!(output, "]");
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
            "version = 1\n\npurpose = \"Owns workspace rules.\"\n\nsignoffs = []\n\nrule_includes = []\n\n[ownership]\nowns = []\nboundaries = []\n"
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
                .map(|signoff| signoff.value.as_str())
                .collect::<Vec<_>>(),
            vec!["shared", "apple"]
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
            "version = 1\npurpose = \"Apple\"\nsignoffs = [\"apple\", \"shared\"]\n",
        )
        .unwrap();
        fs.write_string(
            "/repo/windows/context.toml",
            "version = 1\npurpose = \"Windows\"\nsignoffs = [\"windows\"]\n",
        )
        .unwrap();

        let required = required_signoffs_for_paths(
            &fs,
            Utf8Path::new("/repo"),
            &[String::from("apple/app.rs"), String::from("windows/app.rs")],
        )
        .unwrap();

        assert_eq!(required, vec!["shared", "apple", "windows"]);
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

    fn repo_fs() -> InMemoryFileSystem {
        let mut fs = InMemoryFileSystem::default();
        fs.add_directory("/repo/.git");
        fs
    }
}
