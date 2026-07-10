use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io;

pub(crate) const SHARED_WORKFLOW: &str = ".github/workflows/rapport-signoff.yml";
const WORKFLOW_DIRECTORY: &str = ".github/workflows";
const REQUEST_PREFIX: &str = "rapport-";
const MAX_GITHUB_STATUS_CONTEXT_BYTES: usize = 140;
const STATUS_CONTEXT_PREFIX: &str = "signoff: ";

const SHARED_WORKFLOW_CONTENTS: &str = r#"name: Rapport signoff (reusable)

# Rapport owns this file byte-for-byte. Run `rapport context signoff repair` to restore it.
# It asks for SHA-bound local proof by posting a pending `signoff: <folder>-build-<target>` or `signoff: <folder>-review` status.

on:
  workflow_call:
    inputs:
      target:
        description: "Folder- and kind-qualified signoff; build identities also include the target"
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SignoffKind {
    Build,
    Review,
}

impl fmt::Display for SignoffKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Build => f.write_str("build"),
            Self::Review => f.write_str("review"),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct SignoffRequest {
    context_directory: Utf8PathBuf,
    folder: String,
    kind: SignoffKind,
    target: String,
    minimum_grade: Option<crate::state::ReviewGrade>,
    qualified_target: String,
    workflow_path: Utf8PathBuf,
}

impl fmt::Debug for SignoffRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SignoffRequest")
            .field(
                "context_directory",
                &RedactedSignoffText(self.context_directory.as_str()),
            )
            .field("folder", &RedactedSignoffText(&self.folder))
            .field("kind", &self.kind)
            .field("target", &RedactedSignoffText(&self.target))
            .field("minimum_grade", &self.minimum_grade)
            .field(
                "qualified_target",
                &RedactedSignoffText(&self.qualified_target),
            )
            .field(
                "workflow_path",
                &RedactedSignoffText(self.workflow_path.as_str()),
            )
            .finish()
    }
}

struct RedactedSignoffText<'a>(&'a str);

impl fmt::Debug for RedactedSignoffText<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<redacted; {} bytes>", self.0.len())
    }
}

