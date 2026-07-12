//! Built-in shared Ruleset catalog.
//!
//! Owns catalog metadata, dependency closure, installation, updates, and the
//! exact lock connecting installed files to catalog versions.

use super::Error;
use super::boundary;
use super::domain::{Rule, Ruleset, RulesetId};
use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const LOCK_VERSION: u16 = 1;
const CATALOG_VERSION: &str = "1.0.0";

const CATALOG_FILES: &[CatalogFile] = &[
    CatalogFile::new(
        "RUST_CODING",
        "Standards for clear, safe, and maintainable Rust implementation.",
        "rust/coding.toml",
        include_str!("../../catalog/rust/coding.toml"),
    ),
    CatalogFile::new(
        "RUST_TEST",
        "Standards for Rust tests as readable executable specifications.",
        "rust/test.toml",
        include_str!("../../catalog/rust/test.toml"),
    ),
    CatalogFile::new(
        "RUST_COMMENT",
        "Standards for concise Rust documentation and meaningful comments.",
        "rust/comment.toml",
        include_str!("../../catalog/rust/comment.toml"),
    ),
    CatalogFile::new(
        "RUST_CRATE",
        "Complete coding, testing, and documentation policy for a Rust crate.",
        "rust/crate.toml",
        include_str!("../../catalog/rust/crate.toml"),
    ),
    CatalogFile::new(
        "CRUX_EFFECTS",
        "Standards for typed Crux effects and shell-owned execution.",
        "crux/effects.toml",
        include_str!("../../catalog/crux/effects.toml"),
    ),
    CatalogFile::new(
        "CRUX_MODEL",
        "Standards for explicit Crux state-machine ownership and transitions.",
        "crux/model.toml",
        include_str!("../../catalog/crux/model.toml"),
    ),
    CatalogFile::new(
        "CRUX_VIEW",
        "Standards for shell-facing Crux ViewModels and projections.",
        "crux/view.toml",
        include_str!("../../catalog/crux/view.toml"),
    ),
    CatalogFile::new(
        "CRUX_TEST",
        "Standards for testing Crux commands, effects, and model transitions.",
        "crux/test.toml",
        include_str!("../../catalog/crux/test.toml"),
    ),
    CatalogFile::new(
        "CRUX_APP",
        "Complete Rust and Crux policy for a cross-platform application.",
        "crux/app.toml",
        include_str!("../../catalog/crux/app.toml"),
    ),
];

struct CatalogFile {
    id: &'static str,
    purpose: &'static str,
    path: &'static str,
    contents: &'static str,
}

impl CatalogFile {
    const fn new(
        id: &'static str,
        purpose: &'static str,
        path: &'static str,
        contents: &'static str,
    ) -> Self {
        Self {
            id,
            purpose,
            path,
            contents,
        }
    }
}

#[derive(Clone)]
pub(super) struct Catalog {
    entries: BTreeMap<RulesetId, Entry>,
}

impl Catalog {
    pub(super) fn load() -> Result<Self, Error> {
        let mut entries = BTreeMap::new();
        for file in CATALOG_FILES {
            let source_path = Utf8Path::new(file.path);
            let ruleset =
                boundary::parse_catalog(file.contents, source_path, file.purpose, CATALOG_VERSION)?;
            if ruleset.id().as_str() != file.id {
                return Err(Error::UnknownRuleset(file.id.to_owned()));
            }
            let installed_contents = boundary::render(&ruleset)?;
            let entry = Entry {
                ruleset,
                relative_path: Utf8PathBuf::from(file.path),
                contents: installed_contents,
            };
            if entries.insert(entry.ruleset.id().clone(), entry).is_some() {
                return Err(Error::DuplicateRuleset(file.id.to_owned()));
            }
        }
        let catalog = Self { entries };
        catalog.validate()?;
        Ok(catalog)
    }

