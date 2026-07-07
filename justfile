# default target for local development
default: dev

# --------------------------------------------------------------------------
# Development
# --------------------------------------------------------------------------

# removes build artifacts
clean:
    @echo '{{ style("command") }}clean:{{ NORMAL }}'
    rm -rf ./target
    cargo clean

# builds all crates
build:
    @echo '{{ style("command") }}build:{{ NORMAL }}'
    cargo build --all

# builds the rust in release mode
build-release:
    @echo '{{ style("command") }}build-release:{{ NORMAL }}'
    cargo build --release --all

# runs tests
test:
    @echo '{{ style("command") }}test:{{ NORMAL }}'
    cargo nextest run --workspace

# runs tests with coverage report
cover:
    @echo '{{ style("command") }}cover:{{ NORMAL }}'
    cargo llvm-cov nextest --workspace --open

# auto-fix formatting issues
fix:
    @echo '{{ style("command") }}fix:{{ NORMAL }}'
    cargo fmt --all

# validate formatting and lint (strict, no auto-fix)
check:
    @echo '{{ style("command") }}check:{{ NORMAL }}'
    cargo fmt --all -- --check
    cargo clippy --all --all-targets -- -D warnings -A clippy::empty_line_after_doc_comments

# find unused dependencies (requires nightly)
check-deps:
    @echo '{{ style("command") }}check-deps:{{ NORMAL }}'
    rustup run nightly cargo udeps --workspace --all-targets

# local development: fix, check, build, test
dev: fix check build test

# CI pipeline: check, build, test
ci: check build test

# Update Cargo dependencies and test
update-deps:
    @echo '{{ style("command") }}update-deps:{{ NORMAL }}'
    cargo update
    just ci

# --------------------------------------------------------------------------
# Publishing
# --------------------------------------------------------------------------

# dry-run publish for a single crate (e.g. just publish-dry rapport-temporal)
publish-dry crate:
    @echo '{{ style("command") }}publish-dry {{crate}}:{{ NORMAL }}'
    cargo publish --dry-run -p {{crate}}

# publish a single crate (requires CARGO_REGISTRY_TOKEN)
publish crate:
    @echo '{{ style("command") }}publish {{crate}}:{{ NORMAL }}'
    cargo publish -p {{crate}}
