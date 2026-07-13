//! Repository-owned shared Rulesets.
//!
//! Owns discovery, graph validation, repository mutations, and the distinction
//! between Git-versioned repository Rulesets and locked catalog installations.

use super::Error;
use super::boundary;
use super::catalog::{self, Catalog};
use super::domain::{NewRule, Rule, RuleUpdate, Ruleset, RulesetId};
use crate::repository_files::find_named_files;
use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Store<'store, F> {
    fs: &'store mut F,
    repo_root: &'store Utf8Path,
    catalog: &'store Catalog,
}

impl<'store, F: FileSystem> Store<'store, F> {
    pub(super) fn new(
        fs: &'store mut F,
        repo_root: &'store Utf8Path,
        catalog: &'store Catalog,
    ) -> Self {
        Self {
            fs,
            repo_root,
            catalog,
        }
    }

    pub(super) fn load(&self) -> Result<Snapshot, Error> {
        Snapshot::load(self.fs, self.repo_root, self.catalog)
    }

    pub(super) fn init(&mut self, id: &str, purpose: &str) -> Result<StoredRuleset, Error> {
        let ruleset = Ruleset::try_new(id, purpose, None, Vec::new(), Vec::new())?;
        let path = boundary::path_for_repository_ruleset(self.repo_root, &ruleset);
        let mut snapshot = self.load()?;
        if snapshot.entries.contains_key(ruleset.id()) || self.fs.exists(&path) {
            return Err(Error::DuplicateRuleset(ruleset.id().to_string()));
        }
        let stored = StoredRuleset {
            ruleset,
            path,
            source: Source::Repository,
        };
        snapshot
            .entries
            .insert(stored.ruleset.id().clone(), stored.clone());
        snapshot.validate()?;
        self.write(&stored)?;
        Ok(stored)
    }

    pub(super) fn set_purpose(&mut self, id: &str, purpose: &str) -> Result<StoredRuleset, Error> {
        self.mutate(id, |ruleset| ruleset.set_purpose(purpose))
    }

    pub(super) fn remove(&mut self, id: &str) -> Result<StoredRuleset, Error> {
        let mut snapshot = self.load()?;
        let id = RulesetId::parse(id)?;
        let stored = snapshot
            .entries
            .get(&id)
            .cloned()
            .ok_or_else(|| Error::UnknownRuleset(id.to_string()))?;
        stored.require_repository_owned()?;
        for candidate in snapshot.entries.values() {
            if candidate.ruleset.includes().contains(&id) {
                return Err(Error::RulesetUses {
                    owner: candidate.ruleset.id().to_string(),
                    used: id.to_string(),
                });
            }
        }
        self.require_no_context_uses(&id)?;
        snapshot.entries.remove(&id);
        snapshot.validate()?;
        self.fs
            .remove_file(&stored.path)
            .map_err(|source| Error::Io {
                path: stored.path.clone(),
                source,
            })?;
        Ok(stored)
    }

    pub(super) fn compose(&mut self, owner: &str, included: &str) -> Result<StoredRuleset, Error> {
        let included = RulesetId::parse(included)?;
        let snapshot = self.load()?;
        if !snapshot.entries.contains_key(&included) {
            return Err(Error::UnknownRuleset(included.to_string()));
        }
        drop(snapshot);
        self.mutate(owner, |ruleset| {
            ruleset.compose(included);
            Ok(())
        })
    }

    pub(super) fn uncompose(
        &mut self,
        owner: &str,
        included: &str,
    ) -> Result<StoredRuleset, Error> {
        let included = RulesetId::parse(included)?;
        self.mutate(owner, |ruleset| {
            if ruleset.uncompose(&included) {
                Ok(())
            } else {
                Err(Error::UnknownRuleset(included.to_string()))
            }
        })
    }

    pub(super) fn add_rule(&mut self, owner: &str, rule: NewRule) -> Result<StoredRuleset, Error> {
        self.mutate(owner, |ruleset| ruleset.add_rule(rule))
    }

