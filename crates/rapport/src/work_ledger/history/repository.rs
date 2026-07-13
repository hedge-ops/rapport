//! Atomic persistence for finalized Work History.
//!
//! This module owns global archive publication and resumable cleanup of active repository-local state.

use super::super::Error;
use super::super::domain::{TASK_SCHEMA_VERSION, Task, WORK_SCHEMA_VERSION, Work};
use super::super::repository::{Store, decode_work, encode_work};
#[cfg(not(test))]
use directories::ProjectDirs;
use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};
use std::fmt;
use uuid::Uuid;

#[derive(Clone)]
pub(in crate::work_ledger) struct HistoryStore {
    pub(super) root: Utf8PathBuf,
}

impl fmt::Debug for HistoryStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HistoryStore")
            .field("root", &"[redacted]")
            .finish()
    }
}

impl HistoryStore {
    #[cfg_attr(
        test,
        expect(
            clippy::unnecessary_wraps,
            reason = "production platform state resolution is fallible while tests use an isolated repository path"
        )
    )]
    pub(in crate::work_ledger) fn new(repository_root: &Utf8Path) -> Result<Self, Error> {
        let _ = repository_root;
        #[cfg(test)]
        let root = repository_root.join(".rapport/test-history/work");
        #[cfg(not(test))]
        let root = {
            let project = ProjectDirs::from("com", "Hedge Ops", "Rapport")
                .ok_or(Error::MissingHistoryDirectory)?;
            let state = project
                .state_dir()
                .unwrap_or_else(|| project.data_local_dir());
            Utf8PathBuf::from_path_buf(state.join("work")).map_err(|_| Error::NonUtf8Path)?
        };
        Ok(Self { root })
    }

    pub(in crate::work_ledger) fn archive(
        &self,
        fs: &mut impl FileSystem,
        active: &Store,
        work: &Work,
        tasks: &[Task],
    ) -> Result<Utf8PathBuf, Error> {
        if work.outcome.is_none() {
            return Err(Error::UnfinalizedHistory);
        }
        let target = self.root.join(work.id.to_string());
        if fs.exists(&target) {
            let archived = Self::load_record(fs, &target)?;
            let current_tasks_match = tasks.iter().all(|task| archived.tasks.contains(task));
            if archived.work != *work || !current_tasks_match {
                return Err(Error::HistoryConflict(work.id.to_string()));
            }
            active.clear_local(fs, &archived.tasks)?;
            return Ok(target);
        }

        fs.create_dir_all(&self.root).map_err(|source| Error::Io {
            path: self.root.clone(),
            source,
        })?;
        let pending = self
            .root
            .join(format!(".{}.pending-{}", work.id, Uuid::new_v4()));
        let publish = Self::write_record(fs, &pending, work, tasks).and_then(|()| {
            fs.rename(&pending, &target).map_err(|source| Error::Io {
                path: target.clone(),
                source,
            })
        });
        if let Err(error) = publish {
            if fs.is_dir(&pending) {
                let _ = fs.remove_dir_all(&pending);
            }
            return Err(error);
        }

        active.clear_local(fs, tasks)?;
        Ok(target)
    }

    pub(super) fn records(&self, fs: &impl FileSystem) -> Result<Vec<HistoryRecord>, Error> {
        if !fs.is_dir(&self.root) {
            return Ok(Vec::new());
        }
        let mut records = Vec::new();
        for path in fs.read_dir(&self.root).map_err(|source| Error::Io {
            path: self.root.clone(),
            source,
        })? {
            if !fs.is_dir(&path) || record_id(&path).is_none() {
                continue;
            }
            records.push(Self::load_record(fs, &path)?);
        }
        records.sort_by(|left, right| {
            let left_outcome = left
                .work
                .outcome
                .as_ref()
                .map(|outcome| outcome.at.as_str());
            let right_outcome = right
                .work
                .outcome
                .as_ref()
                .map(|outcome| outcome.at.as_str());
            right_outcome
                .cmp(&left_outcome)
                .then_with(|| right.work.id.cmp(&left.work.id))
        });
        Ok(records)
    }

    pub(super) fn resolve(
        &self,
        fs: &impl FileSystem,
        prefix: &str,
    ) -> Result<HistoryRecord, Error> {
        let prefix = prefix.trim().to_ascii_lowercase();
        if prefix.is_empty() {
            return Err(Error::MissingHistory(prefix));
        }
        let mut matches = self
            .records(fs)?
            .into_iter()
            .filter(|record| record.work.id.to_string().starts_with(&prefix));
        let record = matches
            .next()
            .ok_or_else(|| Error::MissingHistory(prefix.clone()))?;
        if matches.next().is_some() {
            return Err(Error::AmbiguousHistory(prefix));
        }
        Ok(record)
    }

    fn load_record(fs: &impl FileSystem, path: &Utf8Path) -> Result<HistoryRecord, Error> {
        let work_path = path.join("work.toml");
        let work_contents = fs.read_to_string(&work_path).map_err(|source| Error::Io {
            path: work_path.clone(),
            source,
        })?;
        let work = decode_work(&work_contents, &work_path)?;
        let expected_work_id = work.id.to_string();
        if work.version != WORK_SCHEMA_VERSION
            || path.file_name() != Some(expected_work_id.as_str())
        {
            return Err(Error::SchemaVersion {
                kind: "Work History",
                path: work_path,
            });
        }
        if work.outcome.is_none() {
            return Err(Error::UnfinalizedHistory);
        }

        let tasks_dir = path.join("tasks");
        let mut tasks = Vec::new();
        if fs.is_dir(&tasks_dir) {
            for task_path in fs.read_dir(&tasks_dir).map_err(|source| Error::Io {
                path: tasks_dir.clone(),
                source,
            })? {
                if task_path.extension() != Some("toml") {
                    continue;
                }
                let contents = fs.read_to_string(&task_path).map_err(|source| Error::Io {
                    path: task_path.clone(),
                    source,
                })?;
                let task: Task = toml::from_str(&contents).map_err(|source| Error::Decode {
                    path: task_path.clone(),
                    source,
                })?;
                let expected_task_file = format!("{}.toml", task.id);
                if task.version != TASK_SCHEMA_VERSION
                    || task_path.file_name() != Some(expected_task_file.as_str())
                {
                    return Err(Error::SchemaVersion {
                        kind: "Task History",
                        path: task_path,
                    });
                }
                tasks.push(task);
            }
        }
        tasks.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(HistoryRecord {
            path: path.to_path_buf(),
            work,
            tasks,
        })
    }

    fn write_record(
        fs: &mut impl FileSystem,
        path: &Utf8Path,
        work: &Work,
        tasks: &[Task],
    ) -> Result<(), Error> {
        fs.create_dir_all(path).map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let work_path = path.join("work.toml");
        fs.write_string(&work_path, encode_work(work)?)
            .map_err(|source| Error::Io {
                path: work_path,
                source,
            })?;
        let tasks_path = path.join("tasks");
        fs.create_dir_all(&tasks_path).map_err(|source| Error::Io {
            path: tasks_path.clone(),
            source,
        })?;
        for task in tasks {
            let task_path = tasks_path.join(format!("{}.toml", task.id));
            fs.write_string(&task_path, toml::to_string_pretty(task)?)
                .map_err(|source| Error::Io {
                    path: task_path,
                    source,
                })?;
        }
        Ok(())
    }
}

