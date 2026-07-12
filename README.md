# Rapport

Let your agents have rapport with your repository.

Rapport is a repository workflow layer for human-directed agent work. It keeps
active work grounded in repository-owned rules, build conventions, GitHub
integration, and local state.

The first delivery loop is intentionally tight:

```text
work -> build -> review -> integrate
```

The goal is not to invent another build system. Rapport sits on top of the
durable tools and conventions a repository already owns: checked-in rules,
conventional Just targets, Git/GitHub, local `.rapport` work state, and command
telemetry.

## Install

Install the latest released binary with:

```bash
cargo binstall rapport
```

## Inner Loop

Rapport's first responsibility is the inner development loop:

- `work` creates and reports active local work context: title, ticket,
  objective, paths, stage, and applicable rules.
- `build` runs applicable typed build declarations through existing repository
  Just conventions and records their exact inputs.
- `review start` emits a product-neutral Markdown prompt for an independent
  adversarial reviewer, while `review complete` records its structured JSON
  result against the exact reviewed inputs.
- `integrate` turns local work into durable Git/GitHub state: commit, PR,
  signoff status, and remaining action.

The intended command surface is:

```text
rapport work start
rapport work status
rapport work add path <path>
rapport work rules list
rapport work rules show <id>
rapport rules catalog
rapport rules add rust
rapport rules add crux
rapport rules list
rapport rules show <ruleset-id>
rapport rules init <path> --id <ruleset-id>
rapport rules include add <ruleset-id> <included-id>
rapport rules rule add <ruleset-id> --id <rule-id> --text "..."
rapport context ruleset id set <path> <ruleset-id>
rapport work task address REV-001 --summary "what changed"
rapport build                              # acceptance proof for active Work; otherwise just dev
rapport build <path>                       # ad hoc just dev feedback
rapport build <path> --target <just-target>
rapport build status [task-id]
rapport review start [path...]
rapport review start --json [path...] # optional machine-readable request
rapport review complete --result /tmp/review-result.json
rapport integrate --summary "..." --message "..."
rapport integrate # retry signoff for the recorded PR
```

## Repository Shape

Rapport should work with repository-owned conventions rather than replacing
them:

- checked-in standalone and composite rulesets in `.rapport/rules/**/*.toml`,
  including optional versioned built-in Rust and Crux packs;
- exact installed pack versions and semantic digests in `.rapport/rules.lock`;
- embedded rulesets in folder `context.toml` files;
- conventional Just targets for project-specific validation;
- Git and GitHub for commits, pull requests, and status checks;
- inherited `signoffs` in folder `context.toml` files, fulfilled by matching
  GitHub Actions workflows;
- ignored local state in `.rapport/work.toml`, while `.rapport/rules/**` and
  `.rapport/rules.lock` remain trackable;
- local command telemetry in `.rapport/events.jsonl`.

Just remains the right home for installs, local servers, generated assets,
bespoke checks, deploys, release tasks, and ecosystem-specific details. Rapport
uses those conventions to keep agents oriented inside the current work.

Folder contexts declare integration needs close to the code they govern:

```toml
version = 1
purpose = "Owns the Apple application."

[[signoffs]]
kind = "build"
target = "ci"

[[signoffs]]
kind = "review"
minimum_grade = "A-"
```

Manage targets through Rapport so their GitHub request workflows stay aligned:

```text
rapport context signoff add app/apple build ci
rapport context signoff add app/apple review --minimum-grade A-
rapport context signoff remove app/apple review
rapport context signoff repair app/apple build ci
```

Signoff-owning folder components use ASCII letters, digits, dots, underscores,
or hyphens and each component must contain at least one letter or digit, so
their generated YAML path filters and readable identities remain unambiguous.
Generated identities use `[folder-path|root]-[build-[target]|review]`: for example,
`app-apple-build-ci`, `app-apple-review`, and `root-review`. Reviews are one
comprehensive, adversarial check per declaring folder; security and other
concerns come from the resolved rules and instructions rather than review
profiles. Because readable folder slugs are intentionally lossy, Rapport rejects
collisions such as `app/apple` versus `app-apple`, as well as identities over
GitHub's 140-byte status-context limit and duplicate identities within one
context, before mutating either the context or generated workflows.

Local telemetry records the stable Rapport command, argument count, outcome,
and exit code. It never persists raw command arguments, which may contain issue
text, review summaries, commit messages, or pull-request bodies. External
command diagnostics likewise report the program and argument count without
rendering argument values.
Before appending a schema-v2 event, Rapport rewrites legacy event lines once to
remove persisted argv values and derive their argument count. Malformed legacy
lines are discarded rather than retaining potentially sensitive text; the
sanitized schema-v2 log is append-only afterward.

Adding the build above to `app/apple/context.toml` generates the exact
Rapport-owned `.github/workflows/rapport-app-apple-build-ci.yml` request
workflow. On matching pull
requests it calls the shared `.github/workflows/rapport-signoff.yml` workflow,
which posts `signoff: app-apple-build-ci` as pending for the PR head SHA. Folder
and kind are always part of workflow names and status contexts; build identities
also include their target. It does not run repository validation in GitHub;
`rapport integrate` runs `just ci` from the declaring folder on the local host
and posts the SHA-bound result.

