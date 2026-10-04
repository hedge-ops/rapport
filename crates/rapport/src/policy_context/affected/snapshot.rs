//! In-memory policy trees resolved with the ordinary Context repository.

use super::super::{boundary, repository::Repository, review_policy};
use super::Error;
use rapport_files::{InMemoryFileSystem, Utf8Path, Utf8PathBuf};
use rapport_git::{Git, ObjectId};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Snapshot {
    pub(super) files: BTreeSet<Utf8PathBuf>,
    pub(super) components: BTreeMap<String, Policy>,
    repository: Repository,
    root: Utf8PathBuf,
}

impl Snapshot {
    pub(super) fn committed(
        git: &Git,
        repo: &rapport_git::Repository,
        commit: Option<&ObjectId>,
    ) -> Result<Self, Error> {
        let files = git.snapshot_paths(repo, commit)?;
        Self::load(repo.root(), files, |path| {
            Ok(git.snapshot_file(repo, commit, path)?)
        })
    }

    pub(super) fn working(git: &Git, repo: &rapport_git::Repository) -> Result<Self, Error> {
        let files = git
            .working_tree_files(repo)?
            .into_iter()
            .filter(|path| repo.root().join(path).symlink_metadata().is_ok())
            .collect();
        Self::load(repo.root(), files, |path| {
            let path = repo.root().join(path);
            std::fs::read_to_string(&path).map_err(|source| Error::Io { path, source })
        })
    }

    fn load(
        root: &Utf8Path,
        files: BTreeSet<Utf8PathBuf>,
        mut read: impl FnMut(&Utf8Path) -> Result<String, Error>,
    ) -> Result<Self, Error> {
        let mut fs = InMemoryFileSystem::default();
        fs.add_directory(root);
        let mut contexts = Vec::new();
        for path in &files {
            if path.file_name() == Some("context.toml") {
                contexts.push(root.join(path));
            } else if !(path.starts_with(".rapport/rules") && path.extension() == Some("toml")
                || path == ".rapport/rules.lock")
            {
                continue;
            }
            fs.add_file_with_contents(root.join(path), read(path)?);
        }
        let repository = Repository::load_paths(&mut fs, root, contexts)?;
        let mut components = BTreeMap::new();
        for record in repository.records() {
            let path = relative(root, record.directory());
            let markdown = review_policy(&repository, root, [Utf8Path::new(&path)])?.markdown;
            let mut sources = BTreeMap::new();
            for ancestor in repository.effective(Utf8Path::new(&path))? {
                sources.insert(
                    relative(root, ancestor.path()),
                    boundary::render(ancestor.context())?,
                );
                for id in ancestor.context().ruleset().includes() {
                    let summary = repository
                        .shared()
                        .require(id)
                        .map_err(super::super::Error::from)?;
                    for id in std::iter::once(id).chain(summary.transitive()) {
                        let source = repository
                            .shared()
                            .require(id)
                            .map_err(super::super::Error::from)?;
                        sources.insert(relative(root, source.path()), source.digest().to_owned());
                    }
                }
            }
            components.insert(path, Policy { markdown, sources });
        }
        Ok(Self {
            files,
            components,
            repository,
            root: root.to_owned(),
        })
    }

    pub(super) fn owner(&self, file: &Utf8Path) -> Result<String, Error> {
        let record = self
            .repository
            .at(file)
            .map_err(|_| Error::Unmappable(file.to_string()))?;
        Ok(relative(&self.root, record.directory()))
    }
}

pub(super) struct Policy {
    pub(super) markdown: String,
    pub(super) sources: BTreeMap<String, String>,
}

fn relative(root: &Utf8Path, path: &Utf8Path) -> String {
    let path = path.strip_prefix(root).unwrap_or(path).as_str();
    if path.is_empty() {
        ".".into()
    } else {
        path.into()
    }
}
