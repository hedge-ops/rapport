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

`rapport` currently runs cargo lifecycle commands in the directory you pass:

```text
rapport fix <path>       # cargo fmt
rapport lint <path>      # cargo fmt -- --check; cargo clippy --all-targets -- -D warnings
rapport build <path>     # cargo check
rapport test <path>      # cargo test
rapport validate <path>  # lint + build + test
rapport audit <path>     # validate + cargo build --release + cargo doc --no-deps
```

Project discovery is not implemented yet. For this checkpoint, `rapport`
validates that the path exists and assumes cargo for the command runner.

## Testing

See [TESTING.md](TESTING.md) for the local test commands, Cargo end-to-end
fixture and snapshot layout, snapshot stability rules, and GitHub Actions
coverage.

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
- [ ] project discovery from local markers such as `Cargo.toml`

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
