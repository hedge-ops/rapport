//! `BoltFFI` packaging conventions.
use crate::execution::{Error, declaration::BoltFfiPlatform};
use rapport_command::CommandSpec;
use rapport_files::Utf8Path;

pub(super) fn commands(
    root: &Utf8Path,
    path: &Utf8Path,
    platform: BoltFfiPlatform,
) -> Result<Vec<CommandSpec>, Error> {
    match platform {
        BoltFfiPlatform::Apple => super::require_host("macos")?,
        BoltFfiPlatform::Android => {}
    }
    Ok(vec![
        CommandSpec::new("boltffi")
            .args(["pack", platform.as_str()])
            .current_dir(root.join(path)),
    ])
}
