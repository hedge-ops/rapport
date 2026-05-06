# Rapport

Ergonomic, human-driven, agent-friendly approach to building, based on
real-world experience of building [People Work](https://www.people-work.io).

## Vision

Currently internally I have a `builder` cli that will build anything in my
internal repository. This ports that and makes it better:

- Standardize common lifecycle verbs like `fix`, `lint`, `build`, `test`,
  `validate`, and `audit` - so humans and agents have one thing to call
  anywhere in the repository, to simplify how things are done.
- Make output more human and agent-friendly (less tokens) - if it's successful,
  we say so, if it fails, we curate a list of things to fix. We don't spam
  anyone with a huge cli output, that's so lame (and token-intensive too).
- Listen for file changes in the repository and smartly prioritize testing and
  compiling, then format and building. Start on the thing before the agent asks
  for it, speeding up development.

## Current checkpoint

`rapport` currently discovers Cargo, Bun, SwiftPM, Fastlane, Gradle, Kustomize,
and Terraform projects from the path you pass, walking upward to the nearest
supported project marker.

```text
rapport fix <path>       # cargo fmt
rapport lint <path>      # cargo fmt -- --check; cargo clippy --all-targets -- -D warnings
rapport build <path>     # cargo check
rapport test <path>      # cargo test
rapport validate <path>  # lint + build + test
rapport audit <path>     # validate + cargo build --release + cargo doc --no-deps
```

`rapport` walks upward from the path you pass until the git root, then runs the
nearest supported project it finds. Cargo projects are detected by
`Cargo.toml`; SwiftPM projects are detected by `Package.swift` with a leading
`// swift-tools-version:` declaration.

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

```text
rapport fix <path>       # swift format format --in-place ...
rapport lint <path>      # swift format lint --strict ...
rapport build <path>     # swift build
rapport test <path>      # swift test
rapport validate <path>  # lint + build + test
rapport audit <path>     # validate + swift build -c release
```

SwiftPM formatting uses `swift format` first and falls back to `swift-format`
when installed separately. Build and test do not require formatter tooling.

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
`crates/rapport/tests/fixtures`. SwiftPM, Fastlane, and Kustomize e2e cases use
generated fake toolchains so the suite does not require Swift, Ruby, Bundler,
Fastlane, Xcode, kubectl, Kustomize, kubeconform, or a Kubernetes cluster on the
host. Python tooling for the harness is managed with uv through `pyproject.toml`
and `uv.lock`. The old `just acceptance` command remains as a compatibility
alias for `just e2e`; `just tests/cargo/acceptance` runs only the Cargo subset.

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