#[derive(Clone)]
pub(super) struct HistoryRecord {
    pub(super) path: Utf8PathBuf,
    pub(super) work: Work,
    pub(super) tasks: Vec<Task>,
}

impl fmt::Debug for HistoryRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HistoryRecord")
            .field("path", &"[redacted]")
            .field("work", &self.work)
            .field("task_count", &self.tasks.len())
            .finish()
    }
}

fn record_id(path: &Utf8Path) -> Option<Uuid> {
    let name = path.file_name()?;
    let id = Uuid::parse_str(name).ok()?;
    (id.to_string() == name).then_some(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::work_ledger::domain::{
        RequestKind, RequestSource, TaskStatus, WorkOutcomeKind, Workflow,
    };
    use claims::{assert_err, assert_ok, assert_some};
    use rapport_files::InMemoryFileSystem;
    use rapport_git::{BranchName, ObjectId};
    use std::io;

    const INITIAL_OBJECT_ID: &str = "1111111111111111111111111111111111111111";
    const CHECKPOINT_OBJECT_ID: &str = "2222222222222222222222222222222222222222";
    const LATER_OBJECT_ID: &str = "3333333333333333333333333333333333333333";

    fn branch(value: &str) -> BranchName {
        assert_ok!(BranchName::new(value))
    }

    fn object_id(value: &str) -> ObjectId {
        assert_ok!(ObjectId::new(value))
    }

    #[derive(Debug, Default)]
    struct FailingPublishFileSystem {
        inner: InMemoryFileSystem,
    }

    impl FileSystem for FailingPublishFileSystem {
        fn is_dir(&self, path: impl AsRef<Utf8Path>) -> bool {
            self.inner.is_dir(path)
        }

        fn is_file(&self, path: impl AsRef<Utf8Path>) -> bool {
            self.inner.is_file(path)
        }

        fn read_to_string(&self, path: impl AsRef<Utf8Path>) -> io::Result<String> {
            self.inner.read_to_string(path)
        }

        fn read_dir(&self, path: impl AsRef<Utf8Path>) -> io::Result<Vec<Utf8PathBuf>> {
            self.inner.read_dir(path)
        }

        fn create_dir_all(&mut self, path: impl AsRef<Utf8Path>) -> io::Result<()> {
            self.inner.create_dir_all(path)
        }

        fn write_string(
            &mut self,
            path: impl AsRef<Utf8Path>,
            contents: impl AsRef<str>,
        ) -> io::Result<()> {
            self.inner.write_string(path, contents)
        }

        fn append_line(
            &mut self,
            path: impl AsRef<Utf8Path>,
            line: impl AsRef<str>,
        ) -> io::Result<()> {
            self.inner.append_line(path, line)
        }

        fn remove_file(&mut self, path: impl AsRef<Utf8Path>) -> io::Result<()> {
            self.inner.remove_file(path)
        }

        fn rename(
            &mut self,
            _from: impl AsRef<Utf8Path>,
            _to: impl AsRef<Utf8Path>,
        ) -> io::Result<()> {
            Err(io::Error::other("injected atomic publish failure"))
        }

        fn remove_dir_all(&mut self, path: impl AsRef<Utf8Path>) -> io::Result<()> {
            self.inner.remove_dir_all(path)
        }
    }

    #[test]
    /// When atomic history publication fails, active Work remains available for recovery (WRK-006).
    fn archive_should_preserve_active_work_until_publication_succeeds() {
        let mut fs = FailingPublishFileSystem::default();
        let active = Store::new("/repository");
        let mut work = assert_ok!(Work::new(
            "Preserve interrupted Work".to_owned(),
            "Retain the complete ledger until the archive is visible.".to_owned(),
            RequestSource {
                kind: RequestKind::AdHoc,
                value: "Exercise atomic history publication.".to_owned(),
            },
            "/repository".to_owned(),
            branch("feature"),
            branch("main"),
            object_id(INITIAL_OBJECT_ID),
            object_id(INITIAL_OBJECT_ID),
            "2026-07-13T12:00:00Z".to_owned(),
        ));
        let mut task = Task::new(
            assert_ok!(work.allocate_task_id()),
            "action",
            Workflow::Develop,
            "Create durable history",
            "Publish one complete record.",
            "rapport develop task add",
            TaskStatus::Running,
            "1111",
            "2026-07-13T12:00:00Z",
            None,
        );
        task.finish(
            TaskStatus::Passed,
            "2026-07-13T12:00:30Z".to_owned(),
            "Created the durable record.".to_owned(),
            None,
        );
        assert_ok!(work.finish(
            WorkOutcomeKind::Completed,
            "2026-07-13T12:01:00Z".to_owned(),
            "History is ready.".to_owned(),
            object_id(CHECKPOINT_OBJECT_ID),
            object_id(INITIAL_OBJECT_ID),
        ));
        assert_ok!(active.save_work_and_task(&mut fs, &work, &task));
        let history = assert_ok!(HistoryStore::new(Utf8Path::new("/repository")));

        let error =
            assert_err!(history.archive(&mut fs, &active, &work, std::slice::from_ref(&task)));

        assert!(
            matches!(error, Error::Io { .. }),
            "expecting the injected publication failure to remain an I/O error"
        );
        assert!(
            fs.is_file("/repository/.rapport/work.toml"),
            "expecting active Work to remain after publication fails"
        );
        assert!(
            fs.is_file("/repository/.rapport/tasks/TASK_001.toml"),
            "expecting active Tasks to remain after publication fails"
        );
        assert!(
            assert_ok!(history.records(&fs)).is_empty(),
            "expecting no partial record to become visible"
        );
    }

    #[test]
    /// When archive publication succeeded before local cleanup failed, retry removes only the remaining active files (WRK-006).
    fn archive_should_resume_cleanup_from_the_immutable_record() {
        let mut fs = InMemoryFileSystem::default();
        let active = Store::new("/repository");
        let mut work = assert_ok!(Work::new(
            "Resume history cleanup".to_owned(),
            "Use the published record as the recovery source.".to_owned(),
            RequestSource {
                kind: RequestKind::AdHoc,
                value: "Exercise cleanup recovery.".to_owned(),
            },
            "/repository".to_owned(),
            branch("feature"),
            branch("main"),
            object_id(INITIAL_OBJECT_ID),
            object_id(INITIAL_OBJECT_ID),
            "2026-07-13T12:00:00Z".to_owned(),
        ));
        let mut first = Task::new(
            assert_ok!(work.allocate_task_id()),
            "action",
            Workflow::Develop,
            "First action",
            "Create the first durable Task.",
            "rapport develop task add",
            TaskStatus::Running,
            "1111",
            "2026-07-13T12:00:00Z",
            None,
        );
        first.finish(
            TaskStatus::Passed,
            "2026-07-13T12:00:10Z".to_owned(),
            "Completed the first action.".to_owned(),
            None,
        );
        let mut second = Task::new(
            assert_ok!(work.allocate_task_id()),
            "action",
            Workflow::Develop,
            "Second action",
            "Create the second durable Task.",
            "rapport develop task add",
            TaskStatus::Running,
            "1111",
            "2026-07-13T12:00:10Z",
            None,
        );
        second.finish(
            TaskStatus::Passed,
            "2026-07-13T12:00:20Z".to_owned(),
            "Completed the second action.".to_owned(),
            None,
        );
        assert_ok!(work.finish(
            WorkOutcomeKind::Completed,
            "2026-07-13T12:01:00Z".to_owned(),
            "Published the complete record.".to_owned(),
            object_id(CHECKPOINT_OBJECT_ID),
            object_id(INITIAL_OBJECT_ID),
        ));
        let tasks = vec![first, second];
        assert_ok!(active.save_work_and_tasks(&mut fs, &work, &tasks));
        let history = assert_ok!(HistoryStore::new(Utf8Path::new("/repository")));
        let archive = history.root.join(work.id.to_string());
        assert_ok!(HistoryStore::write_record(&mut fs, &archive, &work, &tasks));
        assert_ok!(fs.remove_file("/repository/.rapport/tasks/TASK_001.toml"));
        let remaining = assert_ok!(active.load_tasks(&fs));

        let resumed = assert_ok!(history.archive(&mut fs, &active, &work, &remaining));

        assert_eq!(
            resumed, archive,
            "expecting cleanup retry to retain the original immutable archive"
        );
        assert!(
            !fs.is_file("/repository/.rapport/work.toml"),
            "expecting cleanup retry to remove finalized active Work"
        );
        assert!(
            !fs.is_file("/repository/.rapport/tasks/TASK_002.toml"),
            "expecting cleanup retry to remove remaining active Tasks"
        );
        assert!(
            fs.is_file(archive.join("tasks/TASK_001.toml"))
                && fs.is_file(archive.join("tasks/TASK_002.toml")),
            "expecting cleanup retry to preserve every archived Task"
        );
    }

    #[test]
    /// When history is listed or selected, newest records lead and ambiguous UUID prefixes are refused (WRK-006).
    fn records_should_sort_newest_first_and_require_a_unique_prefix() {
        let mut fs = InMemoryFileSystem::default();
        let history = assert_ok!(HistoryStore::new(Utf8Path::new("/repository")));
        let mut older = assert_ok!(Work::new(
            "Older Work".to_owned(),
            "The earlier archived result.".to_owned(),
            RequestSource {
                kind: RequestKind::Ticket,
                value: "#105".to_owned(),
            },
            "/first-repository".to_owned(),
            branch("older"),
            branch("main"),
            object_id(INITIAL_OBJECT_ID),
            object_id(INITIAL_OBJECT_ID),
            "2026-07-13T10:00:00Z".to_owned(),
        ));
        older.id = assert_ok!(Uuid::parse_str("019f5300-0000-4000-8000-000000000001"));
        assert_ok!(older.finish(
            WorkOutcomeKind::Completed,
            "2026-07-13T10:30:00Z".to_owned(),
            "Completed earlier.".to_owned(),
            object_id(CHECKPOINT_OBJECT_ID),
            object_id(INITIAL_OBJECT_ID),
        ));
        let mut newer = assert_ok!(Work::new(
            "Newer Work".to_owned(),
            "The later archived result.".to_owned(),
            RequestSource {
                kind: RequestKind::Ticket,
                value: "#106".to_owned(),
            },
            "/second-repository".to_owned(),
            branch("newer"),
            branch("main"),
            object_id(LATER_OBJECT_ID),
            object_id(INITIAL_OBJECT_ID),
            "2026-07-13T11:00:00Z".to_owned(),
        ));
        newer.id = assert_ok!(Uuid::parse_str("019f5300-0000-4000-8000-000000000002"));
        assert_ok!(newer.finish(
            WorkOutcomeKind::Abandoned,
            "2026-07-13T11:30:00Z".to_owned(),
            "Stopped later.".to_owned(),
            object_id(LATER_OBJECT_ID),
            object_id(INITIAL_OBJECT_ID),
        ));
        assert_ok!(HistoryStore::write_record(
            &mut fs,
            &history.root.join(older.id.to_string()),
            &older,
            &[]
        ));
        assert_ok!(HistoryStore::write_record(
            &mut fs,
            &history.root.join(newer.id.to_string()),
            &newer,
            &[]
        ));

        let records = assert_ok!(history.records(&fs));
        let newer_position = assert_some!(
            records
                .iter()
                .position(|record| record.work.title == "Newer Work")
        );
        let older_position = assert_some!(
            records
                .iter()
                .position(|record| record.work.title == "Older Work")
        );
        let error = assert_err!(history.resolve(&fs, "019f53"));

        assert!(
            newer_position < older_position,
            "expecting the newest finalized Work to be listed first"
        );
        assert!(
            matches!(error, Error::AmbiguousHistory(prefix) if prefix == "019f53"),
            "expecting an ambiguous six-character prefix to require more characters"
        );
    }
}
