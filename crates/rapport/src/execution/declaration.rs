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

/// Shared freshness declarations are independent of the selected adapter.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Generator {
    pub(crate) inputs: NonEmpty<String>,
    pub(crate) outputs: NonEmpty<Output>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) environment: Vec<String>,
    #[serde(default)]
    pub(crate) committed: bool,
    pub(crate) adapter: Adapter,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Output {
    pub(crate) path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) candidate: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Adapter {
    Facet {
        language: FacetLanguage,
        destination: String,
    },
    Boltffi {
        platform: BoltFfiPlatform,
    },
    Command {
        commands: NonEmpty<Command>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FacetLanguage {
    Swift,
    Kotlin,
    Csharp,
}
impl FacetLanguage {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Swift => "swift",
            Self::Kotlin => "kotlin",
            Self::Csharp => "csharp",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BoltFfiPlatform {
    Apple,
    Android,
}
impl BoltFfiPlatform {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Apple => "apple",
            Self::Android => "android",
        }
    }
}

/// Commands run directly, with literal arguments and a repository-relative cwd.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Command {
    pub(crate) program: Program,
    #[serde(default)]
    pub(crate) args: Vec<String>,
    pub(crate) cwd: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) struct Program(String);
impl TryFrom<String> for Program {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.trim().is_empty() {
            return Err("command program must not be empty");
        }
        let file = std::path::Path::new(&value)
            .file_stem()
            .and_then(|name| name.to_str());
        if file.is_some_and(|name| name.eq_ignore_ascii_case("just")) {
            return Err("Rapport commands must invoke underlying tools, not Just");
        }
        Ok(Self(value))
    }
}
impl From<Program> for String {
    fn from(value: Program) -> Self {
        value.0
    }
}
impl Program {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// Construction rejects empty lists before they can reach execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct NonEmpty<T>(Vec<T>);
impl<T> TryFrom<Vec<T>> for NonEmpty<T> {
    type Error = &'static str;
    fn try_from(value: Vec<T>) -> Result<Self, Self::Error> {
        if value.is_empty() {
            Err("list must not be empty")
        } else {
            Ok(Self(value))
        }
    }
}
impl<'de, T: Deserialize<'de>> Deserialize<'de> for NonEmpty<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(Vec::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}
impl<T> std::ops::Deref for NonEmpty<T> {
    type Target = [T];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Adapter {
    pub(crate) fn validate_output(&self, tool: &str, target: &str) -> Result<(), super::Error> {
        let expected = match self {
            Self::Facet { language, .. } => Some(("facet_generate", language.as_str())),
            Self::Boltffi { platform } => Some(("boltffi_generate", platform.as_str())),
            Self::Command { .. } => None,
        };
        if expected.is_some_and(|expected| expected != (tool, target)) {
            return Err(super::Error::UnsupportedGenerator {
                tool: tool.to_owned(),
                target: target.to_owned(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ComponentKind {
    Crate,
    SwiftPackage,
    McpUi,
}
impl ComponentKind {
    pub(crate) fn parse(
        value: Option<&str>,
        path: &rapport_files::Utf8Path,
    ) -> Result<Self, super::Error> {
        match value {
            Some("crate") => Ok(Self::Crate),
            Some("swift_package") => Ok(Self::SwiftPackage),
            Some("mcp_ui") => Ok(Self::McpUi),
            _ => Err(super::Error::UnsupportedComponent {
                component: path.to_path_buf(),
                kind: value.unwrap_or("unspecified").to_owned(),
            }),
        }
    }
}
