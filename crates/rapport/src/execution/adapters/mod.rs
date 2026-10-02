//! Dispatch typed adapters without repository-specific generator conventions.
mod boltffi;
mod command;
mod component;
mod facet;
use super::{Error, declaration::Adapter};
pub(crate) use component::component;
use rapport_command::CommandSpec;
use rapport_files::Utf8Path;

pub(crate) fn generation(
    root: &Utf8Path,
    path: &Utf8Path,
    adapter: &Adapter,
) -> Result<Vec<CommandSpec>, Error> {
    match adapter {
        Adapter::Facet {
            language,
            destination,
        } => facet::commands(root, path, *language, destination),
        Adapter::Boltffi { platform } => boltffi::commands(root, path, *platform),
        Adapter::Command { commands } => command::commands(root, commands),
    }
}

fn require_host(required: &'static str) -> Result<(), Error> {
    let actual = std::env::consts::OS;
    if actual == required {
        Ok(())
    } else {
        Err(Error::Platform { required, actual })
    }
}
