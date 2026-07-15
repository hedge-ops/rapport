//! Work and Task persistence.
//!
//! This module owns atomic active Work and Task files; domain types own state invariants and history owns finalized records.

use super::Error;
use super::domain::{
    RequestSource, TASK_SCHEMA_VERSION, Task, WORK_SCHEMA_VERSION, Work, WorkOutcome,
    WorkOutcomeKind,
};
use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};
use rapport_git::{BranchName, ObjectId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredWork {
    version: u16,
    id: uuid::Uuid,
    repository: String,
    title: String,
    description: String,
    request: RequestSource,
    source_branch: String,
    target_branch: String,
    starting_source: String,
    starting_target: String,
    latest_checkpoint: Option<String>,
    #[serde(default)]
    develop_completed_checkpoint: Option<String>,
    #[serde(default)]
    development_sequence: Vec<String>,
    next_task: u32,
    #[serde(default = "default_counter")]
    next_finding: u32,
    created_at: String,
    outcome: Option<StoredWorkOutcome>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredWorkOutcome {
    kind: WorkOutcomeKind,
    at: String,
    summary: String,
    source_commit: String,
    target_commit: String,
}

fn default_counter() -> u32 {
    1
}

impl TryFrom<StoredWork> for Work {
    type Error = Error;

    fn try_from(stored: StoredWork) -> Result<Self, Self::Error> {
        Ok(Self {
            version: stored.version,
            id: stored.id,
            repository: stored.repository,
            title: stored.title,
            description: stored.description,
            request: stored.request,
            source_branch: BranchName::new(stored.source_branch)?,
            target_branch: BranchName::new(stored.target_branch)?,
            starting_source: ObjectId::new(stored.starting_source)?,
            starting_target: ObjectId::new(stored.starting_target)?,
            latest_checkpoint: stored.latest_checkpoint.map(ObjectId::new).transpose()?,
            develop_completed_checkpoint: stored
                .develop_completed_checkpoint
                .map(ObjectId::new)
                .transpose()?,
            development_sequence: stored.development_sequence,
            next_task: stored.next_task,
            next_finding: stored.next_finding,
            created_at: stored.created_at,
            outcome: stored.outcome.map(WorkOutcome::try_from).transpose()?,
        })
    }
}

impl TryFrom<StoredWorkOutcome> for WorkOutcome {
    type Error = Error;

    fn try_from(stored: StoredWorkOutcome) -> Result<Self, Self::Error> {
        Ok(Self {
            kind: stored.kind,
            at: stored.at,
            summary: stored.summary,
            source_commit: ObjectId::new(stored.source_commit)?,
            target_commit: ObjectId::new(stored.target_commit)?,
        })
    }
}

impl From<&Work> for StoredWork {
    fn from(work: &Work) -> Self {
        Self {
            version: work.version,
            id: work.id,
            repository: work.repository.clone(),
            title: work.title.clone(),
            description: work.description.clone(),
            request: work.request.clone(),
            source_branch: work.source_branch.as_str().to_owned(),
            target_branch: work.target_branch.as_str().to_owned(),
            starting_source: work.starting_source.as_str().to_owned(),
            starting_target: work.starting_target.as_str().to_owned(),
            latest_checkpoint: work
                .latest_checkpoint
                .as_ref()
                .map(|checkpoint| checkpoint.as_str().to_owned()),
            develop_completed_checkpoint: work
                .develop_completed_checkpoint
                .as_ref()
                .map(|checkpoint| checkpoint.as_str().to_owned()),
            development_sequence: work.development_sequence.clone(),
            next_task: work.next_task,
            next_finding: work.next_finding,
            created_at: work.created_at.clone(),
            outcome: work.outcome.as_ref().map(StoredWorkOutcome::from),
        }
    }
}

impl From<&WorkOutcome> for StoredWorkOutcome {
    fn from(outcome: &WorkOutcome) -> Self {
        Self {
            kind: outcome.kind,
            at: outcome.at.clone(),
            summary: outcome.summary.clone(),
            source_commit: outcome.source_commit.as_str().to_owned(),
            target_commit: outcome.target_commit.as_str().to_owned(),
        }
    }
}

pub(super) fn decode_work(contents: &str, path: &Utf8Path) -> Result<Work, Error> {
    let stored = toml::from_str::<StoredWork>(contents).map_err(|source| Error::Decode {
        path: path.to_path_buf(),
        source,
    })?;
    stored.try_into()
}

pub(super) fn encode_work(work: &Work) -> Result<String, Error> {
    Ok(toml::to_string_pretty(&StoredWork::from(work))?)
}

#[derive(Debug, Clone)]
pub(super) struct Store {
    root: Utf8PathBuf,
}

impl Store {
    pub(super) fn new(root: impl Into<Utf8PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn rapport_dir(&self) -> Utf8PathBuf {
        self.root.join(".rapport")
    }

    fn work_path(&self) -> Utf8PathBuf {
        self.rapport_dir().join("work.toml")
    }

    fn tasks_dir(&self) -> Utf8PathBuf {
        self.rapport_dir().join("tasks")
    }

    fn task_path(&self, id: &str) -> Utf8PathBuf {
        self.tasks_dir().join(format!("{id}.toml"))
    }

    pub(super) fn load_work(&self, fs: &impl FileSystem) -> Result<Option<Work>, Error> {
        let path = self.work_path();
        if !fs.is_file(&path) {
            return Ok(None);
        }
        let contents = fs.read_to_string(&path).map_err(|source| Error::Io {
            path: path.clone(),
            source,
        })?;
        let work = decode_work(&contents, &path)?;
        if work.version != WORK_SCHEMA_VERSION {
            return Err(Error::SchemaVersion { kind: "Work", path });
        }
        Ok(Some(work))
    }

    pub(super) fn require_work(&self, fs: &impl FileSystem) -> Result<Work, Error> {
        self.load_work(fs)?.ok_or(Error::MissingWork)
    }

    pub(super) fn save_work(&self, fs: &mut impl FileSystem, work: &Work) -> Result<(), Error> {
        let path = self.work_path();
        fs.create_dir_all(self.rapport_dir())
            .map_err(|source| Error::Io {
                path: path.clone(),
                source,
            })?;
        fs.write_string(&path, encode_work(work)?)
            .map_err(|source| Error::Io { path, source })
    }

    pub(super) fn load_tasks(&self, fs: &impl FileSystem) -> Result<Vec<Task>, Error> {
        let directory = self.tasks_dir();
        if !fs.is_dir(&directory) {
            return Ok(Vec::new());
        }
        let mut tasks = Vec::new();
        for path in fs.read_dir(&directory).map_err(|source| Error::Io {
            path: directory.clone(),
            source,
        })? {
            if path.extension() != Some("toml") {
                continue;
            }
            let contents = fs.read_to_string(&path).map_err(|source| Error::Io {
                path: path.clone(),
                source,
            })?;
            let task: Task = toml::from_str(&contents).map_err(|source| Error::Decode {
                path: path.clone(),
                source,
            })?;
            if task.version != TASK_SCHEMA_VERSION || self.task_path(&task.id) != path {
                return Err(Error::SchemaVersion { kind: "Task", path });
            }
            tasks.push(task);
        }
        tasks.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(tasks)
    }

    pub(super) fn save_task(&self, fs: &mut impl FileSystem, task: &Task) -> Result<(), Error> {
        let path = self.task_path(&task.id);
        fs.create_dir_all(self.tasks_dir())
            .map_err(|source| Error::Io {
                path: path.clone(),
                source,
            })?;
        fs.write_string(&path, toml::to_string_pretty(task)?)
            .map_err(|source| Error::Io { path, source })
    }

    pub(super) fn save_work_and_task(
        &self,
        fs: &mut impl FileSystem,
        work: &Work,
        task: &Task,
    ) -> Result<(), Error> {
        self.save_work_and_tasks(fs, work, std::slice::from_ref(task))
    }

    pub(super) fn save_work_and_tasks(
        &self,
        fs: &mut impl FileSystem,
        work: &Work,
        tasks: &[Task],
    ) -> Result<(), Error> {
        let work_path = self.work_path();
        let work_before = read_optional(fs, &work_path)?;
        let task_paths = tasks
            .iter()
            .map(|task| self.task_path(&task.id))
            .collect::<Vec<_>>();
        let task_before = task_paths
            .iter()
            .map(|path| read_optional(fs, path).map(|contents| (path.clone(), contents)))
            .collect::<Result<Vec<_>, _>>()?;
        let result = self.save_work(fs, work).and_then(|()| {
            for task in tasks {
                self.save_task(fs, task)?;
            }
            Ok(())
        });
        if result.is_err() {
            restore(fs, &work_path, work_before.as_deref());
            for (path, contents) in task_before {
                restore(fs, &path, contents.as_deref());
            }
        }
        result
    }

    pub(super) fn save_tasks(&self, fs: &mut impl FileSystem, tasks: &[Task]) -> Result<(), Error> {
        let task_paths = tasks
            .iter()
            .map(|task| self.task_path(&task.id))
            .collect::<Vec<_>>();
        let before = task_paths
            .iter()
            .map(|path| read_optional(fs, path).map(|contents| (path.clone(), contents)))
            .collect::<Result<Vec<_>, _>>()?;
        for (index, task) in tasks.iter().enumerate() {
            if let Err(error) = self.save_task(fs, task) {
                for (path, contents) in before.into_iter().take(index + 1) {
                    restore(fs, &path, contents.as_deref());
                }
                return Err(error);
            }
        }
        Ok(())
    }

    pub(super) fn clear_local(
        &self,
        fs: &mut impl FileSystem,
        tasks: &[Task],
    ) -> Result<(), Error> {
        for task in tasks {
            let path = self.task_path(&task.id);
            if fs.is_file(&path) {
                fs.remove_file(&path)
                    .map_err(|source| Error::Io { path, source })?;
            }
        }
        let path = self.work_path();
        fs.remove_file(&path)
            .map_err(|source| Error::Io { path, source })
    }
}

fn read_optional(fs: &impl FileSystem, path: &Utf8Path) -> Result<Option<String>, Error> {
    if fs.is_file(path) {
        fs.read_to_string(path)
            .map(Some)
            .map_err(|source| Error::Io {
                path: path.to_path_buf(),
                source,
            })
    } else {
        Ok(None)
    }
}

fn restore(fs: &mut impl FileSystem, path: &Utf8Path, contents: Option<&str>) {
    if let Some(contents) = contents {
        let _ = fs.write_string(path, contents);
    } else if fs.is_file(path) {
        let _ = fs.remove_file(path);
    }
}
