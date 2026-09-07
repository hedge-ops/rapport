# Component Review

Review the selected components and the changes supplied by the caller. Inspect the code before drawing conclusions. Use the architecture below to assess responsibilities and boundaries, and evaluate every applicable benchmark.

Report actionable findings with severity, code path and line, benchmark or architecture identifier, source context path, evidence, and a concrete correction. Distinguish verified issues from questions and state any review limitations. If no issues are found, say so. This prompt does not contain a diff or execute a review.

## Selected Paths by Context

- `ROOT` — .
- `SYNC` — app/core/workspace_sync

## Effective Context

### `ROOT`

Purpose: Repository architecture.

Ownership — Prefer Here:


Boundaries — Avoid Here:


Source: `context.toml`

### `SYNC`

Purpose: Coordinates encrypted synchronization and transfer state.

Ownership — Prefer Here:

- `SYNC_OWNERSHIP_001` — Owns synchronization scheduling and transfer coordination.

Boundaries — Avoid Here:

- `SYNC_BOUNDARY_001` — Document invariants and mutations belong to the domain component.

Source: `app/core/workspace_sync/context.toml`

Type: `crate`

## Applicable Shared Rulesets

- `TEAM` — Team standards. — source `.rapport/rules/custom/team-standards.toml`

## Review Benchmarks

### `SYNC_001`

Keep synchronization state transitions within this component.

Rationale: One owner preserves ordering and consistency.

Avoid (text):

```text
A UI handler directly updates synchronization checkpoints.
```

Prefer (text):

```text
The UI requests an operation; synchronization coordinates its state.
```

Sources: `app/core/workspace_sync/context.toml`

### `TEAM_001`

Preserve domain ownership.

Rationale: One owner preserves ordering and consistency.

Avoid (text):

```text
A UI handler directly updates synchronization checkpoints.
```

Prefer (text):

```text
The UI requests an operation; synchronization coordinates its state.
```

Sources: `.rapport/rules/custom/team-standards.toml`

