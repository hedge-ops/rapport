# Rapport

Let your agents have rapport with your repository.

Rapport is a repository workflow layer for human-directed agent work. It keeps
active work grounded in repository-owned rules, build conventions, GitHub
integration, and local state.

The first delivery loop is intentionally tight:

```text
work -> build -> integrate
```

The goal is not to invent another build system. Rapport sits on top of the
durable tools and conventions a repository already owns: checked-in rules,
conventional Just targets, Git/GitHub, local `.rapport` work state, and command
telemetry.

## Install

Install the latest released binary with:

```bash
cargo binstall rapport
```

## Inner Loop

Rapport's first responsibility is the inner development loop:

- `work` creates and reports active local work context: title, ticket,
  objective, paths, stage, and applicable rules.
- `build` validates that active work by running existing repository Just
  conventions.
- `integrate` turns local work into durable Git/GitHub state: commit, PR,
  signoff status, and remaining action.

The intended command surface is:

```text
rapport work start
rapport work status
rapport work add path <path>
rapport work rules list
rapport work rules show <id>
rapport build [path...]
rapport integrate --summary "..." --message "..."
rapport integrate # retry signoff for the recorded PR
```

## Repository Shape

Rapport should work with repository-owned conventions rather than replacing
them:

- checked-in shared rules in `rules/*.toml`;
- folder-collocated owner rules in `**/rules.toml`;
- conventional Just targets for project-specific validation;
- Git and GitHub for commits, pull requests, and status checks;
- inherited `signoffs` in folder `context.toml` files, fulfilled by matching
  GitHub Actions workflows;
- ignored local state in `.rapport/work.toml`;
- append-only local telemetry in `.rapport/events.jsonl`.

Just remains the right home for installs, local servers, generated assets,
bespoke checks, deploys, release tasks, and ecosystem-specific details. Rapport
uses those conventions to keep agents oriented inside the current work.

Folder contexts declare integration needs close to the code they govern:

```toml
version = 1
purpose = "Owns the Apple application."
signoffs = ["ci"]
```

Manage targets through Rapport so their GitHub request workflows stay aligned:

```text
rapport context signoff add app/apple ci
rapport context signoff remove app/apple ci
rapport context signoff repair app/apple ci
```

Adding `ci` to `app/apple/context.toml` generates the exact Rapport-owned
`.github/workflows/rapport-app-apple-ci.yml` request workflow. On matching pull
requests it calls the shared `.github/workflows/rapport-signoff.yml` workflow,
which posts `signoff: app-apple-ci` as pending for the PR head SHA. It does not
run repository validation in GitHub; `rapport integrate` runs `just ci` from
the declaring folder on the local host and posts the SHA-bound result.

`rapport doctor` compares generated workflows byte-for-byte with their context
declarations. `rapport integrate` runs the same validation before committing or
opening a PR and unions inherited targets for every active work path. Once the
PR exists, integration records its branch, commit, URL, and pending signoffs in
current work before attempting proof. It rejects missing or unexpected signoff
statuses, runs each requested Just target locally, and posts success or failure
to the exact PR head SHA. A failed attempt leaves the PR recorded, so a bare
`rapport integrate` retries signoff on that same PR without creating another.

## Later Phases

The outer factory phases are separate from the first loop:

- `plan` comes before active local work and ensures durable tickets/plans exist.
- `ship` comes after integration and handles release, deploy, and completion
  workflows.

Those phases are intentionally outside the first implementation slice.

## Building Blocks

Keeper crates:

- `rapport-prose` - markdown-ish output primitives;
- `rapport-temporal` - date, time, and recurrence primitives.
- `rapport-files` - fake/real filesystem support for testable workflow code.

## Development

Local checks are driven through the repository `justfile`:

```bash
just check
just build
just test
just ci
```

Command-level coverage should grow around the new `work -> build -> integrate`
surface as it lands.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <http://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.
