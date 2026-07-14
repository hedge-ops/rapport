# Changelog

All notable changes to `rapport` are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this crate
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.3] - 2026-07-13

### Added

- Added `RUST_TEST_019`, which keeps tests focused on production logic rather
  than static fixture text while allowing assertions on generated or
  transformed output.

### Changed

- Made independent Review requests explicitly direct agent-capable hosts to
  delegate to a fresh reviewer and prohibit self-certification.
- Rendered Review requests and result contracts through `rapport-prose`.

## [0.5.2] - 2026-07-13

### Fixed

- Fixed Context initialization and Build signoffs for hidden-root repository
  paths by deriving explicit stable IDs such as `DOT_GITHUB`.

## [0.5.1] - 2026-07-13

### Added

- Added repository-owned `RAPPORT_PRIVACY` policy that classifies local paths,
  revisions, Work metadata, command details, and captured output as actionable
  operational evidence while retaining narrow redaction for actual secrets.
- Added per-Ruleset catalog versions, advancing `RUST_CODING`, `RUST_CRATE`,
  and the transitively affected `CRUX_APP` aggregate to 1.0.1 without
  relabeling unchanged component policy.

### Changed

- Made catalog Rust privacy guidance domain-neutral so repositories classify
  their own sensitive data instead of inheriting Rapport-specific assumptions.
- Changed Debug output to preserve non-sensitive repository paths, identifiers,
  revisions, policy, command arguments, captured output, and Work/Task state.

### Removed

- Removed the obsolete `events.jsonl` command telemetry subsystem and its
  public event types; Work, Tasks, proof, and immutable Work History own durable
  workflow evidence.

### Fixed

- Fixed `rapport prime` to direct agents to the existing read-only
  `rapport work task next` command.
- Fixed Context and Ruleset decoding errors to include their TOML cause, and
  identified legacy Rapport 0.4 Context fields with an actionable all-Context
  migration message.

## [0.5.0] - 2026-07-13

### Added

- Added the complete Git-backed golden path from repository policy through
  Develop, Build, Review, Integrate, and immutable local Work History.
- Added a durable Work ledger with request provenance, ordered Develop tasks,
  cheap Git checkpoints, rebase recovery, exact candidate state, and resumable
  workflow guidance.
- Added exact-candidate Build proof with staged signoff execution, optional
  machine resource groups, generated GitHub workflows, and aggregate
  `Rapport Build` status publication.
- Added independent Context-aware acceptance Review with structured grades,
  evidence-bearing findings, corrective task reconciliation, and explicit
  human-owned quality exceptions.
- Added recoverable GitHub integration with proof-carrying pull requests,
  target-freshness policies, conventional squash merging, remote branch
  cleanup, and safe resumption after partial side effects.
- Added folder-owned Context policy for purpose, ownership, boundaries,
  inherited Rulesets, minimum Review grades, and required Build signoffs.
- Added versioned `rust` and `crux` catalog Rulesets, repository and embedded
  Context Rulesets, transitive composition, typed references, CLI management,
  and `.rapport/rules.lock` version and digest verification.
- Added dedicated command and Git crates for typed process execution,
  concurrent jobs, machine resource locking, validated Git identities, and
  repository operations.

### Changed

- Changed Context, Work, Build, Review, Integrate, and doctor to resolve one
  shared repository Ruleset catalog with stable IDs and exact policy digests.
- Changed `rapport init` to manage ignored runtime state idempotently while
  preserving checked-in Rulesets.

### Fixed

- Fixed the new command and Git crates to inherit the full `RUST_CRATE` policy,
  removed obsolete workflow implementations, and enforced authored Rule
  rationales and exact validated Git branch, revision, and object identities.
- Fixed integration cleanup to tolerate GitHub deleting a merged source branch
  between Rapport's existence check and deletion request.
- Published the expanded filesystem contract as `rapport-files` 0.3.0 and
  updated `rapport-git` 0.1.1 and `rapport` to require that version, ensuring
  crates.io builds use the same APIs as workspace builds.

## [0.4.2] - 2026-07-10

### Added

- Added `rapport --version`, which prints the executable's package version.

## [0.4.1] - 2026-07-10

### Fixed

- Changed `rapport work rules list`, `rapport work rules show`, and
  `rapport work add path` to resolve inherited benchmarks and reusable rule
  libraries through structured project contexts. These commands now agree with
  `rapport context show` and no longer require a legacy folder `rules.toml`
  owner.