    pub(super) fn update_rule(
        &mut self,
        owner: &str,
        id: &str,
        update: RuleUpdate,
    ) -> Result<StoredRuleset, Error> {
        self.mutate(owner, |ruleset| ruleset.update_rule(id, update))
    }

    pub(super) fn remove_rule(&mut self, owner: &str, id: &str) -> Result<StoredRuleset, Error> {
        self.mutate(owner, |ruleset| ruleset.remove_rule(id))
    }

    fn mutate(
        &mut self,
        id: &str,
        change: impl FnOnce(&mut Ruleset) -> Result<(), Error>,
    ) -> Result<StoredRuleset, Error> {
        let mut snapshot = self.load()?;
        let id = RulesetId::parse(id)?;
        let stored = snapshot
            .entries
            .get_mut(&id)
            .ok_or_else(|| Error::UnknownRuleset(id.to_string()))?;
        stored.require_repository_owned()?;
        change(&mut stored.ruleset)?;
        let changed = stored.clone();
        snapshot.validate()?;
        self.write(&changed)?;
        Ok(changed)
    }

    fn write(&mut self, stored: &StoredRuleset) -> Result<(), Error> {
        if let Some(parent) = stored.path.parent() {
            self.fs.create_dir_all(parent).map_err(|source| Error::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let contents = boundary::render(&stored.ruleset)?;
        self.fs
            .write_string(&stored.path, contents)
            .map_err(|source| Error::Io {
                path: stored.path.clone(),
                source,
            })
    }

    fn require_no_context_uses(&self, id: &RulesetId) -> Result<(), Error> {
        let paths =
            find_named_files(self.fs, self.repo_root, "context.toml").map_err(|source| {
                Error::Io {
                    path: self.repo_root.to_path_buf(),
                    source,
                }
            })?;
        for path in paths {
            let contents = self.fs.read_to_string(&path).map_err(|source| Error::Io {
                path: path.clone(),
                source,
            })?;
            let document: toml::Value =
                toml::from_str(&contents).map_err(|source| Error::Decode {
                    path: path.clone(),
                    source,
                })?;
            if value_contains(&document, id.as_str()) {
                return Err(Error::ContextUses {
                    path,
                    ruleset: id.to_string(),
                });
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
pub(super) struct Snapshot {
    entries: BTreeMap<RulesetId, StoredRuleset>,
}

impl Snapshot {
    fn load(fs: &impl FileSystem, repo_root: &Utf8Path, catalog: &Catalog) -> Result<Self, Error> {
        let lock_path = repo_root.join(".rapport/rules.lock");
        let lock = catalog::load_lock(fs, &lock_path)?;
        let mut entries = BTreeMap::new();
        let mut locked_paths = BTreeSet::new();
        for locked in lock.entries() {
            let catalog_entry = catalog.get(locked.id())?;
            catalog::require_matching_lock(catalog_entry, locked)?;
            let path = repo_root.join(".rapport/rules").join(locked.path());
            let contents = catalog::verify_locked_file(fs, repo_root, locked)?;
            let ruleset = boundary::parse_catalog(
                &contents,
                &path,
                catalog_entry.ruleset().purpose(),
                locked.catalog_version(),
            )?;
            let stored = StoredRuleset {
                ruleset,
                path,
                source: Source::Catalog {
                    version: locked.catalog_version().to_owned(),
                },
            };
            locked_paths.insert(Utf8PathBuf::from(locked.path()));
            insert_unique(&mut entries, stored)?;
        }

        let rules_root = repo_root.join(".rapport/rules");
        for path in collect_toml_files(fs, &rules_root)? {
            let relative = path
                .strip_prefix(&rules_root)
                .unwrap_or(&path)
                .to_path_buf();
            if locked_paths.contains(&relative) {
                continue;
            }
            let contents = fs.read_to_string(&path).map_err(|source| Error::Io {
                path: path.clone(),
                source,
            })?;
            let ruleset = boundary::parse_repository(&contents, &path)?;
            insert_unique(
                &mut entries,
                StoredRuleset {
                    ruleset,
                    path,
                    source: Source::Repository,
                },
            )?;
        }
        let snapshot = Self { entries };
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub(super) fn entries(&self) -> impl Iterator<Item = &StoredRuleset> {
        self.entries.values()
    }

    pub(super) fn get(&self, id: &str) -> Result<&StoredRuleset, Error> {
        let id = RulesetId::parse(id)?;
        self.entries
            .get(&id)
            .ok_or_else(|| Error::UnknownRuleset(id.to_string()))
    }

    pub(super) fn closure(&self, id: &RulesetId) -> Result<Vec<&StoredRuleset>, Error> {
        let mut visiting = Vec::new();
        let mut visited = BTreeSet::new();
        let mut ordered = Vec::new();
        self.collect_closure(id, &mut visiting, &mut visited, &mut ordered)?;
        Ok(ordered)
    }

    fn collect_closure<'snapshot>(
        &'snapshot self,
        id: &RulesetId,
        visiting: &mut Vec<RulesetId>,
        visited: &mut BTreeSet<RulesetId>,
        ordered: &mut Vec<&'snapshot StoredRuleset>,
    ) -> Result<(), Error> {
        if visited.contains(id) {
            return Ok(());
        }
        if let Some(start) = visiting.iter().position(|candidate| candidate == id) {
            let mut cycle = visiting[start..]
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            cycle.push(id.to_string());
            return Err(Error::IncludeCycle(cycle));
        }
        let stored = self
            .entries
            .get(id)
            .ok_or_else(|| Error::UnknownRuleset(id.to_string()))?;
        visiting.push(id.clone());
        for included in stored.ruleset.includes() {
            if !self.entries.contains_key(included) {
                return Err(Error::MissingInclude {
                    owner: id.to_string(),
                    included: included.to_string(),
                });
            }
            self.collect_closure(included, visiting, visited, ordered)?;
        }
        visiting.pop();
        visited.insert(id.clone());
        ordered.push(stored);
        Ok(())
    }

    pub(super) fn resolved_rules(&self, id: &RulesetId) -> Result<Vec<(&RulesetId, &Rule)>, Error> {
        let mut rules = BTreeMap::<String, (&RulesetId, &Rule)>::new();
        for stored in self.closure(id)? {
            for rule in stored.ruleset.rules() {
                if let Some((owner, existing)) =
                    rules.insert(rule.id().to_string(), (stored.ruleset.id(), rule))
                    && existing != rule
                {
                    return Err(Error::ConflictingRule {
                        rule: rule.id().to_string(),
                        first: owner.to_string(),
                        second: stored.ruleset.id().to_string(),
                    });
                }
            }
        }
        Ok(rules.into_values().collect())
    }

    fn validate(&self) -> Result<(), Error> {
        for stored in self.entries.values() {
            self.closure(stored.ruleset.id())?;
            self.resolved_rules(stored.ruleset.id())?;
        }
        Ok(())
    }
}

impl std::fmt::Debug for Snapshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RulesetSnapshot")
            .field("entry_count", &self.entries.len())
            .finish()
    }
}

#[derive(Clone)]
pub(super) struct StoredRuleset {
    ruleset: Ruleset,
    path: Utf8PathBuf,
    source: Source,
}

impl StoredRuleset {
    pub(super) fn ruleset(&self) -> &Ruleset {
        &self.ruleset
    }

    pub(super) fn path(&self) -> &Utf8Path {
        &self.path
    }

    pub(super) fn source(&self) -> &Source {
        &self.source
    }

    fn require_repository_owned(&self) -> Result<(), Error> {
        match self.source {
            Source::Repository => Ok(()),
            Source::Catalog { .. } => Err(Error::CatalogOwned(self.ruleset.id().to_string())),
        }
    }
}

impl std::fmt::Debug for StoredRuleset {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StoredRuleset")
            .field("ruleset", &self.ruleset)
            .field("path", &self.path)
            .field("source", &self.source)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, derive_more::Display)]
pub(super) enum Source {
    #[display("repository")]
    Repository,
    #[display("catalog {version}")]
    Catalog { version: String },
}

fn insert_unique(
    entries: &mut BTreeMap<RulesetId, StoredRuleset>,
    stored: StoredRuleset,
) -> Result<(), Error> {
    let id = stored.ruleset.id().clone();
    if entries.insert(id.clone(), stored).is_some() {
        return Err(Error::DuplicateRuleset(id.to_string()));
    }
    Ok(())
}

fn collect_toml_files(
    fs: &impl FileSystem,
    directory: &Utf8Path,
) -> Result<Vec<Utf8PathBuf>, Error> {
    if !fs.is_dir(directory) {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    collect_toml_files_into(fs, directory, &mut paths)?;
    paths.sort();
    Ok(paths)
}

fn collect_toml_files_into(
    fs: &impl FileSystem,
    directory: &Utf8Path,
    paths: &mut Vec<Utf8PathBuf>,
) -> Result<(), Error> {
    for path in fs.read_dir(directory).map_err(|source| Error::Io {
        path: directory.to_path_buf(),
        source,
    })? {
        if fs.is_dir(&path) {
            collect_toml_files_into(fs, &path, paths)?;
        } else if path.extension() == Some("toml") {
            paths.push(path);
        }
    }
    Ok(())
}

fn value_contains(value: &toml::Value, expected: &str) -> bool {
    match value {
        toml::Value::String(value) => value == expected,
        toml::Value::Array(values) => values.iter().any(|value| value_contains(value, expected)),
        toml::Value::Table(values) => values.values().any(|value| value_contains(value, expected)),
        toml::Value::Integer(_)
        | toml::Value::Float(_)
        | toml::Value::Boolean(_)
        | toml::Value::Datetime(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{Snapshot, Store};
    use crate::shared_ruleset::catalog::Catalog;
    use claims::{assert_err, assert_ok};
    use rapport_files::{FileSystem, InMemoryFileSystem, Utf8Path};

    #[test]
    fn compose_should_reject_cycles_before_writing() {
        let catalog = assert_ok!(Catalog::load());
        let mut fs = InMemoryFileSystem::default();
        let mut store = Store::new(&mut fs, Utf8Path::new("/repo"), &catalog);
        assert_ok!(store.init("CODE", "Base coding policy."));
        assert_ok!(store.init("APP", "Application policy."));
        assert_ok!(store.compose("APP", "CODE"));

        assert_err!(store.compose("CODE", "APP"));
        let code = assert_ok!(fs.read_to_string("/repo/.rapport/rules/code.toml"));
        assert!(
            !code.contains("APP"),
            "expecting a rejected cycle not to mutate repository state"
        );
    }

    #[test]
    fn snapshot_should_resolve_catalog_and_repository_rulesets_together() {
        let catalog = assert_ok!(Catalog::load());
        let mut fs = InMemoryFileSystem::default();
        assert_ok!(catalog.install(&mut fs, Utf8Path::new("/repo"), "RUST_CRATE"));
        let mut store = Store::new(&mut fs, Utf8Path::new("/repo"), &catalog);
        assert_ok!(store.init("APP", "Application policy."));
        assert_ok!(store.compose("APP", "RUST_CRATE"));

        let snapshot = assert_ok!(Snapshot::load(&fs, Utf8Path::new("/repo"), &catalog));
        let app = assert_ok!(snapshot.get("APP"));
        let rules = assert_ok!(snapshot.resolved_rules(app.ruleset().id()));

        assert_eq!(
            rules.len(),
            64,
            "expecting the repository aggregate to resolve every Rust Rule once"
        );
    }
}
