use crate::runner::{CommandOutcome, CommandRunner, CommandSpec};
use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};
use sha2::{Digest, Sha256};
use std::error::Error;
use std::fmt;
use std::io;

const SNAPSHOT_PROTOCOL_VERSION: &str = "rapport-snapshot-v1";

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct OperationSnapshot {
    pub(crate) base_sha: String,
    pub(crate) head_sha: String,
    pub(crate) content_checksum: String,
    pub(crate) rules_checksum: String,
    pub(crate) instructions_checksum: String,
    pub(crate) input_checksum: String,
}

impl fmt::Debug for OperationSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OperationSnapshot")
            .field("base_sha", &RedactedSnapshotValue(&self.base_sha))
            .field("head_sha", &RedactedSnapshotValue(&self.head_sha))
            .field(
                "content_checksum",
                &RedactedSnapshotValue(&self.content_checksum),
            )
            .field(
                "rules_checksum",
                &RedactedSnapshotValue(&self.rules_checksum),
            )
            .field(
                "instructions_checksum",
                &RedactedSnapshotValue(&self.instructions_checksum),
            )
            .field(
                "input_checksum",
                &RedactedSnapshotValue(&self.input_checksum),
            )
            .finish()
    }
}

struct RedactedSnapshotValue<'a>(&'a str);

impl fmt::Debug for RedactedSnapshotValue<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<redacted; {} bytes>", self.0.len())
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "snapshot capture keeps every exact-input dimension explicit at the service boundary"
)]
pub(crate) fn capture(
    fs: &impl FileSystem,
    runner: &dyn CommandRunner,
    repo_root: &Utf8Path,
    requirement_id: &str,
    paths: &[String],
    explicit_base_sha: Option<&str>,
    rules_checksum: &str,
    instructions_checksum: &str,
) -> Result<OperationSnapshot, SnapshotError> {
    let head_sha = stdout(
        runner,
        repo_root,
        &CommandSpec::new("git", ["rev-parse", "HEAD"]),
        "git rev-parse HEAD",
    )?;
    let base_sha = match explicit_base_sha {
        Some(base) => base.to_string(),
        None => discover_base(runner, repo_root, &head_sha)?,
    };

    let mut diff_args = vec![
        String::from("diff"),
        String::from("--binary"),
        String::from("--no-ext-diff"),
        String::from("--no-textconv"),
        String::from("--no-renames"),
        base_sha.clone(),
        String::from("--"),
    ];
    diff_args.extend(paths.iter().cloned());
    let diff = stdout_preserving(
        runner,
        repo_root,
        &CommandSpec::new("git", diff_args),
        "git diff snapshot",
    )?;

    let mut untracked_args = vec![
        String::from("ls-files"),
        String::from("--others"),
        String::from("--exclude-standard"),
        String::from("-z"),
        String::from("--"),
    ];
    untracked_args.extend(paths.iter().cloned());
    let untracked = stdout_preserving(
        runner,
        repo_root,
        &CommandSpec::new("git", untracked_args),
        "git ls-files untracked snapshot",
    )?;
    let mut file_patches = split_file_patches(&diff);
    for path in untracked.split('\0').filter(|path| !path.is_empty()) {
        let patch = untracked_diff(
            runner,
            repo_root,
            &CommandSpec::new(
                "git",
                [
                    "diff",
                    "--no-index",
                    "--binary",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--",
                    "/dev/null",
                    path,
                ],
            ),
        )?;
        if patch.is_empty() {
            // Git emits no patch at all for an empty untracked file. Preserve
            // its path and executable mode so add/remove/rename/chmod changes
            // still alter the exact local input checksum. Once committed, this
            // conservative marker may force a safe rerun instead of reuse.
            let absolute_path = repo_root.join(path);
            let mode =
                fs.git_file_mode(&absolute_path)
                    .map_err(|source| SnapshotError::FileMetadata {
                        path: absolute_path,
                        source,
                    })?;
            file_patches.push(format!("rapport-empty-untracked-v1\0{path}\0{mode:o}"));
        } else {
            // `git diff --no-index --binary` includes both `new file mode` and
            // the complete textual/binary content, so chmod changes are exact inputs.
            file_patches.push(patch);
        }
    }
    file_patches.sort();

    let content_checksum = checksum(file_patches.iter().map(String::as_str));
    let input_checksum = checksum([
        SNAPSHOT_PROTOCOL_VERSION,
        requirement_id,
        &base_sha,
        &content_checksum,
        rules_checksum,
        instructions_checksum,
    ]);

    Ok(OperationSnapshot {
        base_sha,
        head_sha,
        content_checksum,
        rules_checksum: rules_checksum.to_string(),
        instructions_checksum: instructions_checksum.to_string(),
        input_checksum,
    })
}

