//! Context discovery, persistence, and effective inheritance.

use super::Error;
use super::boundary;
use super::domain::{Context, ContextId, Grade};
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
    ) -> Result<&Record, Error> {
        let directory = resolve_path(&self.repo_root, user_path)?;
        if !fs.is_dir(&directory) {
            return Err(Error::InvalidPath);
        }
        let path = directory.join("context.toml");
        let relative = directory
            .strip_prefix(&self.repo_root)
            .unwrap_or(&directory);
        let id = ContextId::derive(relative)?;
        if self
            .records
            .iter()
            .any(|record| record.context.id() == &id || record.path == path)
        {
            return Err(Error::DuplicateContext(id.to_string()));
        }
        self.records.push(Record {
            context: Context::new(id, purpose)?,
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

    pub(super) fn effective_grade(&self, user_path: &Utf8Path) -> Result<Grade, Error> {
        Ok(self
            .effective(user_path)?
            .into_iter()
            .filter_map(|record| record.context.minimum_grade())
            .max()
            .unwrap_or(Grade::DEFAULT))
    }

    pub(super) fn inherited_grade(&self, directory: &Utf8Path) -> Grade {
        self.records
            .iter()
            .filter(|record| {
                directory.starts_with(&record.directory) && record.directory != directory
            })
            .filter_map(|record| record.context.minimum_grade())
            .max()
            .unwrap_or(Grade::DEFAULT)
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
                self.shared.require(included)?;
            }
            for signoff in record.context.signoffs() {
                for included in signoff.included_paths() {
                    self.validate_stored_included_path(record.directory(), included)?;
                }
            }
            let inherited = self.inherited_grade(&record.directory);
            if let Some(direct) = record.context.minimum_grade()
                && direct < inherited
            {
                return Err(Error::LowerReviewGrade {
                    requested: direct.to_string(),
                    inherited: inherited.to_string(),
                });
            }
        }
        Ok(())
    }

    fn validate_stored_included_path(
        &self,
        context_directory: &Utf8Path,
        value: &str,
    ) -> Result<(), Error> {
        let relative = Utf8Path::new(value);
        let canonical = !relative.is_absolute()
            && !relative.as_str().is_empty()
            && relative.components().all(|component| {
                let part = component.as_str();
                part != "." && part != ".." && !part.is_empty()
            });
        let absolute = self.repo_root.join(relative);
        if !canonical
            || !absolute.starts_with(&self.repo_root)
            || absolute.starts_with(context_directory)
        {
            return Err(Error::InvalidIncludedPath);
        }
        Ok(())
    }

    pub(super) fn validate_included_path_existence(
        &self,
        fs: &impl FileSystem,
    ) -> Result<(), Error> {
        for record in &self.records {
            for signoff in record.context.signoffs() {
                for included in signoff.included_paths() {
                    if !fs.exists(self.repo_root.join(included)) {
                        return Err(Error::InvalidIncludedPath);
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn applicable_signoffs(
        &self,
        user_path: &Utf8Path,
    ) -> Result<Vec<SignoffMatch<'_>>, Error> {
        let path = resolve_path(&self.repo_root, user_path)?;
        let mut matches = Vec::new();
        for record in &self.records {
            for signoff in record.context.signoffs() {
                if path.starts_with(record.directory()) {
                    matches.push(SignoffMatch {
                        record,
                        signoff,
                        trigger: display_relative(&self.repo_root, record.directory()),
                    });
                    continue;
                }
                if let Some(included) = signoff
                    .included_paths()
                    .iter()
                    .find(|included| path.starts_with(self.repo_root.join(included)))
                {
                    matches.push(SignoffMatch {
                        record,
                        signoff,
                        trigger: included.clone(),
                    });
                }
            }
        }
        matches.sort_by(|left, right| left.signoff.id().cmp(right.signoff.id()));
        Ok(matches)
    }

    pub(super) fn save(&self, fs: &mut impl FileSystem, path: &Utf8Path) -> Result<(), Error> {
        let record = self
            .records
            .iter()
            .find(|record| record.path == path)
            .ok_or(Error::InvalidPath)?;
        write_record(fs, record)
    }

    pub(super) fn normalize_included_path(
        &self,
        context_directory: &Utf8Path,
        value: &Utf8Path,
        fs: &impl FileSystem,
        must_exist: bool,
    ) -> Result<String, Error> {
        if value.is_absolute() {
            return Err(Error::InvalidIncludedPath);
        }
        let context_relative = context_directory
            .strip_prefix(&self.repo_root)
            .map_err(|_| Error::InvalidIncludedPath)?;
        let mut parts = context_relative
            .as_str()
            .split('/')
            .filter(|part| !part.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        for part in value.as_str().split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    if parts.pop().is_none() {
                        return Err(Error::InvalidIncludedPath);
                    }
                }
                part => parts.push(part.to_owned()),
            }
        }
        let relative = Utf8PathBuf::from(parts.join("/"));
        let path = self.repo_root.join(&relative);
        if !path.starts_with(&self.repo_root) || (must_exist && !fs.exists(&path)) {
            return Err(Error::InvalidIncludedPath);
        }
        if relative.as_str().is_empty() || path.starts_with(context_directory) {
            return Err(Error::InvalidIncludedPath);
        }
        Ok(relative.to_string())
    }
}

pub(super) struct SignoffMatch<'record> {
    pub(super) record: &'record Record,
    pub(super) signoff: &'record super::domain::BuildSignoff,
    pub(super) trigger: String,
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

fn display_relative(root: &Utf8Path, path: &Utf8Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    if relative.as_str().is_empty() {
        ".".to_owned()
    } else {
        relative.to_string()
    }
}
