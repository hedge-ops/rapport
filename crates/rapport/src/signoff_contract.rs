use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io;

pub(crate) const SHARED_WORKFLOW: &str = ".github/workflows/rapport-signoff.yml";
const WORKFLOW_DIRECTORY: &str = ".github/workflows";
const REQUEST_PREFIX: &str = "rapport-";

const SHARED_WORKFLOW_CONTENTS: &str = r#"name: Rapport signoff (reusable)

# Rapport owns this file byte-for-byte. Run `rapport context signoff repair` to restore it.
# It asks for SHA-bound local proof by posting a pending `signoff: <target>` status.

on:
  workflow_call:
    inputs:
      target:
        description: "Folder-qualified signoff target"
        required: true
        type: string

permissions:
  statuses: write

jobs:
  pending:
    runs-on: ubuntu-latest
    steps:
      - name: Request local signoff
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          REPO: ${{ github.repository }}
          SHA: ${{ github.event.pull_request.head.sha }}
          PR_URL: ${{ github.event.pull_request.html_url }}
          TARGET: ${{ inputs.target }}
        run: |
          gh api -X POST "repos/${REPO}/statuses/${SHA}" \
            -f "context=signoff: ${TARGET}" \
            -f state=pending \
            -f "description=awaiting local signoff" \
            -f "target_url=${PR_URL}"
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SignoffRequest {
    folder: String,
    target: String,
    qualified_target: String,
    workflow_path: Utf8PathBuf,
}

impl SignoffRequest {
    pub(crate) fn new(
        repo_root: &Utf8Path,
        context_directory: &Utf8Path,
        target: &str,
    ) -> Result<Self, SignoffContractError> {
        let target = target.trim();
        if !valid_target(target) {
            return Err(SignoffContractError::InvalidTarget {
                target: target.to_string(),
            });
        }
        let relative = context_directory.strip_prefix(repo_root).map_err(|_| {
            SignoffContractError::OutsideRepository {
                path: context_directory.to_path_buf(),
            }
        })?;
        let folder = if relative.as_str().is_empty() {
            String::from(".")
        } else {
            relative.as_str().replace('\\', "/")
        };
        let folder_slug = if folder == "." {
            String::from("root")
        } else {
            slug(&folder)
        };
        let qualified_target = format!("{folder_slug}-{target}");
        let workflow_path = repo_root
            .join(WORKFLOW_DIRECTORY)
            .join(format!("{REQUEST_PREFIX}{qualified_target}.yml"));
        Ok(Self {
            folder,
            target: target.to_string(),
            qualified_target,
            workflow_path,
        })
    }

    pub(crate) fn target(&self) -> &str {
        &self.target
    }

    pub(crate) fn qualified_target(&self) -> &str {
        &self.qualified_target
    }

    pub(crate) fn workflow_path(&self) -> &Utf8Path {
        &self.workflow_path
    }

    pub(crate) fn render(&self, repo_root: &Utf8Path) -> String {
        let workflow = self
            .workflow_path
            .strip_prefix(repo_root)
            .unwrap_or(&self.workflow_path)
            .as_str()
            .replace('\\', "/");
        let paths = if self.folder == "." {
            String::from("      - \"**\"\n")
        } else {
            format!("      - \"{}/**\"\n", self.folder)
        };
        format!(
            "name: \"Rapport signoff: {qualified}\"\n\n# Rapport owns this file byte-for-byte. Run `rapport context signoff repair {folder} {target}` to restore it.\n\non:\n  pull_request:\n    branches:\n      - \"*\"\n    paths:\n{paths}      - \"{workflow}\"\n\nconcurrency:\n  group: ${{{{ github.workflow }}}}-${{{{ github.event.pull_request.number || github.ref }}}}\n  cancel-in-progress: true\n\njobs:\n  signoff:\n    uses: ./.github/workflows/rapport-signoff.yml\n    with:\n      target: {qualified}\n    secrets: inherit\n",
            qualified = self.qualified_target,
            folder = self.folder,
            target = self.target,
        )
    }
}

pub(crate) fn validate(
    fs: &impl FileSystem,
    repo_root: &Utf8Path,
    requests: &[SignoffRequest],
) -> Vec<String> {
    let mut problems = Vec::new();
    let shared_path = repo_root.join(SHARED_WORKFLOW);
    if !requests.is_empty() || fs.is_file(&shared_path) {
        validate_exact_file(
            fs,
            &shared_path,
            SHARED_WORKFLOW_CONTENTS,
            repo_root,
            &mut problems,
        );
    }

    let mut expected_paths = BTreeSet::new();
    let mut qualified_targets = BTreeMap::new();
    for request in requests {
        if let Some(first) = qualified_targets.insert(
            request.qualified_target().to_string(),
            request.workflow_path().to_path_buf(),
        ) {
            problems.push(format!(
                "signoff target collision `{}` between `{}` and `{}`",
                request.qualified_target(),
                display_path(repo_root, &first),
                display_path(repo_root, request.workflow_path())
            ));
        }
        expected_paths.insert(request.workflow_path().to_path_buf());
        validate_exact_file(
            fs,
            request.workflow_path(),
            &request.render(repo_root),
            repo_root,
            &mut problems,
        );
    }

    let workflows = repo_root.join(WORKFLOW_DIRECTORY);
    if fs.is_dir(&workflows) {
        match fs.read_dir(&workflows) {
            Ok(entries) => {
                for entry in entries {
                    let Some(name) = entry.file_name() else {
                        continue;
                    };
                    if name.starts_with(REQUEST_PREFIX)
                        && entry.extension().is_some_and(|extension| {
                            extension.eq_ignore_ascii_case("yml")
                                || extension.eq_ignore_ascii_case("yaml")
                        })
                        && entry != repo_root.join(SHARED_WORKFLOW)
                        && !expected_paths.contains(&entry)
                    {
                        problems.push(format!(
                            "orphaned Rapport signoff workflow `{}` has no context.toml declaration",
                            display_path(repo_root, &entry)
                        ));
                    }
                }
            }
            Err(source) => problems.push(format!(
                "could not scan Rapport signoff workflows at `{}`: {source}",
                display_path(repo_root, &workflows)
            )),
        }
    }
    problems
}