## [0.4.0] - 2026-07-10

### Added

- Added typed build and review signoff declarations with inherited, folder-owned
  requirements and readable folder/kind workflow identities; build identities
  also include their target.
- Added a host-neutral `rapport review start` / `rapport review complete`
  protocol with Markdown-first requests, optional request JSON, structured
  result JSON, adversarial instructions, resolved rules, grades,
  evidence-bearing actions, exact input checksums, and local
  uncommitted-change support.
- Added `rapport work task address` and work-global review-task IDs with
  `open -> addressed -> resolved` reconciliation across independent reviews.
- Added structured build and review state, review attempt/action reconciliation,
  dynamic staleness reporting, and completion gates.
- Added `rapport context signoff add`, `remove`, and `repair` commands that own
  exact GitHub workflows for requesting SHA-bound local signoffs.
- Added byte-for-byte doctor validation for shared and folder-target signoff
  request workflows.

### Changed

- Changed `rapport build` and `rapport integrate` to use one typed build service,
  and changed integration to coordinate that same service with the review
  service. Exact local results are reused only when their base, content, rule,
  and instruction inputs match the PR head operation. Integration no longer
  requires the obsolete aggregate build fact, including for review-only
  contexts.
- Kept legacy string signoffs readable as build declarations; the next context
  edit writes typed `[[signoffs]]` tables, replaces the legacy workflow with its
  kind-qualified identity, and review declarations require the typed form.
  Every implicit migration now checks repository-wide readable-identity
  collisions before changing either context or workflow files, and legacy
  cleanup preserves paths owned by a distinct typed workflow.
- Changed signoff workflows and commit statuses to use
  `[folder-path|root]-[build-[target]|review]`, such as
  `signoff: root-build-ci` and `signoff: root-review`; ambiguous readable folder
  slugs are rejected before mutation and reported by doctor. Folder components
  made only of separators are invalid because they disappear from the readable
  identity.
- Made each declared review a comprehensive adversarial review with no target or
  profile; resolved context rules and instructions supply concerns such as
  security.
- Changed review results to omit pass/fail and new-task IDs: Rapport assigns
  `REV-###` IDs, applies the repository threshold, derives the result, and emits
  Markdown guidance. Review prompts include the grading rubric but withhold the
  passing threshold to avoid anchoring the reviewer.
- Made Markdown multi-review requests define one ordered JSON result array, and
  rejected result files stored inside reviewed content so repository-wide
  reviews cannot invalidate themselves.
- Replaced the unreleased single-command review protocol rather than retaining
  an alias. Work state v2 loads with prior actions treated as open and saves as
  v3, with duplicate reviewer-assigned IDs migrated to work-global IDs; pending
  draft protocol-v1 requests must be restarted before submitting a protocol-v2
  result.
- Added redacted diagnostic formatting for build/review state, command output,
  resolved rules, proof snapshots, protocol errors, signoff requirements,
  attempts, and actions so captured output, repository-authored text, reviewer
  prose, evidence, paths, SHAs, and checksums do not leak through Debug or error
  displays. Work state/facts, completion identity errors, and user-visible
  captured operation output use the same redacted summaries.
- Changed command telemetry to schema v2, replacing raw argv with argument
  count while retaining legacy-line deserialization, sanitizing durable v1
  logs before the next append, and discarding malformed legacy lines that may
  contain private text. Changed `CommandSpec` Debug output to report only the
  program and argument count.
- Changed work status and completion to recapture build inputs, mark old proof
  stale, and reject missing, stale, or failing required builds.
- Changed work status to refresh the displayed head SHA when exact review proof
  survives a commit, and to route passing review-only or no-signoff work to
  integration without consulting the legacy aggregate build fact.
- Changed successful build guidance to request review only when a typed review
  applies; build-only work proceeds directly to integration.
- Changed review requests to expose prior tasks only in a final reconciliation
  ledger, preserving independent findings and stable Rapport task IDs.
- Changed explicit `rapport review start <path...>` requests to scope shared
  review paths, rules, instructions, and checksums to only the selected work,
  and rejected parent traversal or ancestor widening for explicit build and
  review paths.
- Changed integration to re-evaluate exact local proof even when GitHub already
  reports a successful status, and included untracked file mode in snapshots.
