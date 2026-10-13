# Component Review

Review the selected components and the changes supplied by the caller. Inspect the code before drawing conclusions. Use the architecture below to assess responsibilities and boundaries, and evaluate every applicable benchmark.

Report actionable findings with severity, code path and line, benchmark or architecture identifier, source context path, evidence, and a concrete correction. Distinguish verified issues from questions and state any review limitations. If no issues are found, say so. This prompt does not contain a diff or execute a review.

## Selected Paths by Context

- `ROOT` — .

## Effective Context

### `ROOT`

Purpose: Repository architecture.

Ownership — Prefer Here:


Boundaries — Avoid Here:


Source: `context.toml`

## Applicable Shared Rulesets

- `BUILD` — Reproducible build commands. — source `.rapport/rules/build.toml`

## Review Benchmarks

### `BUILD_001`

Make CI run every required check.

Rationale: One recipe keeps local and automated validation consistent.

Avoid (just):

```just
ci:
    cargo build

```

Prefer (just):

```just
ci: check build test

check:
    cargo fmt --all -- --check
    cargo clippy --all --all-targets -- -D warnings

build:
    cargo build --all

test:
    cargo nextest run --workspace

```

Sources: `.rapport/rules/build.toml`