    pub(super) fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.values()
    }

    pub(super) fn get(&self, id: &str) -> Result<&Entry, Error> {
        let id = RulesetId::parse(id)?;
        self.entries
            .get(&id)
            .ok_or_else(|| Error::UnknownRuleset(id.to_string()))
    }

    pub(super) fn closure(&self, id: &RulesetId) -> Result<Vec<&Entry>, Error> {
        let mut visiting = Vec::new();
        let mut visited = BTreeSet::new();
        let mut ordered = Vec::new();
        self.collect_closure(id, &mut visiting, &mut visited, &mut ordered)?;
        Ok(ordered)
    }

    fn collect_closure<'catalog>(
        &'catalog self,
        id: &RulesetId,
        visiting: &mut Vec<RulesetId>,
        visited: &mut BTreeSet<RulesetId>,
        ordered: &mut Vec<&'catalog Entry>,
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
        let entry = self
            .entries
            .get(id)
            .ok_or_else(|| Error::UnknownRuleset(id.to_string()))?;
        visiting.push(id.clone());
        for included in entry.ruleset.includes() {
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
        ordered.push(entry);
        Ok(())
    }

    pub(super) fn resolved_rules(&self, id: &RulesetId) -> Result<Vec<(&RulesetId, &Rule)>, Error> {
        let mut rules = BTreeMap::<String, (&RulesetId, &Rule)>::new();
        for entry in self.closure(id)? {
            for rule in entry.ruleset.rules() {
                if let Some((owner, existing)) =
                    rules.insert(rule.id().to_string(), (entry.ruleset.id(), rule))
                    && existing != rule
                {
                    return Err(Error::ConflictingRule {
                        rule: rule.id().to_string(),
                        first: owner.to_string(),
                        second: entry.ruleset.id().to_string(),
                    });
                }
            }
        }
        Ok(rules.into_values().collect())
    }

    fn validate(&self) -> Result<(), Error> {
        for entry in self.entries.values() {
            self.closure(entry.ruleset.id())?;
            self.resolved_rules(entry.ruleset.id())?;
        }
        Ok(())
    }

    pub(super) fn install(
        &self,
        fs: &mut impl FileSystem,
        repo_root: &Utf8Path,
        id: &str,
    ) -> Result<Vec<RulesetId>, Error> {
        let selected = self.get(id)?;
        let entries = self.closure(selected.ruleset.id())?;
        let lock_path = repo_root.join(".rapport/rules.lock");
        let mut lock = load_lock(fs, &lock_path)?;
        preflight(fs, repo_root, &entries, &lock)?;
        let installed = write_entries(fs, repo_root, entries, &mut lock)?;
        write_lock(fs, &lock_path, &lock)?;
        Ok(installed)
    }

    pub(super) fn update(
        &self,
        fs: &mut impl FileSystem,
        repo_root: &Utf8Path,
        id: &str,
    ) -> Result<Vec<RulesetId>, Error> {
        let selected = self.get(id)?;
        let entries = self.closure(selected.ruleset.id())?;
        let lock_path = repo_root.join(".rapport/rules.lock");
        let mut lock = load_lock(fs, &lock_path)?;
        if lock.find(selected.ruleset.id()).is_none() {
            return Err(Error::NotInstalled(id.to_owned()));
        }
        preflight(fs, repo_root, &entries, &lock)?;
        let updated = write_entries(fs, repo_root, entries, &mut lock)?;
        write_lock(fs, &lock_path, &lock)?;
        Ok(updated)
    }
}

impl std::fmt::Debug for Catalog {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Catalog")
            .field("entry_count", &self.entries.len())
            .finish()
    }
}

#[derive(Clone)]
pub(super) struct Entry {
    ruleset: Ruleset,
    relative_path: Utf8PathBuf,
    contents: String,
}

impl Entry {
    pub(super) fn ruleset(&self) -> &Ruleset {
        &self.ruleset
    }

    pub(super) fn relative_path(&self) -> &Utf8Path {
        &self.relative_path
    }

    fn digest(&self) -> String {
        digest(&self.contents)
    }
}

impl std::fmt::Debug for Entry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CatalogEntry")
            .field("ruleset", &self.ruleset)
            .field("relative_path", &self.relative_path)
            .field("content_length", &self.contents.len())
            .finish()
    }
}

fn preflight(
    fs: &impl FileSystem,
    repo_root: &Utf8Path,
    entries: &[&Entry],
    lock: &LockFile,
) -> Result<(), Error> {
    for entry in entries {
        let path = repo_root.join(".rapport/rules").join(entry.relative_path());
        match lock.find(entry.ruleset.id()) {
            Some(locked) => {
                require_matching_lock(entry, locked)?;
                verify_locked_file(fs, repo_root, locked)?;
            }
            None if fs.exists(&path) => return Err(Error::FileConflict(path)),
            None => {}
        }
    }
    Ok(())
}

