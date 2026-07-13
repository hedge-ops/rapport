//! Context and generated-workflow validation.
//!
//! This module owns repository-wide Context validation and delegates command
//! execution and workflow file checks to their boundary owners.

use super::repository::Repository;
use super::{Error, workflow};
use rapport_files::{FileSystem, Utf8Path};

pub(super) fn run(
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
    path: &Utf8Path,
    runner: &dyn crate::CommandRunner,
) -> Result<String, Error> {
    let repository = Repository::load(fs, repo_root)?;
    repository.validate_included_path_existence(fs)?;
    let records = repository.descendants(path)?;
    let mut signoff_count = 0;
    if records
        .iter()
        .any(|record| !record.context().signoffs().is_empty())
    {
        workflow::validate_shared(fs, repo_root)?;
    }
    for record in &records {
        for signoff in record.context().signoffs() {
            workflow::validate_target(runner, record.directory(), signoff.target())?;
            workflow::validate_file(
                fs,
                repo_root,
                record.context().id(),
                record.directory(),
                signoff,
            )?;
            signoff_count += 1;
        }
    }
    Ok(format!(
        "# rapport context doctor\n\n- `status` — pass\n- `contexts` — {}\n- `signoffs` — {signoff_count}",
        records.len()
    ))
}

pub(crate) fn doctor_all(
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
    runner: &dyn crate::CommandRunner,
) -> Result<(), Error> {
    run(fs, repo_root, Utf8Path::new("."), runner).map(|_| ())
}
