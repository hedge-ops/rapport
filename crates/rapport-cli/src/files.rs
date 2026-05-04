//! Testable filesystem primitives for rapport CLIs.

use camino::{Utf8Path, Utf8PathBuf};
use std::collections::HashSet;

pub trait FileSystem {
    fn is_dir(&self, path: impl AsRef<Utf8Path>) -> bool;

    fn is_file(&self, path: impl AsRef<Utf8Path>) -> bool;

    fn exists(&self, path: impl AsRef<Utf8Path>) -> bool {
        self.is_dir(path.as_ref()) || self.is_file(path)
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct RealFileSystem;

impl FileSystem for RealFileSystem {
    fn is_dir(&self, path: impl AsRef<Utf8Path>) -> bool {
        path.as_ref().is_dir()
    }

    fn is_file(&self, path: impl AsRef<Utf8Path>) -> bool {
        path.as_ref().is_file()
    }
}

#[derive(Debug, Default, Clone)]
pub struct InMemoryFileSystem {
    directories: HashSet<Utf8PathBuf>,
    files: HashSet<Utf8PathBuf>,
}

impl InMemoryFileSystem {
    pub fn add_directory(&mut self, path: impl AsRef<Utf8Path>) {
        self.directories.insert(path.as_ref().to_path_buf());
    }

    pub fn add_file(&mut self, path: impl AsRef<Utf8Path>) {
        self.files.insert(path.as_ref().to_path_buf());
    }
}

impl FileSystem for InMemoryFileSystem {
    fn is_dir(&self, path: impl AsRef<Utf8Path>) -> bool {
        self.directories.contains(path.as_ref())
    }

    fn is_file(&self, path: impl AsRef<Utf8Path>) -> bool {
        self.files.contains(path.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_memory_file_system_recognizes_added_directory() {
        let mut fs = InMemoryFileSystem::default();
        fs.add_directory("/work");

        assert!(fs.is_dir("/work"));
    }

    #[test]
    fn in_memory_file_system_does_not_treat_directory_as_file() {
        let mut fs = InMemoryFileSystem::default();
        fs.add_directory("/work");

        assert!(!fs.is_file("/work"));
    }

    #[test]
    fn in_memory_file_system_recognizes_added_file() {
        let mut fs = InMemoryFileSystem::default();
        fs.add_file("/work/Cargo.toml");

        assert!(fs.is_file("/work/Cargo.toml"));
    }

    #[test]
    fn in_memory_file_system_does_not_treat_file_as_directory() {
        let mut fs = InMemoryFileSystem::default();
        fs.add_file("/work/Cargo.toml");

        assert!(!fs.is_dir("/work/Cargo.toml"));
    }

    #[test]
    fn in_memory_file_system_exists_recognizes_added_file() {
        let mut fs = InMemoryFileSystem::default();
        fs.add_file("/work/Cargo.toml");

        assert!(fs.exists("/work/Cargo.toml"));
    }
}
