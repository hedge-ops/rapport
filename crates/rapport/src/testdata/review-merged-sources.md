# Component Review

Review the selected components and the changes supplied by the caller. Inspect the code before drawing conclusions. Use the architecture below to assess responsibilities and boundaries, and evaluate every applicable benchmark.

Report actionable findings with severity, code path and line, benchmark or architecture identifier, source context path, evidence, and a concrete correction. Distinguish verified issues from questions and state any review limitations. If no issues are found, say so. This prompt does not contain a diff or execute a review.

## Selected Paths by Context

- `TEAM` — other

## Effective Context

### `ROOT`

Purpose: Repository architecture.

Ownership — Prefer Here:


Boundaries — Avoid Here:


Source: `context.toml`

### `TEAM`

Purpose: Other component.

Ownership — Prefer Here:


Boundaries — Avoid Here:


Source: `other/context.toml`

## Applicable Shared Rulesets

- `TEAM` — Team standards. — source `.rapport/rules/custom/team-standards.toml`

## Review Benchmarks

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

Sources: `.rapport/rules/custom/team-standards.toml`, `other/context.toml`

