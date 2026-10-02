//! Direct component declarations for the explicit execution boundary.

use super::{Error, domain::Context, repository::Repository};
use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};
use std::collections::BTreeMap;

/// Load validated direct declarations without inheriting parent execution inputs.
pub(crate) fn declarations(
    fs: &mut impl FileSystem,
    root: &Utf8Path,
) -> Result<BTreeMap<Utf8PathBuf, Context>, Error> {
    let repository = Repository::load(fs, root)?;
    Ok(repository
        .records()
        .iter()
        .map(|record| {
            let path = record
                .directory()
                .strip_prefix(root)
                .unwrap_or(record.directory());
            let path = if path.as_str().is_empty() {
                Utf8Path::new(".")
            } else {
                path
            };
            (path.to_path_buf(), record.context().clone())
        })
        .collect())
}
