//! Generated Build-signoff workflows.

use super::Error;
use super::domain::{BuildSignoff, ContextId};
use crate::{CommandRunner, CommandSpec};
use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};

pub(super) fn path(
    repo_root: &Utf8Path,
    context: &ContextId,
    signoff: &BuildSignoff,
) -> Utf8PathBuf {
    let context_slug = context.as_str().to_ascii_lowercase().replace('_', "-");
    let target_slug = signoff.target().to_ascii_lowercase().replace('_', "-");
    repo_root.join(format!(
        ".github/workflows/rapport-{context_slug}-signoff-{target_slug}.yml"
    ))
}

pub(super) fn check_name(context: &ContextId, signoff: &BuildSignoff) -> String {
    let context_name = context
        .as_str()
        .split('_')
        .map(title_case)
        .collect::<Vec<_>>()
        .join(" ");
    format!("Rapport {context_name} Signoff {}", signoff.target())
}

fn title_case(value: &str) -> String {
    let mut characters = value.chars();
    characters.next().map_or_else(String::new, |first| {
        first
            .to_uppercase()
            .chain(characters.flat_map(char::to_lowercase))
            .collect()
    })
}

pub(super) fn render(
    context: &ContextId,
    context_directory: &Utf8Path,
    repo_root: &Utf8Path,
    signoff: &BuildSignoff,
) -> String {
    let relative_directory = context_directory
        .strip_prefix(repo_root)
        .unwrap_or(context_directory);
    let own_path = if relative_directory.as_str().is_empty() {
        "**".to_owned()
    } else {
        format!("{relative_directory}/**")
    };
    let mut paths = vec![own_path];
    paths.extend(signoff.included_paths().iter().cloned());
    paths.sort();
    paths.dedup();
    let path_lines = paths
        .iter()
        .map(|path| format!("      - \"{path}\""))
        .collect::<Vec<_>>()
        .join("\n");
    let workflow_path = path(repo_root, context, signoff);
    let workflow_relative = workflow_path
        .strip_prefix(repo_root)
        .unwrap_or(&workflow_path);
    let working_directory = if relative_directory.as_str().is_empty() {
        "."
    } else {
        relative_directory.as_str()
    };
    let name = check_name(context, signoff);
    format!(
        "name: \"{name}\"\n\n# Rapport owns this file byte-for-byte.\n\non:\n  pull_request:\n    paths:\n{path_lines}\n      - \"{workflow_relative}\"\n\npermissions:\n  contents: read\n\njobs:\n  signoff:\n    name: \"{name}\"\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v6\n      - uses: extractions/setup-just@v3\n      - name: Run {target}\n        working-directory: \"{working_directory}\"\n        run: just {target}\n",
        target = signoff.target()
    )
}

pub(super) fn validate_target(
    runner: &dyn CommandRunner,
    directory: &Utf8Path,
    target: &str,
) -> Result<(), Error> {
    let outcome = runner
        .run(&CommandSpec::new("just", ["--summary"]), directory)
        .map_err(|_| Error::InvalidTarget)?;
    if !outcome.success
        || !outcome
            .stdout
            .split_ascii_whitespace()
            .any(|candidate| candidate == target)
    {
        return Err(Error::InvalidTarget);
    }
    Ok(())
}

pub(super) fn validate_file(
    fs: &impl FileSystem,
    repo_root: &Utf8Path,
    context: &ContextId,
    directory: &Utf8Path,
    signoff: &BuildSignoff,
) -> Result<(), Error> {
    let workflow_path = path(repo_root, context, signoff);
    let actual = fs
        .read_to_string(&workflow_path)
        .map_err(|_| Error::WorkflowDrift(workflow_path.clone()))?;
    let expected = render(context, directory, repo_root, signoff);
    if actual != expected {
        return Err(Error::WorkflowDrift(workflow_path));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::render;
    use crate::policy_context::domain::{BuildSignoff, ContextId};
    use claims::assert_ok;
    use rapport_files::Utf8Path;

    #[test]
    fn checked_in_root_workflow_matches_the_context_contract() {
        let context = assert_ok!(ContextId::parse("ROOT"));
        let signoff = assert_ok!(BuildSignoff::try_new(
            &context,
            "ci".to_owned(),
            0,
            None,
            Vec::new(),
        ));

        let generated = render(
            &context,
            Utf8Path::new("/repo"),
            Utf8Path::new("/repo"),
            &signoff,
        );

        assert_eq!(
            generated,
            include_str!("../../../../.github/workflows/rapport-root-signoff-ci.yml")
        );
    }
}
