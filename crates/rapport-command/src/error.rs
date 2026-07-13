//! Invalid command and machine-resource inputs.
//!
//! This module owns typed validation failures returned before process or lock
//! execution begins.

/// A resource key that cannot safely identify a lock file.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid machine resource key: {0:?}")]
pub struct InvalidResourceKey(String);

impl InvalidResourceKey {
    pub(crate) fn new(value: String) -> Self {
        Self(value)
    }
}
