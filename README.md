# Rapport

Ergonomic, human-driven, agent-friendly approach to building, based on
real-world experience of building [People Work](https://www.people-work.io).

## Vision

Currently internally I have a `builder` cli that will build anything in my
internal repository. This ports that and makes it better:

- Standardize across languages common patterns of `format`, `check`, `test`,
  `build`, `dev`, and `ci` - so humans and agents have one thing to call
  anywhere in the repository, to simplify how things are done.
- Make output more human and agent-friendly (less tokens) - if it's successful,
  we say so, if it fails, we curate a list of things to fix. We don't spam
  anyone with a huge cli output, that's so lame (and token-intensive too).
- Listen for file changes in the repository and smartly prioritize testing and
  compiling, then format and building. Start on the thing before the agent asks
  for it, speeding up development.

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

I'm going to migrate my internal crates to `rapport_` crates, as a scaffolding
for the `rapport` cli:

- [ ] `rapport_dates` - or potentially `rapport_temporal` a date-friendly and
  recursion-friendly crate
- [ ] `rapport_prose` - or potentially `rapport_output` for markdown output
  using the `Builder` pattern
- [ ] `rapport_cli` - how can I wrap `clap` to build a pit of success around
  creating command-line tools with rust, for agents? There are principles I
  have internally, so this would be a little more work.

## Oustanding Questions

- [ ] Double check the licensing is proper
- [ ] How to set up this repository for multiple crates
- [ ] How to publish to crates.io
