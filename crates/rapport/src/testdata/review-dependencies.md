# Component Review

Review the selected components and the changes supplied by the caller. Inspect the code before drawing conclusions. Use the architecture below to assess responsibilities and boundaries, and evaluate every applicable benchmark.

Report actionable findings with severity, code path and line, benchmark or architecture identifier, source context path, evidence, and a concrete correction. Distinguish verified issues from questions and state any review limitations. If no issues are found, say so. This prompt does not contain a diff or execute a review.

## Selected Paths by Context

- `VIEW` — app/core/view

## Effective Context

### `ROOT`

Purpose: Repository architecture.

Ownership — Prefer Here:


Boundaries — Avoid Here:


Source: `context.toml`


### Component Membership

- `app/core/shared`
- `app/core/view`
### `VIEW`

Purpose: Owns the view.

Ownership — Prefer Here:


Boundaries — Avoid Here:


Source: `app/core/view/context.toml`


### Generated Inputs

- `app` — component `app/core/shared`, output `facet_swift` — producer source `app/core/shared/context.toml`

### Kustomizations

Paths are relative to `app/core/view`.

- `.`
## Applicable Shared Rulesets

## Review Benchmarks
