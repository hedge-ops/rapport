//! Repository-owned generator commands.
use crate::execution::{
    Error,
    declaration::{Command, NonEmpty},
    freshness,
};
use rapport_command::CommandSpec;
use rapport_files::Utf8Path;

pub(super) fn commands(
    root: &Utf8Path,
    commands: &NonEmpty<Command>,
) -> Result<Vec<CommandSpec>, Error> {
    commands
        .iter()
        .map(|command| {
            Ok(CommandSpec::new(command.program.as_str())
                .args(command.args.iter().cloned())
                .current_dir(freshness::inside(root, &command.cwd)?))
        })
        .collect()
}
