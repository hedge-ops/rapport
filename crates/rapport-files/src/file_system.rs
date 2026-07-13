//! Filesystem behavior required by Rapport workflows.
//!
//! The trait defines portable defaults while implementations own storage and
//! operating-system details.

use camino::{Utf8Path, Utf8PathBuf};
use std::io;

pub trait FileSystem {
    fn is_dir(&self, path: impl AsRef<Utf8Path>) -> bool;

    fn is_file(&self, path: impl AsRef<Utf8Path>) -> bool;

    /// Resolve symlinks and return an absolute canonical path.
    ///
    /// # Errors
    ///
    /// Returns an error when the path does not exist or cannot be canonicalized.
    fn canonicalize(&self, path: impl AsRef<Utf8Path>) -> io::Result<Utf8PathBuf> {
        let path = path.as_ref();
        if self.exists(path) {
            Ok(path.to_path_buf())
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{path} not found"),
            ))
        }
    }

    /// Return the Git file mode for a regular working-tree file.
    ///
    /// In-memory and non-Unix implementations default to a non-executable
    /// regular file. Production Unix filesystems preserve the executable bit.
    ///
    /// # Errors
    ///
    /// Returns [`io::ErrorKind::NotFound`] when the file does not exist.
    fn git_file_mode(&self, path: impl AsRef<Utf8Path>) -> io::Result<u32> {
        let path = path.as_ref();
        if self.is_file(path) {
            Ok(0o100_644)
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{path} not found"),
            ))
        }
    }

    /// Read a UTF-8 file from the filesystem.
    ///
    /// # Errors
    ///
    /// Returns the underlying filesystem error when the path cannot be read.
    fn read_to_string(&self, path: impl AsRef<Utf8Path>) -> io::Result<String>;

    /// Read arbitrary file bytes from the filesystem.
    ///
    /// # Errors
    ///
    /// Returns the underlying filesystem error when the path cannot be read.
    fn read_bytes(&self, path: impl AsRef<Utf8Path>) -> io::Result<Vec<u8>> {
        self.read_to_string(path).map(String::into_bytes)
    }

    /// Read the immediate children of a directory.
    ///
    /// # Errors
    ///
    /// Returns the underlying filesystem error when the directory cannot be read.
    fn read_dir(&self, path: impl AsRef<Utf8Path>) -> io::Result<Vec<Utf8PathBuf>>;

    /// Create a directory and all missing parent directories.
    ///
    /// # Errors
    ///
    /// Returns the underlying filesystem error when the directory cannot be created.
    fn create_dir_all(&mut self, path: impl AsRef<Utf8Path>) -> io::Result<()>;

    /// Write a UTF-8 file, replacing any existing contents.
    ///
    /// # Errors
    ///
    /// Returns the underlying filesystem error when the path cannot be written.
    fn write_string(
        &mut self,
        path: impl AsRef<Utf8Path>,
        contents: impl AsRef<str>,
    ) -> io::Result<()>;

    /// Append one UTF-8 line to a file, creating it when it does not exist.
    ///
    /// # Errors
    ///
    /// Returns the underlying filesystem error when the path cannot be appended.
    fn append_line(&mut self, path: impl AsRef<Utf8Path>, line: impl AsRef<str>) -> io::Result<()>;

    /// Remove a file from the filesystem.
    ///
    /// # Errors
    ///
    /// Returns the underlying filesystem error when the file cannot be removed.
    fn remove_file(&mut self, path: impl AsRef<Utf8Path>) -> io::Result<()>;

    /// Atomically rename a file or directory within one filesystem.
    ///
    /// # Errors
    ///
    /// Returns the underlying filesystem error when the source cannot be
    /// renamed or the destination already exists.
    fn rename(&mut self, from: impl AsRef<Utf8Path>, to: impl AsRef<Utf8Path>) -> io::Result<()>;

    /// Remove a directory and every descendant.
    ///
    /// # Errors
    ///
    /// Returns the underlying filesystem error when the directory cannot be
    /// removed.
    fn remove_dir_all(&mut self, path: impl AsRef<Utf8Path>) -> io::Result<()>;

    fn exists(&self, path: impl AsRef<Utf8Path>) -> bool {
        self.is_dir(path.as_ref()) || self.is_file(path)
    }
}