fn discover_base(
    runner: &dyn CommandRunner,
    repo_root: &Utf8Path,
    head_sha: &str,
) -> Result<String, SnapshotError> {
    let remote_default = stdout(
        runner,
        repo_root,
        &CommandSpec::new(
            "git",
            [
                "symbolic-ref",
                "--quiet",
                "--short",
                "refs/remotes/origin/HEAD",
            ],
        ),
        "git symbolic-ref origin HEAD",
    )?;
    stdout(
        runner,
        repo_root,
        &CommandSpec::new("git", ["merge-base", head_sha, &remote_default]),
        "git merge-base snapshot",
    )
}

pub(crate) fn checksum<'value>(values: impl IntoIterator<Item = &'value str>) -> String {
    let mut digest = Sha256::new();
    for value in values {
        digest.update(value.len().to_be_bytes());
        digest.update(value.as_bytes());
    }
    format!("{:x}", digest.finalize())
}

fn split_file_patches(diff: &str) -> Vec<String> {
    let mut patches = Vec::new();
    let mut current = String::new();
    for line in diff.split_inclusive('\n') {
        if line.starts_with("diff --git ") && !current.is_empty() {
            patches.push(std::mem::take(&mut current));
        }
        current.push_str(line);
    }
    if !current.is_empty() {
        patches.push(current);
    }
    patches
}

fn stdout(
    runner: &dyn CommandRunner,
    cwd: &Utf8Path,
    spec: &CommandSpec,
    display: &'static str,
) -> Result<String, SnapshotError> {
    stdout_preserving(runner, cwd, spec, display).map(|value| value.trim().to_string())
}

fn stdout_preserving(
    runner: &dyn CommandRunner,
    cwd: &Utf8Path,
    spec: &CommandSpec,
    display: &'static str,
) -> Result<String, SnapshotError> {
    match runner.run(spec, cwd) {
        Ok(outcome) if outcome.success => Ok(outcome.stdout),
        Ok(outcome) => Err(SnapshotError::CommandFailed {
            command: display,
            outcome,
        }),
        Err(source) => Err(SnapshotError::Invoke {
            command: display,
            source,
        }),
    }
}

fn untracked_diff(
    runner: &dyn CommandRunner,
    cwd: &Utf8Path,
    spec: &CommandSpec,
) -> Result<String, SnapshotError> {
    match runner.run(spec, cwd) {
        // `git diff --no-index` exits 1 when it successfully found a diff.
        Ok(outcome)
            if outcome.success
                || (!outcome.stdout.is_empty() && outcome.stderr.trim().is_empty()) =>
        {
            Ok(outcome.stdout)
        }
        Ok(outcome) => Err(SnapshotError::CommandFailed {
            command: "git diff untracked snapshot",
            outcome,
        }),
        Err(source) => Err(SnapshotError::Invoke {
            command: "git diff untracked snapshot",
            source,
        }),
    }
}

pub(crate) enum SnapshotError {
    CommandFailed {
        command: &'static str,
        outcome: CommandOutcome,
    },
    Invoke {
        command: &'static str,
        source: io::Error,
    },
    FileMetadata {
        path: Utf8PathBuf,
        source: io::Error,
    },
}

impl fmt::Debug for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (kind, command, source_kind) = match self {
            Self::CommandFailed { command, .. } => ("command_failed", Some(*command), None),
            Self::Invoke {
                command, source, ..
            } => ("invoke", Some(*command), Some(source.kind())),
            Self::FileMetadata { source, .. } => ("file_metadata", None, Some(source.kind())),
        };
        f.debug_struct("SnapshotError")
            .field("kind", &kind)
            .field("command", &command)
            .field("source_kind", &source_kind)
            .finish()
    }
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CommandFailed { command, outcome } => write!(
                f,
                "`{command}` failed while checksumming operation inputs (stdout {} bytes, stderr {} bytes)",
                outcome.stdout.len(),
                outcome.stderr.len()
            ),
            Self::Invoke { command, source } => {
                write!(
                    f,
                    "could not run `{command}` while checksumming inputs ({:?})",
                    source.kind()
                )
            }
            Self::FileMetadata { path, source } => {
                write!(
                    f,
                    "could not read snapshot file metadata (path {} bytes; {:?})",
                    path.as_str().len(),
                    source.kind()
                )
            }
        }
    }
}

