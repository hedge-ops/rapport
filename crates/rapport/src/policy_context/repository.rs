//! Context discovery, persistence, and effective inheritance.
//!
//! This module owns Context discovery, hierarchy resolution, mutation, and composition with shared Rulesets.

use super::Error;
use super::boundary;
use super::domain::{Context, ContextId, RepositoryPath};
use crate::repository_files::find_named_files;
use crate::shared_ruleset::SharedRulesets;
use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};
use std::collections::BTreeSet;

pub(super) struct Repository {
    repo_root: Utf8PathBuf,
    records: Vec<Record>,
    shared: SharedRulesets,
}

impl Repository {
    pub(super) fn load(fs: &mut impl FileSystem, repo_root: &Utf8Path) -> Result<Self, Error> {
        let shared = SharedRulesets::load(fs, repo_root)?;
        let paths =
            find_named_files(fs, repo_root, "context.toml").map_err(|source| Error::Io {
                path: repo_root.to_path_buf(),
                source,
            })?;
        let mut records = Vec::new();
        for path in paths {
            let contents = fs.read_to_string(&path).map_err(|source| Error::Io {
                path: path.clone(),
                source,
            })?;
            let context = boundary::parse(&contents, &path)?;
            let directory = path.parent().unwrap_or(repo_root).to_path_buf();
            records.push(Record {
                context,
                path,
                directory,
            });
        }
        records.sort_by(|left, right| left.directory.cmp(&right.directory));
        let repository = Self {
            repo_root: repo_root.to_path_buf(),
            records,
            shared,
        };
        repository.validate()?;
        Ok(repository)
    }

    pub(super) fn init(
        &mut self,
        fs: &mut impl FileSystem,
        user_path: &Utf8Path,
        purpose: String,
        namespace: Option<&str>,
        component_type: Option<String>,
    ) -> Result<&Record, Error> {
        let directory = resolve_path(&self.repo_root, user_path)?;
        if !fs.is_dir(&directory) {
            return Err(Error::InvalidPath);
        }
        let path = directory.join("context.toml");
        let relative = directory
            .strip_prefix(&self.repo_root)
            .unwrap_or(&directory);
        let id = namespace.map_or_else(|| ContextId::derive(relative), ContextId::parse)?;
        if self
            .records
            .iter()
            .any(|record| record.context.id() == &id || record.path == path)
        {
            return Err(Error::DuplicateContext(id.to_string()));
        }
        let mut context = Context::new(id.clone(), purpose)?;
        if namespace.is_some() {
            *context.ruleset_mut() = crate::shared_ruleset::Ruleset::try_new(
                id.as_str(),
                "Context-owned architectural Rules.",
                None,
                Vec::new(),
                Vec::new(),
            )?;
        }
        context.set_schema(namespace.is_some(), component_type)?;
        self.records.push(Record {
            context,
            path,
            directory,
        });
        self.records
            .sort_by(|left, right| left.directory.cmp(&right.directory));
        self.validate()?;
        let record = self
            .records
            .iter()
            .find(|record| record.path.ends_with("context.toml") && !fs.is_file(&record.path))
            .ok_or(Error::InvalidPath)?;
        write_record(fs, record)?;
        Ok(record)
    }

    pub(super) fn records(&self) -> &[Record] {
        &self.records
    }
    pub(super) fn shared(&self) -> &SharedRulesets {
        &self.shared
    }

    pub(super) fn record_for_component(&self, component: &RepositoryPath) -> Option<&Record> {
        self.component_record_index(component)
            .map(|index| &self.records[index])
    }

    pub(super) fn at(&self, user_path: &Utf8Path) -> Result<&Record, Error> {
        let path = resolve_path(&self.repo_root, user_path)?;
        self.records
            .iter()
            .filter(|record| path.starts_with(&record.directory))
            .max_by_key(|record| record.directory.components().count())
            .ok_or(Error::MissingContext(path))
    }

    pub(super) fn at_mut(&mut self, user_path: &Utf8Path) -> Result<&mut Record, Error> {
        let path = resolve_path(&self.repo_root, user_path)?;
        let index = self
            .records
            .iter()
            .enumerate()
            .filter(|(_, record)| path.starts_with(&record.directory))
            .max_by_key(|(_, record)| record.directory.components().count())
            .map(|(index, _)| index)
            .ok_or_else(|| Error::MissingContext(path.clone()))?;
        Ok(&mut self.records[index])
    }

    pub(super) fn effective(&self, user_path: &Utf8Path) -> Result<Vec<&Record>, Error> {
        let path = resolve_path(&self.repo_root, user_path)?;
        let mut records = self
            .records
            .iter()
            .filter(|record| path.starts_with(&record.directory))
            .collect::<Vec<_>>();
        if records.is_empty() {
            return Err(Error::MissingContext(path));
        }
        records.sort_by_key(|record| record.directory.components().count());
        Ok(records)
    }

    pub(super) fn descendants(&self, user_path: &Utf8Path) -> Result<Vec<&Record>, Error> {
        let path = resolve_path(&self.repo_root, user_path)?;
        Ok(self
            .records
            .iter()
            .filter(|record| record.directory.starts_with(&path))
            .collect())
    }

    pub(super) fn remove(
        &mut self,
        fs: &mut impl FileSystem,
        user_path: &Utf8Path,
    ) -> Result<(Record, Vec<ContextId>), Error> {
        let target_path = self.at(user_path)?.path.clone();
        let index = self
            .records
            .iter()
            .position(|record| record.path == target_path)
            .ok_or(Error::InvalidPath)?;
        let removed = self.records.remove(index);
        let affected = self
            .records
            .iter()
            .filter(|record| record.directory.starts_with(&removed.directory))
            .map(|record| record.context.id().clone())
            .collect();
        fs.remove_file(&removed.path).map_err(|source| Error::Io {
            path: removed.path.clone(),
            source,
        })?;
        Ok((removed, affected))
    }

