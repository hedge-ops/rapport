//! Generated Build-signoff workflows.
//!
//! This module owns generated GitHub workflow contracts and local validation for Context Build signoffs.

use super::Error;
use super::domain::{BuildSignoff, ContextId};
use crate::{CommandRunner, CommandSpec};
use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};

pub(crate) const SHARED_PATH: &str = ".github/workflows/rapport-signoff.yml";

const SHARED_CONTENTS: &str = r#"name: Rapport signoff request (reusable)

# Rapport owns this file byte-for-byte.
# It requests SHA-bound local proof; it never runs repository build behavior.

on:
  workflow_call:
    inputs:
      identity:
        description: "Stable Rapport signoff identity"
        required: true
        type: string

permissions:
  statuses: write

jobs:
  pending:
    runs-on: ubuntu-latest
    steps:
      - name: Request local Rapport signoff
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          AGGREGATE: Rapport Build
          IDENTITY: ${{ inputs.identity }}
          PR_URL: ${{ github.event.pull_request.html_url }}
          REPO: ${{ github.repository }}
          SHA: ${{ github.event.pull_request.head.sha }}
        run: |
          state() {
            gh api "repos/${REPO}/commits/${SHA}/status" \
              --jq ".statuses | map(select(.context == \"$1\")) | first | .state // \"missing\""
          }
          if [ "$(state "${IDENTITY}")" != success ]; then
            gh api -X POST "repos/${REPO}/statuses/${SHA}" \
              -f "context=${IDENTITY}" \
              -f state=pending \
              -f "description=run Rapport locally and publish proof" \
              -f "target_url=${PR_URL}"
          fi
          if [ "$(state "${AGGREGATE}")" != success ]; then
            gh api -X POST "repos/${REPO}/statuses/${SHA}" \
              -f "context=${AGGREGATE}" \
              -f state=pending \
              -f "description=one or more Rapport Build signoffs need local proof" \
              -f "target_url=${PR_URL}"
          fi
"#;

pub(crate) fn write_shared(fs: &mut impl FileSystem, repo_root: &Utf8Path) -> std::io::Result<()> {
    fs.create_dir_all(repo_root.join(".github/workflows"))?;
    fs.write_string(repo_root.join(SHARED_PATH), SHARED_CONTENTS)
}

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

pub(super) fn shared_path(repo_root: &Utf8Path) -> Utf8PathBuf {
    repo_root.join(SHARED_PATH)
}

pub(super) fn shared_contents() -> &'static str {
    SHARED_CONTENTS
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
    paths.push(SHARED_PATH.to_owned());
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
    let name = check_name(context, signoff);
    format!(
        "name: \"Request {name}\"\n\n# Rapport owns this file byte-for-byte.\n# It requests local proof for `{name}`; it never runs repository build behavior.\n\non:\n  pull_request:\n    paths:\n{path_lines}\n      - \"{workflow_relative}\"\n\npermissions:\n  statuses: write\n\nconcurrency:\n  group: ${{{{ github.workflow }}}}-${{{{ github.event.pull_request.number || github.ref }}}}\n  cancel-in-progress: true\n\njobs:\n  request:\n    if: github.event.pull_request.head.repo.full_name == github.repository\n    uses: ./.github/workflows/rapport-signoff.yml\n    with:\n      identity: \"{name}\"\n    secrets: inherit\n"
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

pub(super) fn validate_shared(fs: &impl FileSystem, repo_root: &Utf8Path) -> Result<(), Error> {
    let path = shared_path(repo_root);
    let actual = fs
        .read_to_string(&path)
        .map_err(|_| Error::WorkflowDrift(path.clone()))?;
    if actual != shared_contents() {
        return Err(Error::WorkflowDrift(path));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{render, shared_contents};
    use crate::policy_context::domain::{BuildSignoff, ContextId};
    use claims::assert_ok;
    use rapport_files::Utf8Path;

    #[test]
    /// When root signoff policy is checked in, its workflow requests the stable Context identity (CTX-002).
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

    #[test]
    /// When a request runs, the shared workflow asks for local proof without executing repository behavior (CTX-002).
    fn checked_in_shared_workflow_requests_local_proof() {
        assert_eq!(
            shared_contents(),
            include_str!("../../../../.github/workflows/rapport-signoff.yml")
        );
    }
}
