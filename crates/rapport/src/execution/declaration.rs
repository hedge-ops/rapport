//! Repository-owned options for built-in component and generator adapters.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Declaration {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) features: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) toolchain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) test_config: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) generators: BTreeMap<String, Generator>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Generator {
    pub(crate) inputs: Vec<String>,
    pub(crate) outputs: Vec<String>,
    pub(crate) destination: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) candidates: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) environment: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) package: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) locales: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) format_config: Option<String>,
    #[serde(default)]
    pub(crate) committed: bool,
}
