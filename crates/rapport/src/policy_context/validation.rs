//! Architecture and benchmark validation.
//!
//! Checks effective component policy without invoking repository workflows.

use super::{Error, repository::Repository};
use rapport_files::{FileSystem, Utf8Path};

pub(super) fn run(
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
    path: &Utf8Path,
) -> Result<String, Error> {
    let repository = Repository::load(fs, repo_root)?;
    let records = repository.descendants(path)?;
    let count = if records.is_empty() {
        super::review_policy(&repository, repo_root, [path])?;
        repository.effective(path)?.len()
    } else {
        for record in &records {
            let relative = record
                .directory()
                .strip_prefix(repo_root)
                .unwrap_or(record.directory());
            super::review_policy(&repository, repo_root, [relative])?;
        }
        records.len()
    };
    Ok(format!(
        "# rapport context validate\n\n- `status` — pass\n- `contexts` — {count}"
    ))
}
