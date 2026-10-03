# Migrating to architecture and reviews

Rapport's commands are `review`, `context`, `ruleset`, `init`, and `prime`.
Repository tools own planning, code changes, tests, builds, commits, and merges.
This is a breaking scope reduction from Rapport 0.6.x.

## Removed commands

| Previous command | Replacement |
| --- | --- |
| `work`, `develop` | Your issue tracker, agent, and Git workflow |
| `build` | Your repository's build/test commands and CI |
| `integrate`, `github`, root `doctor` | Git, GitHub, and repository CI configuration |
| `review start`, `complete`, `reconcile`, `override`, `cancel`, `status` | `review [PATH ...]` emits a prompt for your reviewer |
| `context review`, `context signoff` | Acceptance policy and build configuration in repository tools |
| `context doctor` | `context validate [PATH]` checks architecture and benchmarks |

Review has no subcommands. `rapport review start` now means reviewing the path
named `start`. Old options such as `--result` and `--minimum-grade` are unsupported.
Review neither executes an agent nor records an acceptance decision.

## Context files

Remove `[review]` and `[[signoffs]]` declarations from every `context.toml`.
Rapport explicitly rejects those fields rather than silently ignoring their
previous meaning. Move any required build targets or acceptance thresholds into
your repository's own CI and review policy.

Architecture, ownership, boundaries, and benchmark declarations remain supported.
The version-1 `id` format and the `namespace` format are both readable; no
architecture migration is required. Run `rapport context validate` after editing.
Validation checks discovered contexts and installed packs; selected components
are checked for conflicting effective standards. It never executes a build or
inspects generated workflows.

## Generated dependencies

Remove all `[generated_inputs.*]` and `[generated_outputs.*]` tables, including
empty `generated_inputs` and `generated_outputs` declarations, from every
consumer `context.toml`. These fields are rejected as unknown schema fields.
Rapport no longer resolves producers or outputs, checks generated dependency
cycles, or includes generated dependencies in context and review output.

Maintain generation and validation prerequisites in repository-owned Just
recipes or native build systems. Ensure consuming build/test recipes depend on
their generators, and update CI change filters to run them when producer inputs
change. Run the repository's Just CI workflow to verify those prerequisites,
then run `rapport context validate` to check the remaining declarations.

Component membership, kustomization paths, architecture, standards, and stateless
review prompts remain supported. Context inspection, validation, and review stay
read-only; they never execute builds, generators, or Just.

## Existing repositories

Finish or export active Work using an older binary before upgrading if you still
need that history. This version does not read, modify, migrate, or delete Work,
Task, or global history files. They remain available to older versions.

Inspect your generated `rapport-*-signoff-*.yml` workflows and the shared
`rapport-signoff.yml` workflow, then replace their proof-request behavior with your
own CI. Review any remote required checks or settings you previously configured.
Rapport performs no automatic workflow deletion or remote configuration changes
in consuming repositories. `rapport init` only updates agent guidance and ignores.

For library consumers, the CLI crate's embedding entry point is now
`run(argv, out, err)`; clock, external-command runner, and lifecycle path APIs have
been removed. The standalone utility crates are unaffected.
