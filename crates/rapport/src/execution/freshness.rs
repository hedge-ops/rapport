//! Content identities for declared inputs and generated output integrity.

use super::{Error, declaration::Generator};
use rapport_files::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) struct Receipt {
    pub(crate) basis: String,
    pub(crate) outputs: String,
}

pub(crate) fn inside(root: &Utf8Path, relative: &str) -> Result<Utf8PathBuf, Error> {
    let path = Utf8Path::new(relative);
    if path.is_absolute()
        || path.components().any(|part| part.as_str() == "..")
        || relative.is_empty()
    {
        return Err(Error::Path(path.to_path_buf()));
    }
    let joined = root.join(path);
    let canonical_root = root.canonicalize_utf8().map_err(|source| Error::Io {
        path: root.to_path_buf(),
        source,
    })?;
    let mut existing = joined.as_path();
    while !existing.exists() {
        existing = existing
            .parent()
            .ok_or_else(|| Error::Path(joined.clone()))?;
    }
    let canonical = existing.canonicalize_utf8().map_err(|source| Error::Io {
        path: existing.to_path_buf(),
        source,
    })?;
    if !canonical.starts_with(&canonical_root) {
        return Err(Error::Path(joined));
    }
    Ok(joined)
}

pub(crate) fn digest(parts: impl IntoIterator<Item = Vec<u8>>) -> String {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update(part.len().to_le_bytes());
        hash.update(part);
    }
    format!("{:x}", hash.finalize())
}

pub(crate) fn basis(root: &Utf8Path, config: &Generator, identity: &[u8]) -> Result<String, Error> {
    let files = patterns(root, &config.inputs)?;
    if files.is_empty() {
        return Err(Error::Declaration(
            "generation inputs matched no files".to_owned(),
        ));
    }
    Ok(digest([
        identity.to_vec(),
        serde_json::to_vec(config)?,
        files_digest(root, &files)?.into_bytes(),
    ]))
}

pub(crate) fn outputs(root: &Utf8Path, config: &Generator) -> Result<String, Error> {
    let mut files = BTreeSet::new();
    for output in config.outputs.iter() {
        let output = &output.path;
        let path = inside(root, output)?;
        if !path.exists() {
            return Err(Error::MissingOutput(path));
        }
        if path.is_dir() {
            let entries = patterns(root, &[format!("{output}/**/*")])?;
            if entries.is_empty() {
                return Err(Error::MissingOutput(path));
            }
            files.extend(entries);
        } else {
            files.insert(path);
        }
    }
    files_digest(root, &files)
}

fn patterns(root: &Utf8Path, patterns: &[String]) -> Result<BTreeSet<Utf8PathBuf>, Error> {
    let mut paths = BTreeSet::new();
    for pattern in patterns {
        let path = inside(root, pattern)?;
        for entry in glob::glob(path.as_str())? {
            let entry = entry?;
            let entry = Utf8PathBuf::from_path_buf(entry)
                .map_err(|_| Error::Declaration("non-UTF-8 generation path".to_owned()))?;
            if entry.is_file() {
                let relative = entry
                    .strip_prefix(root)
                    .map_err(|_| Error::Path(entry.clone()))?;
                paths.insert(inside(root, relative.as_str())?);
            }
        }
    }
    Ok(paths)
}

fn files_digest(root: &Utf8Path, files: &BTreeSet<Utf8PathBuf>) -> Result<String, Error> {
    let mut parts = Vec::new();
    for file in files {
        let relative = file
            .strip_prefix(root)
            .map_err(|_| Error::Path(file.clone()))?;
        parts.push(relative.as_str().as_bytes().to_vec());
        parts.push(std::fs::read(file).map_err(|source| Error::Io {
            path: file.clone(),
            source,
        })?);
    }
    Ok(digest(parts))
}
