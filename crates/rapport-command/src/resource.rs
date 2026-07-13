//! Exclusive machine-local resource coordination.
//!
//! This module validates resource identities and maps them to advisory lock
//! files held for the lifetime of a guard.

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io;
use std::path::PathBuf;

/// A validated name for an exclusive machine-local resource.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResourceKey(String);

impl ResourceKey {
    /// Create a resource key safe for use as a lock filename.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidResourceKey`] when the key is empty or contains
    /// anything other than ASCII letters, digits, `.`, `_`, or `-`.
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidResourceKey> {
        let value = value.into();
        if value.is_empty()
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        {
            return Err(InvalidResourceKey(value));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A resource key that cannot safely identify a lock file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidResourceKey(String);

impl fmt::Display for InvalidResourceKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid machine resource key: {:?}", self.0)
    }
}

impl std::error::Error for InvalidResourceKey {}

/// Coordinates named exclusive resources across processes on one machine.
#[derive(Debug, Clone)]
pub struct MachineResources {
    lock_directory: PathBuf,
}

impl MachineResources {
    #[must_use]
    pub fn new(lock_directory: impl Into<PathBuf>) -> Self {
        Self {
            lock_directory: lock_directory.into(),
        }
    }

    /// Use Rapport's stable lock directory beneath the current user's temporary
    /// directory.
    #[must_use]
    pub fn rapport_default() -> Self {
        Self::new(std::env::temp_dir().join("rapport").join("resources"))
    }

    /// Wait until the named resource is available and hold it until the returned
    /// guard is dropped.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the lock directory or lock file cannot be
    /// created, or when the operating system cannot acquire the file lock.
    pub fn acquire(&self, key: &ResourceKey) -> io::Result<ResourceGuard> {
        std::fs::create_dir_all(&self.lock_directory)?;
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(self.lock_directory.join(format!("{}.lock", key.as_str())))?;
        File::lock(&file)?;
        Ok(ResourceGuard { file })
    }
}

/// Holds an exclusive machine resource until dropped.
#[derive(Debug)]
pub struct ResourceGuard {
    file: File,
}

impl Drop for ResourceGuard {
    fn drop(&mut self) {
        let _ = File::unlock(&self.file);
    }
}