fn write_entries(
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
    entries: Vec<&Entry>,
    lock: &mut LockFile,
) -> Result<Vec<RulesetId>, Error> {
    let mut changed = Vec::new();
    for entry in entries {
        let path = repo_root.join(".rapport/rules").join(entry.relative_path());
        if let Some(parent) = path.parent() {
            fs.create_dir_all(parent).map_err(|source| Error::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        fs.write_string(&path, &entry.contents)
            .map_err(|source| Error::Io {
                path: path.clone(),
                source,
            })?;
        lock.replace(LockedRuleset {
            id: entry.ruleset.id().to_string(),
            catalog_version: CATALOG_VERSION.to_owned(),
            path: entry.relative_path.to_string(),
            digest: entry.digest(),
        });
        changed.push(entry.ruleset.id().clone());
    }
    changed.sort();
    Ok(changed)
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LockFile {
    version: u16,
    #[serde(default)]
    rulesets: Vec<LockedRuleset>,
}

impl LockFile {
    pub(super) fn empty() -> Self {
        Self {
            version: LOCK_VERSION,
            rulesets: Vec::new(),
        }
    }

    pub(super) fn entries(&self) -> impl Iterator<Item = &LockedRuleset> {
        self.rulesets.iter()
    }

    pub(super) fn find(&self, id: &RulesetId) -> Option<&LockedRuleset> {
        self.rulesets.iter().find(|locked| locked.id == id.as_str())
    }

    fn replace(&mut self, replacement: LockedRuleset) {
        self.rulesets.retain(|locked| locked.id != replacement.id);
        self.rulesets.push(replacement);
        self.rulesets.sort_by(|left, right| left.id.cmp(&right.id));
    }
}

impl std::fmt::Debug for LockFile {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RulesetLock")
            .field("version", &self.version)
            .field("entry_count", &self.rulesets.len())
            .finish()
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LockedRuleset {
    id: String,
    catalog_version: String,
    path: String,
    digest: String,
}

impl LockedRuleset {
    pub(super) fn id(&self) -> &str {
        &self.id
    }

    pub(super) fn path(&self) -> &str {
        &self.path
    }

    pub(super) fn catalog_version(&self) -> &str {
        &self.catalog_version
    }
}

impl std::fmt::Debug for LockedRuleset {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LockedRuleset")
            .field("id", &self.id)
            .field("catalog_version", &self.catalog_version)
            .field("path", &self.path)
            .field("digest", &"[redacted]")
            .finish()
    }
}

pub(super) fn load_lock(fs: &impl FileSystem, path: &Utf8Path) -> Result<LockFile, Error> {
    if !fs.is_file(path) {
        return Ok(LockFile::empty());
    }
    let contents = fs.read_to_string(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let lock: LockFile = toml::from_str(&contents).map_err(|source| Error::Decode {
        path: path.to_path_buf(),
        source,
    })?;
    if lock.version != LOCK_VERSION {
        return Err(Error::LockVersion(lock.version));
    }
    let mut ids = BTreeSet::new();
    let mut paths = BTreeSet::new();
    for entry in &lock.rulesets {
        if !ids.insert(entry.id.as_str())
            || !paths.insert(entry.path.as_str())
            || Utf8Path::new(&entry.path).is_absolute()
            || entry.path.split('/').any(|part| part == "..")
        {
            return Err(Error::InvalidCatalogLock(entry.id.clone()));
        }
    }
    Ok(lock)
}

pub(super) fn require_matching_lock(entry: &Entry, locked: &LockedRuleset) -> Result<(), Error> {
    if entry.ruleset.id().as_str() != locked.id || entry.relative_path.as_str() != locked.path {
        return Err(Error::InvalidCatalogLock(locked.id.clone()));
    }
    Ok(())
}

fn write_lock(fs: &mut impl FileSystem, path: &Utf8Path, lock: &LockFile) -> Result<(), Error> {
    let contents = toml_edit::ser::to_string_pretty(lock).map_err(Error::Encode)?;
    fs.write_string(path, contents).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })
}

pub(super) fn verify_locked_file(
    fs: &impl FileSystem,
    repo_root: &Utf8Path,
    locked: &LockedRuleset,
) -> Result<String, Error> {
    let path = repo_root.join(".rapport/rules").join(&locked.path);
    let contents = fs.read_to_string(&path).map_err(|source| Error::Io {
        path: path.clone(),
        source,
    })?;
    if digest(&contents) != locked.digest {
        return Err(Error::ModifiedCatalogRuleset(locked.id.clone()));
    }
    Ok(contents)
}

fn digest(contents: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(contents.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::Catalog;
    use claims::{assert_err, assert_ok};
    use rapport_files::{FileSystem, InMemoryFileSystem, Utf8Path};

    #[test]
    fn install_should_write_the_selected_ruleset_dependency_closure() {
        let catalog = assert_ok!(Catalog::load());
        let mut fs = InMemoryFileSystem::default();

        let installed = assert_ok!(catalog.install(&mut fs, Utf8Path::new("/repo"), "RUST_CRATE"));

        assert_eq!(
            installed.len(),
            4,
            "expecting the Rust aggregate and its three dependencies"
        );
        assert!(fs.is_file("/repo/.rapport/rules/rust/crate.toml"));
        assert!(fs.is_file("/repo/.rapport/rules/rust/coding.toml"));
        assert!(
            fs.read_to_string("/repo/.rapport/rules/rust/coding.toml")
                .is_ok_and(|contents| contents.contains("purpose ="))
        );
    }

    #[test]
    fn update_should_reject_locally_modified_catalog_content() {
        let catalog = assert_ok!(Catalog::load());
        let mut fs = InMemoryFileSystem::default();
        assert_ok!(catalog.install(&mut fs, Utf8Path::new("/repo"), "RUST_CRATE"));
        assert_ok!(fs.write_string("/repo/.rapport/rules/rust/coding.toml", "locally changed"));

        assert_err!(catalog.update(&mut fs, Utf8Path::new("/repo"), "RUST_CRATE"));
    }
}