impl SignoffRequest {
    pub(crate) fn new(
        repo_root: &Utf8Path,
        context_directory: &Utf8Path,
        kind: SignoffKind,
        target: &str,
        minimum_grade: Option<crate::state::ReviewGrade>,
    ) -> Result<Self, SignoffContractError> {
        let target = target.trim();
        if !valid_target(target) {
            return Err(SignoffContractError::InvalidTarget {
                target: target.to_string(),
            });
        }
        if kind == SignoffKind::Review && target != "review" {
            return Err(SignoffContractError::UnexpectedReviewTarget);
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
        if folder != "." && !valid_folder_path(&folder) {
            return Err(SignoffContractError::InvalidFolder {
                path: relative.to_path_buf(),
            });
        }
        let folder_slug = if folder == "." {
            String::from("root")
        } else {
            slug(&folder)
        };
        if kind == SignoffKind::Build && minimum_grade.is_some() {
            return Err(SignoffContractError::BuildMinimumGrade);
        }
        let qualified_target = match kind {
            SignoffKind::Build => format!("{folder_slug}-build-{target}"),
            SignoffKind::Review => format!("{folder_slug}-review"),
        };
        let status_context_bytes = STATUS_CONTEXT_PREFIX.len() + qualified_target.len();
        if status_context_bytes > MAX_GITHUB_STATUS_CONTEXT_BYTES {
            return Err(SignoffContractError::IdentityTooLong {
                folder,
                target: target.to_string(),
                bytes: status_context_bytes,
                maximum: MAX_GITHUB_STATUS_CONTEXT_BYTES,
            });
        }
        let workflow_path = repo_root
            .join(WORKFLOW_DIRECTORY)
            .join(format!("{REQUEST_PREFIX}{qualified_target}.yml"));
        Ok(Self {
            context_directory: context_directory.to_path_buf(),
            folder,
            kind,
            target: target.to_string(),
            minimum_grade: match kind {
                SignoffKind::Build => None,
                SignoffKind::Review => Some(minimum_grade.unwrap_or_default()),
            },
            qualified_target,
            workflow_path,
        })
    }

    pub(crate) fn target(&self) -> &str {
        &self.target
    }

    pub(crate) fn kind(&self) -> SignoffKind {
        self.kind
    }

    pub(crate) fn minimum_grade(&self) -> Option<crate::state::ReviewGrade> {
        self.minimum_grade
    }

    pub(crate) fn declaring_context(&self) -> &str {
        &self.folder
    }

    pub(crate) fn context_directory(&self) -> &Utf8Path {
        &self.context_directory
    }

    pub(crate) fn qualified_target(&self) -> &str {
        &self.qualified_target
    }

    pub(crate) fn workflow_path(&self) -> &Utf8Path {
        &self.workflow_path
    }

    pub(crate) fn legacy_workflow_path(&self, repo_root: &Utf8Path) -> Utf8PathBuf {
        let legacy_folder = if self.folder == "." {
            String::from("root")
        } else {
            slug(&self.folder)
        };
        repo_root.join(WORKFLOW_DIRECTORY).join(format!(
            "{REQUEST_PREFIX}{}-{}.yml",
            legacy_folder, self.target
        ))
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
        let target_argument = match self.kind {
            SignoffKind::Build => format!(" {}", self.target),
            SignoffKind::Review => String::new(),
        };
        format!(
            "name: \"Rapport signoff: {qualified}\"\n\n# Rapport owns this file byte-for-byte. Run `rapport context signoff repair {folder} {kind}{target_argument}` to restore it.\n\non:\n  pull_request:\n    paths:\n{paths}      - \"{workflow}\"\n\nconcurrency:\n  group: ${{{{ github.workflow }}}}-${{{{ github.event.pull_request.number || github.ref }}}}\n  cancel-in-progress: true\n\njobs:\n  signoff:\n    if: github.event.pull_request.head.repo.full_name == github.repository\n    uses: ./.github/workflows/rapport-signoff.yml\n    with:\n      target: {qualified}\n    secrets: inherit\n",
            qualified = self.qualified_target,
            folder = self.folder,
            kind = self.kind,
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
            request.declaring_context().to_string(),
        ) {
            problems.push(format!(
                "signoff identity collision `{}` between declaring contexts `{}` and `{}`",
                request.qualified_target(),
                first,
                request.declaring_context()
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

fn valid_folder_path(folder: &str) -> bool {
    folder.split('/').all(|component| {
        !component.is_empty()
            && component != "."
            && component != ".."
            && component.bytes().any(|byte| byte.is_ascii_alphanumeric())
            && component
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    })
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

pub(crate) enum SignoffContractError {
    InvalidTarget {
        target: String,
    },
    InvalidFolder {
        path: Utf8PathBuf,
    },
    OutsideRepository {
        path: Utf8PathBuf,
    },
    BuildMinimumGrade,
    MissingBuildTarget,
    UnexpectedReviewTarget,
    IdentityTooLong {
        folder: String,
        target: String,
        bytes: usize,
        maximum: usize,
    },
}

impl fmt::Debug for SignoffContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self {
            Self::InvalidTarget { .. } => "invalid_target",
            Self::InvalidFolder { .. } => "invalid_folder",
            Self::OutsideRepository { .. } => "outside_repository",
            Self::BuildMinimumGrade => "build_minimum_grade",
            Self::MissingBuildTarget => "missing_build_target",
            Self::UnexpectedReviewTarget => "unexpected_review_target",
            Self::IdentityTooLong { .. } => "identity_too_long",
        };
        f.debug_struct("SignoffContractError")
            .field("kind", &kind)
            .finish()
    }
}

impl fmt::Display for SignoffContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTarget { target } => write!(
                f,
                "invalid signoff target ({} bytes); use lowercase kebab-case",
                target.len()
            ),
            Self::InvalidFolder { path } => write!(
                f,
                "invalid signoff folder ({} bytes); each path component must contain an ASCII letter or digit and may also use dots, underscores, and hyphens",
                path.as_str().len()
            ),
            Self::OutsideRepository { path } => write!(
                f,
                "signoff context is outside the repository ({} bytes)",
                path.as_str().len()
            ),
            Self::BuildMinimumGrade => f.write_str("build signoffs cannot declare `minimum_grade`"),
            Self::MissingBuildTarget => f.write_str("build signoffs require a target"),
            Self::UnexpectedReviewTarget => {
                f.write_str("review signoffs do not accept a target or profile")
            }
            Self::IdentityTooLong {
                folder,
                target,
                bytes,
                maximum,
            } => write!(
                f,
                "signoff identity is {bytes} bytes (folder {} bytes, target {} bytes); GitHub status contexts support at most {maximum} bytes",
                folder.len(),
                target.len()
            ),
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
            SignoffKind::Build,
            "regression-ios",
            None,
        )
        .unwrap();

        assert_eq!(request.qualified_target(), "app-apple-build-regression-ios");
        assert_eq!(
            request.workflow_path(),
            Utf8Path::new("/repo/.github/workflows/rapport-app-apple-build-regression-ios.yml")
        );
        let rendered = request.render(Utf8Path::new("/repo"));
        assert!(rendered.contains("target: app-apple-build-regression-ios"));
        assert!(rendered.contains("- \"app/apple/**\""));
        assert!(!rendered.contains("branches:"));
        assert!(
            rendered
                .contains("if: github.event.pull_request.head.repo.full_name == github.repository")
        );
        let debug = format!("{request:?}");
        assert!(!debug.contains("app/apple"));
        assert!(!debug.contains("regression-ios"));
        assert!(debug.contains("<redacted;"));
    }

    #[test]
    fn request_rejects_folder_names_that_are_unsafe_in_yaml_globs() {
        for folder in ["app/\"legacy\"", "!app", "日本語", "---", "app/___/api"] {
            let result = SignoffRequest::new(
                Utf8Path::new("/repo"),
                &Utf8Path::new("/repo").join(folder),
                SignoffKind::Build,
                "ci",
                None,
            );

            assert!(matches!(
                result,
                Err(SignoffContractError::InvalidFolder { .. })
            ));
        }
    }

    #[test]
    fn qualified_identity_uses_readable_build_and_review_shapes() {
        let root_build = SignoffRequest::new(
            Utf8Path::new("/repo"),
            Utf8Path::new("/repo"),
            SignoffKind::Build,
            "ci",
            None,
        )
        .unwrap();
        let folder_build = SignoffRequest::new(
            Utf8Path::new("/repo"),
            Utf8Path::new("/repo/app/apple"),
            SignoffKind::Build,
            "ci",
            None,
        )
        .unwrap();
        let root_review = SignoffRequest::new(
            Utf8Path::new("/repo"),
            Utf8Path::new("/repo"),
            SignoffKind::Review,
            "review",
            None,
        )
        .unwrap();

        assert_eq!(root_build.qualified_target(), "root-build-ci");
        assert_eq!(folder_build.qualified_target(), "app-apple-build-ci");
        assert_eq!(root_review.qualified_target(), "root-review");
    }

    #[test]
    fn request_rejects_identity_that_exceeds_github_status_limit() {
        let long_folder = "a".repeat(130);
        let result = SignoffRequest::new(
            Utf8Path::new("/repo"),
            &Utf8Path::new("/repo").join(long_folder),
            SignoffKind::Build,
            "ci",
            None,
        );

        assert!(matches!(
            result,
            Err(SignoffContractError::IdentityTooLong {
                bytes: 140..,
                maximum: MAX_GITHUB_STATUS_CONTEXT_BYTES,
                ..
            })
        ));
    }

    #[test]
    fn signoff_error_debug_redacts_invalid_values() {
        let error = SignoffContractError::InvalidTarget {
            target: String::from("PRIVATE TARGET"),
        };

        let debug = format!("{error:?} {error}");

        assert!(!debug.contains("PRIVATE"));
        assert!(debug.contains("invalid_target"));
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
            SignoffKind::Build,
            "ci",
            None,
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