- Included empty untracked file paths and modes in local snapshot checksums,
  disabled Git text conversion for byte-exact diffs, fetched and revalidated the
  PR head/base merge-base around integration proof, and made multi-review result
  envelopes validate atomically with duplicate-ID rejection.
- Changed new review checksums to remain pending until their own result is
  accepted, without inheriting an earlier passing grade.
- Changed work completion to require current `HEAD` to match the integrated PR
  head, and changed all outdated build outcomes to render as stale.
- Rejected generated signoff identities over GitHub's status-context limit
  before context mutation.
- Changed integration to validate the complete signoff contract before any Git
  commit or pull-request side effect, persist the PR before attempting signoff,
  execute requested Just targets locally, and post folder-qualified SHA-bound
  statuses. A later bare `rapport integrate` resumes signoff on the recorded PR.
- Changed PR integration to push its commit and update an existing open PR for
  the branch instead of attempting to create a duplicate.
- Made integration recoverable by persisting intent before committing and
  publication state before remote side effects. Resumed integration rejects
  dirty trees, closed or mismatched PRs, forks, ambiguous branch PRs, target
  mutations, and late status drift.
- Changed work completion to require signoffs to pass, or to record that none
  were required.
- Changed generated request workflows to match every base branch and skip fork
  PRs, which are intentionally unsupported.
- Changed exact status reconciliation to request up to 100 contexts and reject
  truncated responses, and rejected folder names unsafe for generated YAML
  path filters.

## [0.3.0] - 2026-07-10

### Added

- Added inherited `signoffs` to folder-owned `context.toml` files.
- Added active-work path resolution that unions applicable signoffs and records
  them as pending integration facts for matching GitHub Actions checks.

### Changed

- Replaced repository-wide `signoffs.toml` command and manual signoffs with
  folder-owned signoff needs. GitHub Actions now own host-specific execution.

## [0.2.0] - 2026-07-09

### Added

- Added the `work`, `build`, `integrate`, `complete`, `init`, `prime`,
  `doctor`, and `context` workflow surface.
- Added GitHub Release archives for supported platforms so
  `cargo binstall rapport` can install prebuilt binaries.
- Added git-root-bounded project discovery for Cargo projects via `Cargo.toml`.

## [0.1.0] - 2026-05-02

### Added

- Added the first runnable `rapport` cargo lifecycle CLI.
- Added `fix`, `lint`, `build`, `test`, `validate`, and `audit` verbs.
- Added injected command execution for tests and production cargo runs.
- Added prose-backed success and failure output with required next actions.
- Added captured failure output for failed cargo steps.

### Changed

- Changed `build` to use `cargo check` for the fast compile-verification path.

### Known limitations

- Project discovery is not implemented yet; `rapport` currently assumes cargo
  for any directory path it is given.

## [0.0.1] - 2026-04-27

Name-reservation release. No functionality yet; running the binary prints
a pointer to the workspace.

[Unreleased]: https://github.com/hedge-ops/rapport/compare/rapport-v0.5.3...HEAD
[0.5.3]: https://github.com/hedge-ops/rapport/compare/rapport-v0.5.2...rapport-v0.5.3
[0.5.2]: https://github.com/hedge-ops/rapport/compare/rapport-v0.5.1...rapport-v0.5.2
[0.5.1]: https://github.com/hedge-ops/rapport/compare/rapport-v0.5.0...rapport-v0.5.1
[0.5.0]: https://github.com/hedge-ops/rapport/compare/rapport-v0.4.2...rapport-v0.5.0
[0.4.2]: https://github.com/hedge-ops/rapport/compare/rapport-v0.4.1...rapport-v0.4.2
[0.4.1]: https://github.com/hedge-ops/rapport/compare/rapport-v0.4.0...rapport-v0.4.1
[0.4.0]: https://github.com/hedge-ops/rapport/compare/rapport-v0.3.0...rapport-v0.4.0
[0.3.0]: https://github.com/hedge-ops/rapport/compare/rapport-v0.2.0...rapport-v0.3.0
[0.2.0]: https://github.com/hedge-ops/rapport/compare/rapport-v0.1.0...rapport-v0.2.0
[0.1.0]: https://github.com/hedge-ops/rapport/compare/rapport-v0.0.1...rapport-v0.1.0
[0.0.1]: https://github.com/hedge-ops/rapport/releases/tag/rapport-v0.0.1
