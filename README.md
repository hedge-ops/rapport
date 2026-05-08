# Rapport

Ergonomic, human-driven, agent-friendly approach to running conventional
development lifecycle checks across mixed repositories, based on real-world
experience building [People Work](https://www.people-work.io).

## Vision

- Standardize common lifecycle verbs like `fix`, `lint`, `build`, `test`,
  `validate`, and `audit` - so humans and agents have one thing to call
  anywhere in a repository, to simplify how things are done.
- Make output more human and agent-friendly (less tokens) - if it's successful,
  we say so, if it fails, we curate a list of things to fix. We don't spam
  anyone with a huge cli output, that's so lame (and token-intensive too).
- Listen for file changes in the repository and smartly prioritize testing and
  compiling, then format and building. Start on the thing before the agent asks
  for it, speeding up development.

## Current checkpoint

`rapport` currently discovers Cargo, Zola, Bun, SwiftPM, Fastlane, Android app,
Gradle, Kustomize, and Terraform projects from the path you pass. The path is
treated as a directory scope: if runnable targets exist under that directory,
rapport discovers them recursively and runs the standard lifecycle for each
target. If the path is merely inside a concrete project, rapport preserves the
nearest parent project behavior.

Rapport answers the conventional dev-cycle question: "is everything under this
scope still good?" It is not a general Just replacement. Justfiles remain the
right place for installs, operations, deploys, local servers, dependency
updates, and bespoke workflows; those workflows can call rapport when they need
the standard lifecycle answer.

`rapport doctor <path>` is the preflight and troubleshooting command for that
same target set. It resolves the targets under the requested path, then reports
which tools, probes, marker files, scripts, lanes, and conventions are ready for
rapport to run. Doctor uses lightweight version/probe commands and configuration
inspection only; it does not run lifecycle work such as build, test, lint, fix,
validate, or audit.

`rapport prime <path>` is the agent bootstrap command for the same target set.
Tell agents to call `rapport prime` when they start in an unfamiliar scope, or
put it in a hook before the agent begins editing. Prime does not probe tools or
run lifecycle work; it explains how to drive rapport, which targets were
detected, their expected files/scripts/tasks/lanes or configuration, and the
boundary between rapport and project-specific task runners. Agents should use
`doctor` after prime when they need to know whether the detected targets are
runnable right now.

```text
rapport prime <path>     # agent bootstrap: how to use rapport for this scope
rapport doctor <path>    # check readiness without running lifecycle work
rapport fix <path>       # cargo fmt --all, or cargo fmt --package <name>
rapport lint <path>      # cargo fmt check; strict cargo clippy --all-targets -- -D warnings
rapport build <path>     # cargo check --workspace, or cargo check --package <name>
rapport test <path>      # cargo nextest run when available, otherwise cargo test
rapport validate <path>  # lint + build + test
rapport audit <path>     # validate + cargo build --release + cargo doc --no-deps
```

`rapport <verb> <path>` handles concrete project roots, child directories inside
projects, umbrella directories with multiple child projects, and aggregate
container directories with runnable children but no marker of their own.
Same-ecosystem descendants are de-duplicated when an ancestor target covers
them, such as a Cargo workspace root covering member crates. Different
ecosystems are additive under the same scope. Generated, dependency, cache, and
build output directories such as `.git`, `target`, `node_modules`, `.build`,
`DerivedData`, `.terraform`, `dist`, `build`, and `coverage` are skipped during
recursive discovery.

Cargo projects are detected by `Cargo.toml`; SwiftPM projects are detected by
`Package.swift` with a leading `// swift-tools-version:` declaration.

Cargo scopes are intentionally deterministic. A Cargo workspace root runs once
with workspace-wide flags such as `--workspace` for compile/test work and
`--all` for formatting, so workspace members are not duplicated. A package root,
including a workspace member invoked directly or from one of its child
directories, runs package-scoped with `--package <name>`. Umbrella directories
that contain multiple independent Cargo projects run each child target once.

Cargo testing is nextest-first: rapport probes `cargo nextest --version` and
runs `cargo nextest run` when it is usable. If `cargo-nextest` is missing or the
probe fails, lifecycle runs fall back to `cargo test`. `rapport doctor` reports
`cargo nextest` as a warning rather than a failure because the fallback is part
of the convention.

Cargo linting is strict and read-only: formatting is checked first, then clippy
runs with `--all-targets -- -D warnings` using the same workspace or package
scope as the rest of the lifecycle. `build` remains the fastest useful compile
proof (`cargo check`); `audit` is slower release confidence (`validate`, then a
release build and docs without dependency docs).

Packages or workspaces whose normal dev cycle requires Cargo feature or target
flags can declare the narrow rapport Cargo metadata surface in `Cargo.toml`:

```toml
[package.metadata.rapport.cargo]
features = ["extra"]
no-default-features = true
target = "wasm32-unknown-unknown"
```

Use `[workspace.metadata.rapport.cargo]` for flags that apply to a whole
workspace. `features`, `all-features`, `no-default-features`, and `target` are
supported for compile, lint, test, release, and docs phases. Per-member flag
differences during a single workspace-root run are out of scope for v1; invoke
the member package path when a package needs package-specific flags.

Lifecycle verbs keep their conventional scope:

- `build` is the fastest meaningful proof that the target builds.
- `audit` is slower release or artifact confidence.
- `validate` is the pre-commit path: lint, build, and test where the convention
  supports those phases.

Bun projects are detected by `package.json` plus `bun.lock` or `bun.lockb` at
the package root or an ancestor workspace root. A Bun package runs standard
scripts directly; `validate` is composed as lint + build + test. A scriptless
Bun workspace root with runnable child packages acts as an aggregate scope.

```text
rapport fix <path>       # bun run fix
rapport lint <path>      # bun run lint
rapport build <path>     # bun run build
rapport test <path>      # bun run test
rapport validate <path>  # bun run lint + bun run build + bun run test
rapport audit <path>     # bun run audit
```

Zola projects are detected by `config.toml` with `base_url` and a recognized
Zola section, plus `content/` and `templates/` directories. A colocated Bun
package, or a package inside an ancestor Bun workspace, is treated as the
site's asset/check pipeline rather than as a duplicate target:

```text
rapport fix <path>       # bun run fix, or no-op if no Bun fix script exists
rapport lint <path>      # bun run lint/check when present; zola check
rapport build <path>     # bun run build when present; zola build
rapport test <path>      # bun run test when present; zola check
rapport validate <path>  # lint + build + optional Bun test without duplicate zola check
rapport audit <path>     # validate + production/non-draft zola build
```

```text
rapport fix <path>       # configured formatter in write mode
rapport lint <path>      # configured formatter check + configured SwiftLint
rapport build <path>     # swift build
rapport test <path>      # swift test
rapport validate <path>  # lint + build + test
rapport audit <path>     # validate + swift build -c release
```

SwiftPM style tooling is config-driven. A root `.swift-format` uses Swift's
formatter, resolved as `swift format` first and then standalone `swift-format`;
a root `.swiftformat` uses SwiftFormat through `swiftformat`. A root
`.swiftlint.yml` or `.swiftlint.yaml` runs `swiftlint lint --strict --config`.
Missing configured tools fail with install hints. Build and test do not require
formatter or linter tooling when those configs are absent.

Fastlane projects are detected by `fastlane/Fastfile`. Rapport requires a
`Gemfile` and standard lanes named `fix`, `lint`, `build`, `test`, `validate`,
and `audit`; each verb runs through Bundler:

```text
rapport fix <path>       # bundle exec fastlane fix
rapport lint <path>      # bundle exec fastlane lint
rapport build <path>     # bundle exec fastlane build
rapport test <path>      # bundle exec fastlane test
rapport validate <path>  # bundle exec fastlane validate
rapport audit <path>     # bundle exec fastlane audit
```

Xcode app projects follow the Fastlane convention: keep the `.xcworkspace` or
`.xcodeproj` alongside `fastlane/Fastfile`, and let the standard lanes wrap the
project-specific `xcodebuild`, lint, formatting, and release steps.

Android app projects are detected at a Gradle root with `settings.gradle.kts` or
`settings.gradle`, a checked-in `./gradlew`, and at least one root or included
module that applies the `com.android.application` plugin. Android app discovery
runs before generic Gradle discovery so app projects get Android-specific
variant tasks while non-Android Gradle projects keep the generic Gradle
convention.

```text
rapport fix <path>       # :app:ktlintFormat when ktlint is configured; otherwise no-op
rapport lint <path>      # configured ktlintCheck + configured detekt + Android lint for dev variant
rapport build <path>     # assemble the dev variant
rapport test <path>      # JVM unit tests for the dev variant
rapport validate <path>  # lint + build + test in one Gradle invocation
rapport audit <path>     # validate + release bundle confidence
```

The dev variant is `LocalDebug` when an app module declares a `local` product
flavor; otherwise it is `Debug`. Audit bundles `ProductionRelease` when a
`production` product flavor exists; otherwise it bundles `Release`. Multiple app
modules run once each in sorted Gradle module-path order. Android library
modules are not separate rapport targets unless they are covered by an app
module's Gradle task graph.

Ktlint and detekt are optional and discovered from app module Gradle plugin
configuration. If ktlint is configured, `fix` runs `ktlintFormat` and `lint`
runs `ktlintCheck`; if detekt is configured, `lint` also runs `detekt`. Android
lint is required for every app module through the selected dev variant task.

Generated source and generated resource prerequisites belong in Gradle: wire
custom code generation into the Android task graph so `lint<Variant>`,
`assemble<Variant>`, `test<Variant>UnitTest`, and `bundle<Variant>` have the
inputs they need. Rapport does not call project-specific Just recipes for
codegen, installs, emulator management, signing setup, local servers, or deploys;
those remain in Just or other project tooling and may call rapport for the
standard lifecycle answer.

Kustomize targets are detected by `kustomization.yaml` or `kustomization.yml`.
When you pass an umbrella directory without its own marker, rapport recursively
runs child Kustomize targets beneath it. The initial Kubernetes runner is
offline-only: it renders manifests and performs static validation, but never
runs `kubectl apply`, prunes resources, or depends on a live cluster.
This is shaped for platform-style directories that aggregate services,
observability, ingress, and application overlays through a top-level
`kustomization.yaml`.

```text
rapport fix <path>       # no-op; Kustomize has no autofix
rapport lint <path>      # render, then kubeconform -strict -summary -ignore-missing-schemas -
rapport build <path>     # kustomize build . or kubectl kustomize .
rapport test <path>      # no-op; no Kubernetes tests configured
rapport validate <path>  # lint + build + test
rapport audit <path>     # validate
```

Rendering prefers standalone `kustomize build .` and falls back to
`kubectl kustomize .`. Static validation uses `kubeconform` against the
rendered manifest stream.

CLI end-to-end fixtures run outside Cargo's test runner so `rapport` can invoke
Cargo projects without nesting Cargo inside `cargo test`. The e2e target builds
`rapport` once, copies each fixture into a temporary directory, and compares
normalized command-session snapshots:

```text
just e2e
```

Cases live under `tests/e2e/cases`, snapshots live under
`tests/e2e/snapshots`, and project fixtures live under
`crates/rapport/tests/fixtures`. Android, SwiftPM, Fastlane, and Kustomize e2e
cases use generated fake toolchains so the suite does not require the Android
SDK, Swift, Ruby, Bundler, Fastlane, Xcode, kubectl, Kustomize, kubeconform, or
a Kubernetes cluster on the host. Python tooling for the harness is managed with
uv through `pyproject.toml` and `uv.lock`. The old `just acceptance` command
remains as a compatibility alias for `just e2e`; `just tests/cargo/acceptance`
runs only the Cargo subset.

## Testing

See [TESTING.md](TESTING.md) for local test commands, e2e fixture and snapshot
layout, snapshot stability rules, and GitHub Actions coverage.

## Principles

- Human-driven - the best way to design for agents, is ergonomic,
  out-of-the-box thinking for agents. Every line of code is lovingly crafted to
  make the agent's life easier. Thus, it doesn't matter that we go slower here
  because the benefits we get from _doing it right_ will pay dividends when we
  engage in more agent-heavy development.
- Opinionated Slop reducer - human-driven approaches adapt to human needs to be
  different, while agents want to conform! We deliver an opinionated approach to
  reduce the amount an agent needs to do the work of reporting on what's going on.
  With minimal training, the agent should be able to use tools created here. In
  a sense, this approach is agent-native, unlike approaches that come before it.

## Current Steps

The initial crate scaffolding is in place:

- [x] `rapport-temporal` - date-friendly and recurrence-friendly primitives
- [x] `rapport-prose` - markdown-ish output using the builder pattern
- [x] `rapport-cli` - typed CLI parsing primitives
- [x] `rapport` - first runnable cargo lifecycle CLI
- [x] project discovery from local markers such as `Cargo.toml`, `Package.swift`,
  `fastlane/Fastfile`, and `kustomization.yaml`

## Outstanding Questions

- [x] Double check the licensing is proper
- [x] How to set up this repository for multiple crates
- [x] How to publish to crates.io
- [ ] How much project discovery belongs in `rapport` before a daemon/watch mode

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <http://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.
