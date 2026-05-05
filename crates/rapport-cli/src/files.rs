//! Testable filesystem primitives for rapport CLIs.

pub use camino::{Utf8Path, Utf8PathBuf};
use std::collections::{HashMap, HashSet};
use std::io;

pub trait FileSystem {
    fn is_dir(&self, path: impl AsRef<Utf8Path>) -> bool;

    fn is_file(&self, path: impl AsRef<Utf8Path>) -> bool;

    /// Read a UTF-8 file from the filesystem.
    ///
    /// # Errors
    ///
    /// Returns the underlying filesystem error when the path cannot be read.
    fn read_to_string(&self, path: impl AsRef<Utf8Path>) -> io::Result<String>;

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

    fn read_to_string(&self, path: impl AsRef<Utf8Path>) -> io::Result<String> {
        std::fs::read_to_string(path.as_ref())
    }
}

#[derive(Debug, Default, Clone)]
pub struct InMemoryFileSystem {
    directories: HashSet<Utf8PathBuf>,
    files: HashMap<Utf8PathBuf, String>,
}

impl InMemoryFileSystem {
    pub fn add_directory(&mut self, path: impl AsRef<Utf8Path>) {
        self.directories.insert(path.as_ref().to_path_buf());
    }

    pub fn add_file(&mut self, path: impl AsRef<Utf8Path>) {
        self.add_file_with_contents(path, "");
    }

    pub fn add_file_with_contents(
        &mut self,
        path: impl AsRef<Utf8Path>,
        contents: impl Into<String>,
    ) {
        self.files
            .insert(path.as_ref().to_path_buf(), contents.into());
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok};

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

    #[test]
    fn in_memory_file_system_reads_added_files() {
        let mut fs = InMemoryFileSystem::default();
        let path = Utf8PathBuf::from("/work/Package.swift");
        fs.add_file_with_contents(&path, "// swift-tools-version: 6.0\n");

        assert_eq!(
            assert_ok!(fs.read_to_string(&path)),
            "// swift-tools-version: 6.0\n"
        );
    }

    #[test]
    fn in_memory_file_system_reports_missing_files() {
        let fs = InMemoryFileSystem::default();
        let path = Utf8PathBuf::from("/work/Package.swift");

        let err = assert_err!(fs.read_to_string(&path));

        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }
}