    pub(super) fn validate(&self) -> Result<(), Error> {
        let mut ids = BTreeSet::new();
        for record in &self.records {
            if !ids.insert(record.context.id().clone()) {
                return Err(Error::DuplicateContext(record.context.id().to_string()));
            }
        }
        for record in &self.records {
            record.context.validate_identities()?;
            for boundary in record.context.boundaries() {
                if let Some(owner) = boundary.owner()
                    && !ids.contains(owner)
                {
                    return Err(Error::UnknownBoundaryOwner {
                        context: record.context.id().to_string(),
                        owner: owner.to_string(),
                    });
                }
            }
            for included in record.context.ruleset().includes() {
                if self.shared.get(included).is_none() {
                    return Err(Error::UnresolvedInclude {
                        path: record.path.clone(),
                        included: included.to_string(),
                    });
                }
            }
        }
        self.validate_generated_dependencies()?;
        Ok(())
    }

    fn validate_generated_dependencies(&self) -> Result<(), Error> {
        let mut edges = vec![Vec::new(); self.records.len()];
        for (consumer_index, record) in self.records.iter().enumerate() {
            for (input_name, input) in record.context().generated_inputs() {
                let Some(producer_index) = self.component_record_index(input.component()) else {
                    return Err(Error::MissingGeneratedProducer {
                        path: record.path.clone(),
                        input: input_name.to_string(),
                        component: input.component().to_string(),
                    });
                };
                let producer = &self.records[producer_index];
                if !producer
                    .context()
                    .generated_outputs()
                    .contains_key(input.output())
                {
                    return Err(Error::UnknownGeneratedOutput {
                        path: record.path.clone(),
                        input: input_name.to_string(),
                        component: input.component().to_string(),
                        output: input.output().to_string(),
                        producer_path: producer.path.clone(),
                    });
                }
                if !edges[consumer_index].contains(&producer_index) {
                    edges[consumer_index].push(producer_index);
                }
            }
        }

        let mut visiting = Vec::new();
        let mut visited = BTreeSet::new();
        for index in 0..self.records.len() {
            self.visit_dependency(index, &edges, &mut visiting, &mut visited)?;
        }
        Ok(())
    }

    fn visit_dependency(
        &self,
        index: usize,
        edges: &[Vec<usize>],
        visiting: &mut Vec<usize>,
        visited: &mut BTreeSet<usize>,
    ) -> Result<(), Error> {
        if visited.contains(&index) {
            return Ok(());
        }
        if let Some(start) = visiting.iter().position(|candidate| *candidate == index) {
            let mut cycle = visiting[start..]
                .iter()
                .map(|candidate| self.records[*candidate].path.to_string())
                .collect::<Vec<_>>();
            cycle.push(self.records[index].path.to_string());
            return Err(Error::GeneratedDependencyCycle(cycle));
        }
        visiting.push(index);
        for dependency in &edges[index] {
            self.visit_dependency(*dependency, edges, visiting, visited)?;
        }
        visiting.pop();
        visited.insert(index);
        Ok(())
    }

    fn component_record_index(&self, component: &RepositoryPath) -> Option<usize> {
        let directory = resolve_path(&self.repo_root, component.as_path()).ok()?;
        self.records
            .iter()
            .position(|record| record.directory == directory)
    }

    pub(super) fn save(&self, fs: &mut impl FileSystem, path: &Utf8Path) -> Result<(), Error> {
        let record = self
            .records
            .iter()
            .find(|record| record.path == path)
            .ok_or(Error::InvalidPath)?;
        write_record(fs, record)
    }
}

impl std::fmt::Debug for Repository {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ContextRepository")
            .field("repo_root", &self.repo_root)
            .field("record_count", &self.records.len())
            .field("shared", &self.shared)
            .finish()
    }
}

#[derive(Clone)]
pub(super) struct Record {
    context: Context,
    path: Utf8PathBuf,
    directory: Utf8PathBuf,
}

impl Record {
    pub(super) fn context(&self) -> &Context {
        &self.context
    }
    pub(super) fn context_mut(&mut self) -> &mut Context {
        &mut self.context
    }
    pub(super) fn path(&self) -> &Utf8Path {
        &self.path
    }
    pub(super) fn directory(&self) -> &Utf8Path {
        &self.directory
    }
}

impl std::fmt::Debug for Record {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ContextRecord")
            .field("context", &self.context)
            .field("path", &self.path)
            .field("directory", &self.directory)
            .finish()
    }
}

fn write_record(fs: &mut impl FileSystem, record: &Record) -> Result<(), Error> {
    let contents = boundary::render(&record.context)?;
    fs.write_string(&record.path, contents)
        .map_err(|source| Error::Io {
            path: record.path.clone(),
            source,
        })
}

pub(super) fn resolve_path(repo_root: &Utf8Path, value: &Utf8Path) -> Result<Utf8PathBuf, Error> {
    if value.is_absolute()
        || value
            .components()
            .any(|component| component.as_str() == "..")
    {
        return Err(Error::InvalidPath);
    }
    let path = if value == Utf8Path::new(".") {
        repo_root.to_path_buf()
    } else {
        repo_root.join(value)
    };
    if !path.starts_with(repo_root) {
        return Err(Error::InvalidPath);
    }
    Ok(path)
}
