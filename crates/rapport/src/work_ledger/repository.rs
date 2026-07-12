//! Work and Task persistence.

use super::Error;
use super::domain::{TASK_SCHEMA_VERSION, Task, WORK_SCHEMA_VERSION, Work};
use directories::ProjectDirs;
use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};

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
        let work: Work = toml::from_str(&contents).map_err(|source| Error::Decode {
            path: path.clone(),
            source,
        })?;
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
        fs.write_string(&path, toml::to_string_pretty(work)?)
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

    pub(super) fn archive(
        &self,
        fs: &mut impl FileSystem,
        work: &Work,
        tasks: &[Task],
    ) -> Result<Utf8PathBuf, Error> {
        let project = ProjectDirs::from("com", "Hedge Ops", "Rapport")
            .ok_or(Error::MissingHistoryDirectory)?;
        let state = project
            .state_dir()
            .unwrap_or_else(|| project.data_local_dir());
        let history = Utf8PathBuf::from_path_buf(state.join("work").join(work.id.to_string()))
            .map_err(|_| Error::NonUtf8Path)?;
        fs.create_dir_all(&history).map_err(|source| Error::Io {
            path: history.clone(),
            source,
        })?;
        let work_archive = history.join("work.toml");
        fs.write_string(&work_archive, toml::to_string_pretty(work)?)
            .map_err(|source| Error::Io {
                path: work_archive,
                source,
            })?;
        let tasks_archive = history.join("tasks");
        fs.create_dir_all(&tasks_archive)
            .map_err(|source| Error::Io {
                path: tasks_archive.clone(),
                source,
            })?;
        for task in tasks {
            let path = tasks_archive.join(format!("{}.toml", task.id));
            fs.write_string(&path, toml::to_string_pretty(task)?)
                .map_err(|source| Error::Io { path, source })?;
        }
        self.clear_local(fs, tasks)?;
        Ok(history)
    }

    fn clear_local(&self, fs: &mut impl FileSystem, tasks: &[Task]) -> Result<(), Error> {
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
