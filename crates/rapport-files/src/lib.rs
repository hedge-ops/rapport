//! Testable filesystem primitives for Rapport workflow code.
//!
//! The crate exposes one filesystem contract with production and in-memory
//! implementations owned by focused modules.

mod file_system;
mod memory;
mod real;

pub use camino::{Utf8Path, Utf8PathBuf};
pub use file_system::FileSystem;
pub use memory::InMemoryFileSystem;
pub use real::RealFileSystem;

#[cfg(test)]
mod tests;