pub(crate) fn write_shared(fs: &mut impl FileSystem, repo_root: &Utf8Path) -> io::Result<()> {
    let directory = repo_root.join(WORKFLOW_DIRECTORY);
    fs.create_dir_all(&directory)?;
    fs.write_string(repo_root.join(SHARED_WORKFLOW), SHARED_WORKFLOW_CONTENTS)
}

pub(crate) fn write_request(
    fs: &mut impl FileSystem,
    repo_root: &Utf8Path,
    request: &SignoffRequest,
) -> io::Result<()> {
    fs.create_dir_all(repo_root.join(WORKFLOW_DIRECTORY))?;
    fs.write_string(request.workflow_path(), request.render(repo_root))
}

fn validate_exact_file(
    fs: &impl FileSystem,
    path: &Utf8Path,
    expected: &str,
    repo_root: &Utf8Path,
    problems: &mut Vec<String>,
) {
    if !fs.is_file(path) {
        problems.push(format!(
            "missing Rapport-owned signoff workflow `{}`",
            display_path(repo_root, path)
        ));
        return;
    }
    match fs.read_to_string(path) {
        Ok(contents) if contents == expected => {}
        Ok(_) => problems.push(format!(
            "Rapport-owned signoff workflow `{}` has drifted from its generated content",
            display_path(repo_root, path)
        )),
        Err(source) => problems.push(format!(
            "could not read Rapport-owned signoff workflow `{}`: {source}",
            display_path(repo_root, path)
        )),
    }
}

fn valid_target(target: &str) -> bool {
    !target.is_empty()
        && target
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !target.starts_with('-')
        && !target.ends_with('-')
        && !target.contains("--")
}

fn slug(folder: &str) -> String {
    let mut value = String::new();
    let mut separator = false;
    for character in folder.chars() {
        if character.is_ascii_alphanumeric() {
            if separator && !value.is_empty() {
                value.push('-');
            }
            value.push(character.to_ascii_lowercase());
            separator = false;
        } else {
            separator = true;
        }
    }
    value
}

fn display_path(repo_root: &Utf8Path, path: &Utf8Path) -> String {
    path.strip_prefix(repo_root)
        .unwrap_or(path)
        .as_str()
        .replace('\\', "/")
}

#[derive(Debug)]
pub(crate) enum SignoffContractError {
    InvalidTarget { target: String },
    OutsideRepository { path: Utf8PathBuf },
}

impl fmt::Display for SignoffContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTarget { target } => write!(
                f,
                "invalid signoff target `{target}`; use lowercase kebab-case"
            ),
            Self::OutsideRepository { path } => {
                write!(f, "signoff context `{path}` is outside the repository")
            }
        }
    }
}

impl std::error::Error for SignoffContractError {}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use rapport_files::InMemoryFileSystem;

    #[test]
    fn request_derives_workflow_and_status_from_folder_and_target() {
        let request = SignoffRequest::new(
            Utf8Path::new("/repo"),
            Utf8Path::new("/repo/app/apple"),
            "regression-ios",
        )
        .unwrap();

        assert_eq!(request.qualified_target(), "app-apple-regression-ios");
        assert_eq!(
            request.workflow_path(),
            Utf8Path::new("/repo/.github/workflows/rapport-app-apple-regression-ios.yml")
        );
        let rendered = request.render(Utf8Path::new("/repo"));
        assert!(rendered.contains("target: app-apple-regression-ios"));
        assert!(rendered.contains("- \"app/apple/**\""));
    }

    #[test]
    fn checked_in_shared_workflow_matches_generated_bytes() {
        assert_eq!(
            include_str!("../../../.github/workflows/rapport-signoff.yml"),
            SHARED_WORKFLOW_CONTENTS
        );
    }

    #[test]
    fn validation_requires_exact_generated_files() {
        let mut fs = InMemoryFileSystem::default();
        fs.add_directory("/repo/.git");
        let request = SignoffRequest::new(
            Utf8Path::new("/repo"),
            Utf8Path::new("/repo/app/apple"),
            "ci",
        )
        .unwrap();
        write_shared(&mut fs, Utf8Path::new("/repo")).unwrap();
        write_request(&mut fs, Utf8Path::new("/repo"), &request).unwrap();

        assert!(validate(&fs, Utf8Path::new("/repo"), std::slice::from_ref(&request)).is_empty());

        fs.write_string(request.workflow_path(), "changed\n")
            .unwrap();
        let problems = validate(&fs, Utf8Path::new("/repo"), &[request]);

        assert!(problems.iter().any(|problem| problem.contains("drifted")));
    }

    #[test]
    fn validation_rejects_orphaned_rapport_workflow() {
        let mut fs = InMemoryFileSystem::default();
        fs.add_directory("/repo/.git");
        write_shared(&mut fs, Utf8Path::new("/repo")).unwrap();
        fs.write_string(
            "/repo/.github/workflows/rapport-app-apple-ci.yml",
            "orphaned\n",
        )
        .unwrap();

        let problems = validate(&fs, Utf8Path::new("/repo"), &[]);

        assert!(problems.iter().any(|problem| problem.contains("orphaned")));
    }
}
