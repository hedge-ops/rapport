use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::io;

const LOCK_VERSION: u16 = 1;
const RUST_VERSION: &str = "1.0.0";
const CRUX_VERSION: &str = "1.0.0";

const RUST_FILES: &[(&str, &str)] = &[
    (
        "rust/coding.toml",
        include_str!("../catalog/rust/coding.toml"),
    ),
    ("rust/test.toml", include_str!("../catalog/rust/test.toml")),
    (
        "rust/comment.toml",
        include_str!("../catalog/rust/comment.toml"),
    ),
    (
        "rust/crate.toml",
        include_str!("../catalog/rust/crate.toml"),
    ),
];

const CRUX_FILES: &[(&str, &str)] = &[
    (
        "crux/effects.toml",
        include_str!("../catalog/crux/effects.toml"),
    ),
    (
        "crux/model.toml",
        include_str!("../catalog/crux/model.toml"),
    ),
    ("crux/view.toml", include_str!("../catalog/crux/view.toml")),
    ("crux/test.toml", include_str!("../catalog/crux/test.toml")),
    ("crux/app.toml", include_str!("../catalog/crux/app.toml")),
];

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RulesLock {
    version: u16,
    #[serde(default)]
    packs: Vec<LockedPack>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LockedPack {
    name: String,
    version: String,
    digest: String,
}

pub(crate) fn catalog() -> String {
    format!(
        "# rapport rules catalog\n\n## Built-in packs\n\n- `rust` {RUST_VERSION} -- Michael's opinionated approach to developing, testing, and documenting Rust crates.\n- `crux` {CRUX_VERSION} -- Michael's opinionated approach to architecting and testing Crux applications; depends on `rust` {RUST_VERSION}."
    )
}

pub(crate) fn install(
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
    pack: &str,
) -> Result<String, BuiltinRulesError> {
    match pack {
        "rust" => install_one(fs, repo_root, "rust", RUST_VERSION, RUST_FILES)?,
        "crux" => {
            install_one(fs, repo_root, "rust", RUST_VERSION, RUST_FILES)?;
            install_one(fs, repo_root, "crux", CRUX_VERSION, CRUX_FILES)?;
        }
        _ => return Err(BuiltinRulesError::UnknownPack(pack.to_string())),
    }
    Ok(format!(
        "# rapport rules add\n\n- `pack` -- {pack}\n- `status` -- installed"
    ))
}

pub(crate) fn validate_installation(
    fs: &impl FileSystem,
    repo_root: &Utf8Path,
) -> Result<usize, BuiltinRulesError> {
    let lock = load_lock(fs, &repo_root.join(".rapport/rules.lock"))?;
    for pack in &lock.packs {
        let (version, files) = match pack.name.as_str() {
            "rust" => (RUST_VERSION, RUST_FILES),
            "crux" => (CRUX_VERSION, CRUX_FILES),
            _ => return Err(BuiltinRulesError::UnknownPack(pack.name.clone())),
        };
        if pack.version != version || pack.digest != digest(files)? {
            return Err(BuiltinRulesError::LockedConflict(pack.name.clone()));
        }
        verify_files(fs, repo_root, files)?;
    }
    Ok(lock.packs.len())
}

fn install_one(
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
    name: &str,
    version: &str,
    files: &[(&str, &str)],
) -> Result<(), BuiltinRulesError> {
    let lock_path = repo_root.join(".rapport/rules.lock");
    let mut lock = load_lock(fs, &lock_path)?;
    let digest = digest(files)?;
    if let Some(existing) = lock.packs.iter().find(|pack| pack.name == name) {
        if existing.version != version || existing.digest != digest {
            return Err(BuiltinRulesError::LockedConflict(name.to_string()));
        }
        verify_files(fs, repo_root, files)?;
        return Ok(());
    }
    for (relative, _) in files {
        let path = repo_root.join(".rapport/rules").join(relative);
        if fs.exists(&path) {
            return Err(BuiltinRulesError::FileConflict(path));
        }
    }
    for (relative, contents) in files {
        let path = repo_root.join(".rapport/rules").join(relative);
        if let Some(parent) = path.parent() {
            fs.create_dir_all(parent)
                .map_err(|source| BuiltinRulesError::Io {
                    path: parent.to_path_buf(),
                    source,
                })?;
        }
        fs.write_string(&path, canonical_contents(relative, contents)?)
            .map_err(|source| BuiltinRulesError::Io {
                path: path.clone(),
                source,
            })?;
    }
    lock.packs.push(LockedPack {
        name: name.to_string(),
        version: version.to_string(),
        digest,
    });
    lock.packs.sort_by(|left, right| left.name.cmp(&right.name));
    let rendered = toml_edit::ser::to_string_pretty(&lock).map_err(BuiltinRulesError::Encode)?;
    fs.write_string(&lock_path, rendered)
        .map_err(|source| BuiltinRulesError::Io {
            path: lock_path,
            source,
        })
}

fn verify_files(
    fs: &impl FileSystem,
    repo_root: &Utf8Path,
    files: &[(&str, &str)],
) -> Result<(), BuiltinRulesError> {
    for (relative, expected) in files {
        let path = repo_root.join(".rapport/rules").join(relative);
        let actual = fs
            .read_to_string(&path)
            .map_err(|source| BuiltinRulesError::Io {
                path: path.clone(),
                source,
            })?;
        if canonical_contents(relative, &actual)? != canonical_contents(relative, expected)? {
            return Err(BuiltinRulesError::Modified(path));
        }
    }
    Ok(())
}

fn load_lock(fs: &impl FileSystem, path: &Utf8Path) -> Result<RulesLock, BuiltinRulesError> {
    if !fs.is_file(path) {
        return Ok(RulesLock {
            version: LOCK_VERSION,
            packs: Vec::new(),
        });
    }
    let contents = fs
        .read_to_string(path)
        .map_err(|source| BuiltinRulesError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    let lock: RulesLock =
        toml::from_str(&contents).map_err(|source| BuiltinRulesError::Decode {
            path: path.to_path_buf(),
            source,
        })?;
    if lock.version != LOCK_VERSION {
        return Err(BuiltinRulesError::LockVersion(lock.version));
    }
    Ok(lock)
}

fn digest(files: &[(&str, &str)]) -> Result<String, BuiltinRulesError> {
    let mut hasher = Sha256::new();
    for (path, contents) in files {
        let canonical = canonical_contents(path, contents)?;
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(canonical.as_bytes());
        hasher.update([0]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn canonical_contents(path: &str, contents: &str) -> Result<String, BuiltinRulesError> {
    let document: crate::ruleset::RulesetDocument =
        toml::from_str(contents).map_err(|source| BuiltinRulesError::Decode {
            path: Utf8PathBuf::from(path),
            source,
        })?;
    crate::ruleset::render(&document)
        .map_err(|source| BuiltinRulesError::Catalog(source.to_string()))
}

#[derive(Debug)]
pub(crate) enum BuiltinRulesError {
    UnknownPack(String),
    LockedConflict(String),
    FileConflict(Utf8PathBuf),
    Modified(Utf8PathBuf),
    LockVersion(u16),
    Io {
        path: Utf8PathBuf,
        source: io::Error,
    },
    Decode {
        path: Utf8PathBuf,
        source: toml::de::Error,
    },
    Encode(toml_edit::ser::Error),
    Catalog(String),
}

impl fmt::Display for BuiltinRulesError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownPack(pack) => {
                write!(formatter, "built-in rules pack `{pack}` was not found")
            }
            Self::LockedConflict(pack) => write!(
                formatter,
                "installed rules pack `{pack}` does not match the built-in version"
            ),
            Self::FileConflict(path) => write!(
                formatter,
                "rules pack file `{path}` already exists without a matching lock"
            ),
            Self::Modified(path) => write!(
                formatter,
                "installed rules pack file `{path}` has local modifications"
            ),
            Self::LockVersion(version) => {
                write!(formatter, "unsupported rules lock version `{version}`")
            }
            Self::Io { path, source } => write!(
                formatter,
                "rules pack filesystem error at `{path}`: {source}"
            ),
            Self::Decode { path, source } => {
                write!(formatter, "rules lock parse error at `{path}`: {source}")
            }
            Self::Encode(source) => write!(formatter, "could not encode rules lock: {source}"),
            Self::Catalog(source) => {
                write!(formatter, "could not canonicalize built-in rules: {source}")
            }
        }
    }
}

impl std::error::Error for BuiltinRulesError {}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::ruleset::Catalog;
    use rapport_files::InMemoryFileSystem;

    #[test]
    fn rust_install_is_locked_idempotent_and_resolvable() {
        let mut fs = InMemoryFileSystem::default();
        fs.write_string("/repo/.gitignore", "").unwrap();

        install(&mut fs, Utf8Path::new("/repo"), "rust").unwrap();
        install(&mut fs, Utf8Path::new("/repo"), "rust").unwrap();

        let lock = fs.read_to_string("/repo/.rapport/rules.lock").unwrap();
        let catalog = Catalog::discover_repository(&fs, Utf8Path::new("/repo")).unwrap();
        assert!(lock.contains("name = \"rust\""));
        assert_eq!(catalog.resolve("RUST_CRATE").unwrap().len(), 64);
    }

    #[test]
    fn crux_install_locks_compatible_rust_dependency() {
        let mut fs = InMemoryFileSystem::default();
        fs.write_string("/repo/.gitignore", "").unwrap();

        install(&mut fs, Utf8Path::new("/repo"), "crux").unwrap();

        let lock = fs.read_to_string("/repo/.rapport/rules.lock").unwrap();
        let catalog = Catalog::discover_repository(&fs, Utf8Path::new("/repo")).unwrap();
        assert!(lock.contains("name = \"rust\""));
        assert!(lock.contains("name = \"crux\""));
        assert_eq!(catalog.resolve("CRUX_APP").unwrap().len(), 128);
    }

    #[test]
    fn reinstall_rejects_locally_modified_pack_file() {
        let mut fs = InMemoryFileSystem::default();
        fs.write_string("/repo/.gitignore", "").unwrap();
        install(&mut fs, Utf8Path::new("/repo"), "rust").unwrap();
        let path = "/repo/.rapport/rules/rust/coding.toml";
        let modified = fs
            .read_to_string(path)
            .unwrap()
            .replace("Use rustfmt for formatting.", "Use custom formatting.");
        fs.write_string(path, &modified).unwrap();

        let error = install(&mut fs, Utf8Path::new("/repo"), "rust").unwrap_err();

        assert!(error.to_string().contains("local modifications"));
    }
}
