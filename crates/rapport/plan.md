# rapport — Implementation Plan

A port of `tooling/builder` to the rapport-cli framework. The point is
twofold: ship the agent-friendly builder, and stress-test the
rapport-cli doctrine against a real CLI.

## Source

`tooling/builder/` in the people-work monorepo. Three files:

- `main.rs` — clap CLI, runs through `Cli::parse`, validates dir,
  dispatches to `runner`, prints `status: pass|FAIL` + duration +
  test counts.
- `parser.rs` — parses test output from cargo test, cargo nextest,
  and swift testing.
- `runner.rs` — invokes `just <recipe>` in a directory, captures
  output, returns a `RunResult`.

## What builder does today

Surface area, in one paragraph: builder takes a directory and a
recipe (default `dev`), finds the repo root via git, validates the
directory exists and contains a justfile, runs `just <recipe>` in
it, parses test output for counts, and prints a structured status
line plus duration. The `list` recipe is a special case that
delegates to `just --list`. Errors are plain strings to stderr.
That is source context, not the adopted rapport v1 contract.

## Translation to rapport doctrine

### R1. Structural

| Requirement | rapport implementation |
|---|---|
| R1.1 — bare CLI is summary | `rapport` (no args) shows the current directory context, known verbs, and likely next actions. |
| R1.2 — `prime [topic]` | Embedded markdown. Default topic = "using rapport." Subtopics: `verbs`, `discovery`, `output`, `test-parsing`. |
| R1.3 — `doctor` | Checks: current dir is usable, repo context is understood when present, and required tools for the detected project type are available. Each check produces ok/warn/fail. |
| R1.4 — single exec name | `rapport`. |

### R2. Command shape

Rapport is a **zero-entity CLI**. It doesn't track records; it
operates on directories. The lifecycle verbs
(**fix → lint → build → test → validate → audit**) take a typed
`RepositoryPath` argument that currently validates only that the
path exists and is a directory. The path is the identifier; there
is no separate ID type and no entity ceremony.

The current implementation is Rust-first: it maps verbs to cargo
commands in the selected directory. A later discovery layer should
choose the runner from local project markers (for example,
`Cargo.toml` means Rust and therefore cargo). `just` can become one
detected runner, but v1 is not built around a justfile requirement.

**Verb enum (v1):**

```rust
enum Verb {
    // R2.1 framework-required
    Prime { topic: Option<Topic> },
    Doctor,

    // Lifecycle actions
    Fix      { path: RepositoryPath },
    Lint     { path: RepositoryPath },
    Build    { path: RepositoryPath },
    Test     { path: RepositoryPath },
    Validate { path: RepositoryPath },
    Audit    { path: RepositoryPath },
}
```

Usage: `rapport build api_v2/crates/insights` — verb is `build`,
path is `api_v2/crates/insights`.

All lifecycle verbs are **actions** under R2.2 — they mutate the
build artifacts on disk. They report what changed (duration, test
counts) plus next-actions in their result view.

No `list` or `show` in v1. Agents can locate buildable directories
via shell (`find . -name Cargo.toml` for Rust today); humans can
read `rapport prime` for orientation. If `list`/`show` prove
needed, add them later.

**Args:**

- `RepositoryPath` — implements `Argument`; parses a path and
  validates that the directory exists. Later, project discovery can
  report "no supported project marker" with contextual hints.

- `Topic` — implements `Argument`; closed enum of prime topics.

The CLI defines no other arg types in v1.

### R3. Views

Each verb composes a view from `rapport-prose` primitives. The
templates from `rapport-cli-common` cover the common shapes; we
write only the parts that vary.

| Verb | Template | Domain content |
|---|---|---|
| `prime` | (framework-rendered) | Topic markdown. |
| `doctor` | `doctor_view` | Three checks; outcome badges. |
| bare CLI | `summary_view` | Quick orientation: lifecycle vocabulary, "try `rapport build <path>`", pointer to `rapport prime`. |
| `fix`/`lint`/`build`/`test`/`validate`/`audit` | `result_view` | Duration field, test summary section (when tests ran), next-actions: re-run, move to the next lifecycle step, etc. |

**Test summary as a section, not a layout.** The existing parser
output (`47 passed, 0 failed, 2 skipped`) becomes a sectioned
prose node inside the result view, not its own format. Doctrine
in action: the section is the unit, not the layout.

### R4. Errors

Today's three error sites map to error views:

1. **Path not found** —
   ```
   You ran: rapport build api_v2/crates/inights
   api_v2/crates/inights does not exist under the repo root.
   └ run ls api_v2/crates
   ```

2. **Unsupported project type** — future discovery layer:
   ```
   You ran: rapport build app/some-dir
   app/some-dir exists but rapport did not find a supported project marker.
   └ run ls app/some-dir
   └ run find app/some-dir -maxdepth 2 -name Cargo.toml
   ```

3. **Step failed** — the result view itself, but with the failure
   marker and the build output as a section. Next-actions: re-run,
   run a more granular step (e.g., `lint` before `validate`), see
   the test summary.