impl Error for SnapshotError {}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "snapshot tests use fixed in-memory fixtures whose setup must succeed"
)]
mod tests {
    use super::*;
    use rapport_files::InMemoryFileSystem;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    struct FakeRunner {
        outcomes: RefCell<VecDeque<CommandOutcome>>,
        calls: RefCell<Vec<CommandSpec>>,
    }

    impl FakeRunner {
        fn snapshot_with_mode(mode: &str) -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                outcomes: RefCell::new(
                    [
                        successful("head123\n"),
                        successful(""),
                        successful("script.sh\0"),
                        CommandOutcome {
                            success: false,
                            stdout: format!(
                                "diff --git a/script.sh b/script.sh\nnew file mode {mode}\n+echo hi\n"
                            ),
                            stderr: String::new(),
                        },
                    ]
                    .into(),
                ),
            }
        }

        fn untracked_new_file(patch: &str) -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                outcomes: RefCell::new(
                    vec![
                        successful("head123\n"),
                        successful(""),
                        successful("new.rs\0"),
                        CommandOutcome {
                            success: false,
                            stdout: patch.to_string(),
                            stderr: String::new(),
                        },
                    ]
                    .into(),
                ),
            }
        }

        fn committed_new_file(patch: &str) -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                outcomes: RefCell::new(
                    vec![successful("head456\n"), successful(patch), successful("")].into(),
                ),
            }
        }

        fn empty_untracked(path: &str) -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                outcomes: RefCell::new(
                    vec![
                        successful("head123\n"),
                        successful(""),
                        successful(&format!("{path}\0")),
                        successful(""),
                    ]
                    .into(),
                ),
            }
        }

        fn clean() -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                outcomes: RefCell::new(
                    vec![successful("head123\n"), successful(""), successful("")].into(),
                ),
            }
        }

        fn calls(&self) -> Vec<CommandSpec> {
            self.calls.borrow().clone()
        }
    }

    impl CommandRunner for FakeRunner {
        fn run(&self, spec: &CommandSpec, _cwd: &Utf8Path) -> io::Result<CommandOutcome> {
            self.calls.borrow_mut().push(spec.clone());
            Ok(self.outcomes.borrow_mut().pop_front().unwrap())
        }
    }

    fn successful(stdout: &str) -> CommandOutcome {
        CommandOutcome {
            success: true,
            stdout: stdout.to_string(),
            stderr: String::new(),
        }
    }

    #[test]
    fn checksum_should_separate_values_unambiguously() {
        assert_ne!(checksum(["ab", "c"]), checksum(["a", "bc"]));
        assert_eq!(checksum(["same"]), checksum(["same"]));
    }

    #[test]
    fn snapshot_diagnostics_redact_proof_values_paths_and_captured_output() {
        let snapshot = OperationSnapshot {
            base_sha: String::from("PRIVATE-BASE"),
            head_sha: String::from("PRIVATE-HEAD"),
            content_checksum: String::from("PRIVATE-CONTENT"),
            rules_checksum: String::from("PRIVATE-RULES"),
            instructions_checksum: String::from("PRIVATE-INSTRUCTIONS"),
            input_checksum: String::from("PRIVATE-INPUT"),
        };
        let error = SnapshotError::FileMetadata {
            path: Utf8PathBuf::from("private/file.rs"),
            source: io::Error::new(io::ErrorKind::NotFound, "PRIVATE-SOURCE"),
        };

        let command_error = SnapshotError::CommandFailed {
            command: "git diff snapshot",
            outcome: CommandOutcome {
                success: false,
                stdout: String::from("PRIVATE-DIFF-CONTENTS"),
                stderr: String::from("PRIVATE-STDERR"),
            },
        };
        let diagnostics = format!("{snapshot:?} {error:?} {error} {command_error}");

        assert!(!diagnostics.contains("PRIVATE"));
        assert!(!diagnostics.contains("private/file.rs"));
        assert!(diagnostics.contains("<redacted;"));
        assert!(diagnostics.contains("file_metadata"));
    }

    #[test]
    fn untracked_executable_mode_changes_content_checksum() {
        let regular = capture(
            &InMemoryFileSystem::default(),
            &FakeRunner::snapshot_with_mode("100644"),
            Utf8Path::new("/repo"),
            "root-build-ci",
            &[String::from("script.sh")],
            Some("base123"),
            "rules",
            "instructions",
        )
        .unwrap();
        let executable = capture(
            &InMemoryFileSystem::default(),
            &FakeRunner::snapshot_with_mode("100755"),
            Utf8Path::new("/repo"),
            "root-build-ci",
            &[String::from("script.sh")],
            Some("base123"),
            "rules",
            "instructions",
        )
        .unwrap();

        assert_ne!(regular.content_checksum, executable.content_checksum);
        assert_ne!(regular.input_checksum, executable.input_checksum);
    }

    #[test]
    fn content_checksum_is_stable_when_untracked_file_becomes_committed() {
        let patch = "diff --git a/new.rs b/new.rs\nnew file mode 100644\nindex 0000000..1234567\n--- /dev/null\n+++ b/new.rs\n@@ -0,0 +1 @@\n+fn new() {}\n";
        let untracked = capture(
            &InMemoryFileSystem::default(),
            &FakeRunner::untracked_new_file(patch),
            Utf8Path::new("/repo"),
            "root-review",
            &[String::from("new.rs")],
            Some("base123"),
            "rules",
            "instructions",
        )
        .unwrap();
        let committed = capture(
            &InMemoryFileSystem::default(),
            &FakeRunner::committed_new_file(patch),
            Utf8Path::new("/repo"),
            "root-review",
            &[String::from("new.rs")],
            Some("base123"),
            "rules",
            "instructions",
        )
        .unwrap();

        assert_eq!(untracked.content_checksum, committed.content_checksum);
        assert_eq!(untracked.input_checksum, committed.input_checksum);
        assert_ne!(untracked.head_sha, committed.head_sha);
    }

    #[test]
    fn empty_untracked_paths_are_exact_snapshot_inputs() {
        let clean = capture(
            &InMemoryFileSystem::default(),
            &FakeRunner::clean(),
            Utf8Path::new("/repo"),
            "root-review",
            &[String::from(".")],
            Some("base123"),
            "rules",
            "instructions",
        )
        .unwrap();
        let mut empty_fs = InMemoryFileSystem::default();
        empty_fs.add_file("/repo/empty.txt");
        empty_fs.add_file("/repo/renamed.txt");
        let empty = capture(
            &empty_fs,
            &FakeRunner::empty_untracked("empty.txt"),
            Utf8Path::new("/repo"),
            "root-review",
            &[String::from(".")],
            Some("base123"),
            "rules",
            "instructions",
        )
        .unwrap();
        let renamed = capture(
            &empty_fs,
            &FakeRunner::empty_untracked("renamed.txt"),
            Utf8Path::new("/repo"),
            "root-review",
            &[String::from(".")],
            Some("base123"),
            "rules",
            "instructions",
        )
        .unwrap();

        assert_ne!(clean.content_checksum, empty.content_checksum);
        assert_ne!(empty.content_checksum, renamed.content_checksum);
        assert_ne!(clean.input_checksum, empty.input_checksum);
    }

    #[test]
    fn snapshot_diffs_disable_text_conversion() {
        let mut fs = InMemoryFileSystem::default();
        fs.add_file("/repo/empty.txt");
        let runner = FakeRunner::empty_untracked("empty.txt");

        capture(
            &fs,
            &runner,
            Utf8Path::new("/repo"),
            "root-review",
            &[String::from(".")],
            Some("base123"),
            "rules",
            "instructions",
        )
        .unwrap();

        let diff_calls = runner
            .calls()
            .into_iter()
            .filter(|call| {
                call.program == "git" && call.args.first().is_some_and(|arg| arg == "diff")
            })
            .collect::<Vec<_>>();
        assert_eq!(diff_calls.len(), 2);
        assert!(
            diff_calls
                .iter()
                .all(|call| call.args.iter().any(|arg| arg == "--no-textconv"))
        );
    }
}
