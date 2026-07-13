//! Deterministic in-memory filesystem for tests and simulations.
//!
//! This module owns the virtual directory and file maps while conforming to
//! the same contract as the real filesystem.

use crate::{FileSystem, Utf8Path, Utf8PathBuf};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::io;

#[derive(Debug, Default, Clone)]
pub struct InMemoryFileSystem {
    directories: HashSet<Utf8PathBuf>,
    files: HashMap<Utf8PathBuf, String>,
}

impl InMemoryFileSystem {
    pub fn add_directory(&mut self, path: impl AsRef<Utf8Path>) {
        let path = path.as_ref();
        self.add_parent_directories(path);
        self.directories.insert(path.to_path_buf());
    }

    pub fn add_file(&mut self, path: impl AsRef<Utf8Path>) {
        self.add_file_with_contents(path, "");
    }

    pub fn add_file_with_contents(
        &mut self,
        path: impl AsRef<Utf8Path>,
        contents: impl Into<String>,
    ) {
        if let Some(parent) = path.as_ref().parent() {
            self.add_directory(parent);
        }
        self.files
            .insert(path.as_ref().to_path_buf(), contents.into());
    }

    fn add_parent_directories(&mut self, path: &Utf8Path) {
        let mut current = path.parent();
        while let Some(parent) = current {
            if parent.as_str().is_empty() {
                break;
            }
            self.directories.insert(parent.to_path_buf());
            current = parent.parent();
        }
    }
}

impl FileSystem for InMemoryFileSystem {
    fn is_dir(&self, path: impl AsRef<Utf8Path>) -> bool {
        self.directories.contains(path.as_ref())
    }

    fn is_file(&self, path: impl AsRef<Utf8Path>) -> bool {
        self.files.contains_key(path.as_ref())
    }

    fn read_to_string(&self, path: impl AsRef<Utf8Path>) -> io::Result<String> {
        self.files.get(path.as_ref()).cloned().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("{} not found", path.as_ref()),
            )
        })
    }

    fn read_dir(&self, path: impl AsRef<Utf8Path>) -> io::Result<Vec<Utf8PathBuf>> {
        let path = path.as_ref();
        if !self.is_dir(path) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{path} not found"),
            ));
        }

        let mut entries = BTreeSet::new();
        for directory in &self.directories {
            if directory.parent() == Some(path) {
                entries.insert(directory.clone());
            }
        }
        for file in self.files.keys() {
            if file.parent() == Some(path) {
                entries.insert(file.clone());
            }
        }
        Ok(entries.into_iter().collect())
    }

    fn create_dir_all(&mut self, path: impl AsRef<Utf8Path>) -> io::Result<()> {
        self.add_directory(path);
        Ok(())
    }

    fn write_string(
        &mut self,
        path: impl AsRef<Utf8Path>,
        contents: impl AsRef<str>,
    ) -> io::Result<()> {
        self.add_file_with_contents(path, contents.as_ref());
        Ok(())
    }

    fn append_line(&mut self, path: impl AsRef<Utf8Path>, line: impl AsRef<str>) -> io::Result<()> {
        if let Some(parent) = path.as_ref().parent() {
            self.add_directory(parent);
        }
        let entry = self.files.entry(path.as_ref().to_path_buf()).or_default();
        entry.push_str(line.as_ref());
        entry.push('\n');
        Ok(())
    }

    fn remove_file(&mut self, path: impl AsRef<Utf8Path>) -> io::Result<()> {
        self.files.remove(path.as_ref()).map_or_else(
            || {
                Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("{} not found", path.as_ref()),
                ))
            },
            |_| Ok(()),
        )
    }

    fn rename(&mut self, from: impl AsRef<Utf8Path>, to: impl AsRef<Utf8Path>) -> io::Result<()> {
        let from = from.as_ref().to_path_buf();
        let to = to.as_ref().to_path_buf();
        if self.exists(&to) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{to} already exists"),
            ));
        }
        if let Some(contents) = self.files.remove(&from) {
            if let Some(parent) = to.parent() {
                self.add_directory(parent);
            }
            self.files.insert(to, contents);
            return Ok(());
        }
        if !self.directories.contains(&from) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{from} not found"),
            ));
        }

        let directories = self
            .directories
            .iter()
            .filter(|path| **path == from || path.starts_with(&from))
            .cloned()
            .collect::<Vec<_>>();
        let files = self
            .files
            .iter()
            .filter(|(path, _)| path.starts_with(&from))
            .map(|(path, contents)| (path.clone(), contents.clone()))
            .collect::<Vec<_>>();
        self.directories
            .retain(|path| *path != from && !path.starts_with(&from));
        self.files.retain(|path, _| !path.starts_with(&from));
        if let Some(parent) = to.parent() {
            self.add_directory(parent);
        }
        for path in directories {
            let relative = path.strip_prefix(&from).map_err(io::Error::other)?;
            self.directories.insert(to.join(relative));
        }
        for (path, contents) in files {
            let relative = path.strip_prefix(&from).map_err(io::Error::other)?;
            self.files.insert(to.join(relative), contents);
        }
        Ok(())
    }

    fn remove_dir_all(&mut self, path: impl AsRef<Utf8Path>) -> io::Result<()> {
        let path = path.as_ref();
        if !self.directories.contains(path) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{path} not found"),
            ));
        }
        self.files.retain(|file, _| !file.starts_with(path));
        self.directories
            .retain(|directory| directory != path && !directory.starts_with(path));
        Ok(())
    }
}
