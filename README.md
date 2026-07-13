# Rapport

Let your agents have rapport with your repository.

Rapport is a software-factory workflow for human-directed agent work. It moves
repository knowledge and repeatable mechanics out of an agent's prompt and into
deterministic, inspectable commands.

```text
Plan -> Develop -> Build -> Review -> Integrate -> Ship
```

The current delivery owns Develop through Integrate. Work is the durable local
ledger connecting those phases; it is not another lifecycle phase.

## Install

```bash
cargo binstall rapport
```

## Golden Path

Start Work from a durable ticket, plan, or explicit ad hoc request:

```bash
rapport work start --ticket '#106' --title 'Integrate accepted Work' --target main
rapport work status
```

Perform ordered Develop Tasks and create cheap, coherent Git checkpoints:

```bash
rapport develop task list
rapport develop task start TASK_001
rapport develop task complete TASK_001 --result 'Implemented the requested behavior.'
rapport work checkpoint start
git add <intended-files>
rapport work checkpoint complete 'Implement requested behavior'
```

Build the exact clean checkpoint with the Just targets declared by affected
Contexts:

```bash
rapport build
rapport build status
```

Request one independent Review of the whole candidate, then record and
reconcile its structured result:

```bash
rapport review start
rapport review complete --result /tmp/review-result.json
rapport review reconcile REV_001 --accept
rapport review reconcile REV_001 --dismiss --reason 'Accepted product tradeoff.'
rapport review override --reason 'Business reason for accepting below-policy quality.'
rapport review status
```

Publish and integrate only the exact candidate accepted by Build and Review:

```bash
rapport integrate start
rapport integrate status
rapport integrate complete
```

If the attempt should not continue, cancellation closes only the pull request
Rapport can prove belongs to this Work, deletes its same-repository remote
branch, and preserves the local branch, Work, Tasks, and proof:

```bash
rapport integrate cancel --reason 'Return to product design.'
```

## Integration Contract

`rapport integrate start` does not edit, commit, build, or review source. It
requires:

- active Work on its recorded source branch;
- a clean source `HEAD` equal to the latest checkpoint;
- complete Develop Tasks;
- current passing Build proof for the exact candidate and policy;
- current passing independent Review proof for the same candidate and policy;
- no active source-control operation or other blocking Task.

Start non-force-pushes the source branch, publishes every applicable Context
Build status plus the aggregate `Rapport Build`, and creates a ready pull
request. The pull request is the aggregate Review artifact: its body carries the
Work request, checkpoints, Build proof, Review grade, finding decisions, and any
quality-policy exception. Rapport deliberately publishes no duplicate
`Rapport Review` status.

`rapport integrate status` is read-only. It reports local, pull-request head,
and target commits; proof; checks; approvals; target advancement; mergeability;
blockers; and the next command. A changed pull-request head is never
force-pushed or accepted implicitly—it returns through Develop, Build, and
Review.

`rapport integrate complete` revalidates the exact candidate and GitHub state,
obeys branch protection without administrative bypass, and requests a squash
merge. It records and resumes a merge-queue submission when applicable. After
GitHub confirms the merge, Rapport deletes the remote source branch and archives
Work without switching branches or deleting the local branch from an active
worktree.

Every external side effect is recorded in the Integration Task. Repeating
Start, Cancel, or Complete reconciles recorded and observed identities and
continues only the missing safe transition.

## Repository Policy

Contexts describe meaningful repository areas. They declare purpose,
ownership, boundaries, shared and local Rules, minimum Review quality, and
Build signoffs. Signoffs name conventional Just targets, execution stages, and
optional machine-wide resource groups.

```toml
version = 1
id = "APP_APPLE"
purpose = "Owns the Apple application."
next_ownership = 1
next_boundary = 1

[review]
minimum_grade = "A-"

[[signoffs]]
target = "ci"
stage = 0

[[signoffs]]
target = "regression"
stage = 1
resource_group = "mac-display"

[ruleset]
includes = ["RUST_CRATE"]
```

Operations in one stage are eligible concurrently. Later stages wait for the
current stage to pass. A resource group serializes matching operations across
Rapport processes and worktrees on one machine.

Generated request workflows do not run repository builds in GitHub. They ask
the local operator for exact-head proof. A current passing status is preserved;
otherwise the workflow publishes both its stable Context identity and aggregate
`Rapport Build` as pending.

Configure Rapport's dedicated target-branch ruleset without replacing unrelated
repository or organization protection:

```bash
rapport github setup
rapport github setup --confirm
rapport doctor
```

The setup proposal requires pull requests and `Rapport Build`, adds no duplicate
Review status or approval, enables squash merge and merged-branch deletion, and
uses no administrative bypass actor. `rapport doctor` remains read-only and
checks repository identity, authentication, status-publishing permission,
generated workflows, effective rules, freshness mode, squash merge, and branch
deletion.

## Repository Shape

- `context.toml` files define inherited repository policy.
- `.rapport/rules/**/*.toml` contains shared repository and installed catalog
  Rulesets.
- `.rapport/rules.lock` records exact catalog versions and digests.
- `.rapport/work.toml` and `.rapport/tasks/*.toml` contain ignored active local
  Work state.
- `.github/workflows/rapport-*-signoff-*.yml` contains generated proof-request
  workflows.
- Git is the local source of truth; GitHub is the shared source of truth.

Just remains the home for installs, builds, tests, generated assets, servers,
deploys, releases, and ecosystem-specific behavior. Rapport orders and proves
those conventions; it does not replace them.

## Development

This repository uses its checked-in Just workflow:

```bash
just check
just build
just test
just ci
```

When changing Rapport itself, use an installed or copied binary for dogfooding
instead of `cargo run -p rapport -- ...`, because `rapport build` may rebuild the
CLI executable.

## License

Licensed under either Apache-2.0 or MIT, at your option.