Signoffs inherit from ancestor contexts and union across active work paths. A
parent build and child review therefore remain independent requirements. The
old string form, `signoffs = ["ci"]`, remains readable and means a build named
`ci`. The next context edit (including `signoff add` or `signoff repair`) renders
typed `[[signoffs]]` tables, writes the new kind-qualified workflow, and removes
the corresponding legacy workflow. That is the explicit migration path; review
declarations have no string shorthand. Cleanup preserves any legacy-named path
that is also owned by a distinct current typed declaration.

`rapport review start` resolves each applicable review declaration, active
paths, and context benchmarks. By default it emits a Markdown request containing
the host-neutral JSON contract, adversarial instructions, resolved rules,
base/head SHAs, and deterministic content, rule, instruction, and aggregate
input checksums. `--json` emits only the request packets for hosts that prefer
machine-readable orchestration. Reviewers must form current findings first; the
request then supplies a separate prior-task reconciliation ledger. The prompt
includes the grading rubric but deliberately withholds Rapport's passing
threshold so the reviewer grades the evidence instead of targeting policy.

A capable host hands that request to a fresh independent reviewer and writes
the returned JSON outside the reviewed content—for example under `/tmp`—before
running `rapport review complete --result <file>`. Rapport rejects a result file
inside any reviewed path because the protocol file would change the snapshot it
is meant to attest. The reviewer returns an A-F grade
with optional `+` or `-`, a description, and current actions with cited rule IDs
and concrete evidence. It sets `prior_task_id` only when a finding matches an
outstanding task from the reconciliation ledger; new actions have no ID.
Rapport validates the checksum, assigns work-global `REV-###` IDs, reconciles
prior tasks, applies the declared threshold, derives pass or fail, and emits a
Markdown result with exactly one next command. Recording a valid failing review
is a successful command; integration and completion enforce the failure as a
gate. The default passing threshold is A- and any outstanding action fails the
review.

New findings are `open`. After implementing a correction, the working agent
runs `rapport work task address REV-### --summary "..."`; the task becomes
`addressed`, not resolved. A later independent review reopens it by returning
the same `prior_task_id`, or resolves it by omitting it. Resolved tasks and all
attempts remain in work history. Every new input checksum remains pending until
a matching result is accepted; a prior grade is history, not proof for the new
request. Supplying explicit paths scopes each shared review requirement, its
resolved rules, and its checksum to those paths instead of retaining unrelated
active-work paths. Explicit build and review scopes must be descendants of an
active-work path; parent traversal and ancestor widening are rejected. Local
reviews include uncommitted changes in their content checksum, so `HEAD` is
never treated as sufficient proof.

When multiple folder reviews apply, the Markdown request contains the complete
request array and requires one JSON result array in the same order. A single
review continues to use one request object and one result object.

`rapport work status` recomputes current build and review inputs. It shows each
required build and review as missing, pending, stale, passing, or failing, with
its SHA/checksum; reviews also include grade and outstanding tasks with their
`open` or `addressed` state. Editing signed content makes the prior result stale
while retaining review tasks. A later review reconciles the current task ledger
and records resolved IDs and all prior attempts in work history. When an exact
review input remains reusable across a commit, status refreshes the displayed
head SHA to the current commit. Once all
typed requirements pass—including review-only or no-signoff work—the next step
is integration rather than an inapplicable build command.

`rapport doctor` compares generated workflows byte-for-byte with their typed
context declarations. `rapport integrate` runs the same validation before
committing or opening a PR and unions inherited requirements for every active
work path. Integration does not depend on the legacy aggregate `build` fact: it
runs or exactly reuses each typed build and review requirement directly, so a
review-only context is valid. It records
commit intent before creating the commit, promotes that to publication state
before pushing, then records the open same-repository PR and its pending
signoffs before attempting proof. Signoff requires a completely clean worktree
before and after every target, rejects forks and missing or unexpected statuses,
and reconciles the final SHA-bound status set. Integration calls the same build
and review services as the first-class verbs. A local result is reused only when
its content, base, declaration/instruction, and rule checksums match the exact PR
inputs; otherwise the operation reruns, or a fresh review request remains
pending. Even an existing successful GitHub status is accepted only after that
local exact-input proof is re-evaluated. A failed or interrupted attempt leaves durable state, so a bare
`rapport integrate` resumes without a duplicate commit, PR, or completed
operation. Missing or stale builds, and missing, stale, below-threshold, or
action-bearing reviews, block both integration success and work completion.
Normal work completion also requires local `HEAD` to equal the recorded
integrated PR head; `--without-integrate` remains the explicit local-only path.

## Later Phases

The outer factory phases are separate from the first loop:

- `plan` comes before active local work and ensures durable tickets/plans exist.
- `ship` comes after integration and handles release, deploy, and completion
  workflows.

Those phases are intentionally outside the first implementation slice.

## Building Blocks

Keeper crates:

- `rapport-prose` - markdown-ish output primitives;
- `rapport-temporal` - date, time, and recurrence primitives.
- `rapport-files` - fake/real filesystem support for testable workflow code.

## Development

Local checks are driven through the repository `justfile`:

```bash
just check
just build
just test
just ci
```

Command-level coverage should grow around the new `work -> build -> integrate`
surface as it lands.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <http://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.
