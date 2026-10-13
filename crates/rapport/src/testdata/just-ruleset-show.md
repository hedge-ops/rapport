# rapport ruleset show --rule

- `id` — BUILD_001
- `owning ruleset` — BUILD
- `reference` — none

Make CI run every required check.

## Rationale

One recipe keeps local and automated validation consistent.

## Avoid

```just
ci:
    cargo build

```

## Prefer

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