R4.3 (framework auto-captures the invocation) means the "You ran:"
line is not authored — rapport-cli inserts it.

### R5. Behavior

| Requirement | rapport |
|---|---|
| R5.1 no prompts | Already true. |
| R5.2 actions are legible | Result view names the directory, the verb, and the duration; failures include the build output. An agent can tell from output alone whether a re-run is duplicating work. |
| R5.3 prime offline, doctor degrades | Prime is embedded markdown. Doctor reports missing project tools as failed checks rather than crashing. |
| R5.4/R5.5 channels | Result views to stdout. Underlying tool output is captured; success stays concise, failure includes the relevant captured output. |

### R6. Discoverability

`rapport prime` carries the framework's standardized lifecycle
vocabulary (what `fix`/`lint`/`build`/`test`/`validate`/`audit`
mean and how project discovery chooses the underlying runner).
`--help` excerpts from prime.

## Implementation phases

1. **Phase 0 — rapport-cli skeleton.** Before rapport itself can
   compile, rapport-cli needs the macro that consumes a verb enum,
   the `Argument` trait, and the prose primitives. Out of scope
   here; tracked separately.

2. **Phase 1 — Rust cargo lifecycle.** Wire the current verbs end
   to end for Rust directories: parse CLI input, validate
   `RepositoryPath`, invoke cargo, compose result/error views,
   render. Proves the parser, runner injection, captured output,
   and prose pipeline.

3. **Phase 2 — discovery.** Detect what is runnable in the current
   directory. `Cargo.toml` maps to the Rust runner. Other markers
   can add runners later without changing the verb vocabulary.

4. **Phase 3 — prime.** Author the prime topics. `--help`
   derivation wired up so we don't drift.

5. **Phase 4 — doctor.** Checks: directory context, detected
   project type, and required project tools.

6. **Phase 5 — bare CLI summary.** Quick orientation view that
   ties everything together.

## Tensions found

Surfacing these now while writing a real CLI against the doctrine.

- **T1 — resolved.** Earlier framing: "Project is an entity but has
  no CRUD, so calling it an entity is a stretch." Resolution:
  rapport has no entity at all. It is a zero-entity CLI; its
  verbs operate on directories via a typed `RepositoryPath`
  argument. The path is the identifier — there is no separate ID
  type. Doctrine handles zero-entity CLIs explicitly in R2.3, so no
  doctrine change is needed.

- **T2 — resolved.** Earlier framing: external-mutation actions
  (builds touch files on disk, not anything rapport tracks) make
  R5.2's "detect duplicates from result view" hard. Resolution:
  R5.2 stays strict in doctrine. Rapport v1 ships without
  duplicate detection; "nothing rebuilt" surfacing is a planned
  later enhancement (parsing cargo's "0 packages compiled" and
  similar signals from the underlying tools).

- **T3 — resolved.** Earlier framing: should long actions stream
  progress to stderr? Resolution: actions **capture** underlying
  tool output rather than stream it. During the action, rapport
  is silent on both channels — streaming costs agent tokens
  unnecessarily. On success, a minimal view goes to stdout
  (status, duration, test counts). On failure, the error view
  surfaces a relevant slice of captured output (cargo's actual
  error block, the failing test output) rather than the full log.
  Worth promoting to R5.4 as framework guidance for action verbs
  generally.

- **T4 — resolved, but discovery remains open.** Earlier framing:
  `list`/`show` semantics for filesystem-derived entities differ
  from CRUD entities.
  Resolution: doctrine is agnostic about where entity data lives.
  Rapport should discover project type from marker files near the
  selected directory. Today only the direct Rust cargo mapping is
  implemented; richer discovery is the next behavior slice.

## Acceptance criteria

This port is done when:

- `rapport build app/crates/people` produces the same correct
  exit code and test counts as the equivalent Rust cargo command.
- `rapport doctor` reports actionable failures when `just` is
  missing only if the detected project runner requires `just`; it
  otherwise reports the detected project type and required tools.
- `rapport prime` returns markdown that an agent can read once
  and understand the lifecycle verbs and discovery rules.
- A failed build's error view contains the build output, the test
  summary if applicable, and at least one re-run next-action.
- `--help` text is derived from prime; no separately authored
  help strings exist anywhere in the rapport crate.
- The four tensions above are either resolved in the plan or
  documented as accepted limitations of v1.

## Open from this exercise

The doctrine-level tensions are resolved. Follow-ups:

- **R5.5 added** to the framework requirements: action verbs capture
  rather than stream underlying-tool output. Promotes T3's
  resolution from rapport-specific to framework-wide.

- **Discovery still needs implementation.** The next behavior slice
  is project detection from local markers, starting with
  `Cargo.toml` for Rust.

Setup-style verbs (init, login, anything that prompts a human)
are deferred. The people pattern — `init` for discovery,
`login` for cliclack-driven config — is the likely shape, and
neither is needed to ship rapport v1. Revisit when the first
rapport CLI actually needs a setup verb.
