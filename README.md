# Rapport

Let your agents have rapport with your repository.

Rapport helps you create structured architecture and review benchmarks for your
repositories. Describe component purpose, ownership, boundaries, and standards in
`context.toml`, then generate a complete review prompt for a human or agent.

```bash
cargo binstall rapport
rapport context init app/core/workspace_sync --namespace SYNC --type crate \
  --purpose 'Coordinates encrypted synchronization and transfer state.'
rapport context show app/core/workspace_sync
rapport context validate
rapport review app/core/workspace_sync > review.md
```

The directory must already exist. Paths are relative to the repository root.
`rapport review` defaults to `.`; pass multiple paths to review several components.
Provide the resulting Markdown and relevant code or diff to your reviewer.
Rapport prints a prompt; it does not invoke an agent or execute the review.
No Rapport Work, build, integration, GitHub authentication, or commit is required.

## Architecture and benchmarks

Repository-owned `context.toml` files are editable TOML. Use the context commands
for structured edits or author them directly:

```toml
type = "crate"
namespace = "SYNC"
purpose = "Coordinates encrypted synchronization and transfer state."

[ownership.SYNC_OWNERSHIP_001]
text = "Owns synchronization scheduling and transfer coordination."

[boundaries.SYNC_BOUNDARY_001]
text = "Document invariants and mutations belong to the domain component."

[ruleset]
includes = ["RUST_CRATE"]

[ruleset.rules.SYNC_001]
text = "Keep synchronization state transitions within this component."
rationale = "One owner preserves ordering and consistency."

[ruleset.rules.SYNC_001.avoid]
language = "text"
text = "A UI handler directly updates synchronization checkpoints."

[ruleset.rules.SYNC_001.prefer]
language = "text"
text = "The UI requests an operation; synchronization coordinates its state."
```

`namespace` is a stable uppercase identifier, with words separated by underscores.
Ownership and boundary IDs use `<NAMESPACE>_OWNERSHIP_001` and
`<NAMESPACE>_BOUNDARY_001`; rule IDs use `<NAMESPACE>_001` (three digits).
Namespaces must be unique across contexts. `purpose` and declaration text must
be nonempty. Each rule requires rationale and both examples.

Example language tags are preserved as Markdown fence labels. Supported tags are
`rust`, `swift`, `kotlin`, `csharp`, `xaml`, `html`, `javascript`, `typescript`,
`toml`, `json`, `yaml`, `markdown`, `shell`, and `text`.

`type` is an optional nonempty classification. It accepts `group`, `crate`,
`swift_package`, `apple_shell`, `cloudflare_worker`, `kustomize`,
`terraform_module`, `zola_site`, and other repository-defined classifications.
It describes architecture and does not select a build workflow.

### Component declarations

Contexts may also describe component composition and generated artifacts. All of
these fields are optional and default to empty collections:

```toml
components = [
  "app/apple/PeopleWorkKit",
  "app/capabilities/apple/PeopleWorkCapabilitiesKit",
]

kustomizations = [
  ".",
  "applications/people-work-api/overlays/production",
]

[generated_outputs.facet_swift]
tool = "facet_generate"
target = "swift"

[generated_inputs.app]
component = "app/core/shared"
output = "facet_swift"
```

`components` is explicit membership: it documents which repository-root-relative
component paths make up a context and never expands `rapport review` selection.
`generated_outputs` declares named producer capabilities, while
`generated_inputs` declares direct consumer edges to a producer context and one
of that context's outputs. These declarations are direct to their owning
context; they are not inherited by child contexts. `kustomizations` contains
paths relative to the declaring context directory. Rapport validates and renders
these declarations and their source paths, but never runs the declared tools or
targets.

Install referenced catalog packs before adding includes:

```bash
rapport ruleset catalog list
rapport ruleset catalog install RUST_CRATE
rapport ruleset catalog install CRUX_APP
rapport context ruleset compose add app/core/workspace_sync --ruleset RUST_CRATE
```

Use `rapport ruleset` to create repository-owned packs under `.rapport/rules/`.
Catalog installations and `.rapport/rules.lock` pin pack versions and digests.
Commit those files alongside your context. `rapport init` records review guidance
in `AGENTS.md` and configures ignores without creating a GitHub workflow.

## Resolution and errors

For each selected file or directory, Rapport includes every governing ancestor
`context.toml`, from the root to the nearest component. Architecture accumulates;
a child does not replace parent ownership, boundaries, or standards. Selecting a
parent does not recursively select its children: pass affected component paths
explicitly. Paths may refer to deleted files so their surviving ancestor context
can still govern a change.

Included packs resolve transitively. A pack included by several ancestors or
through several packs contributes each standard once. Identical standards with
the same ID merge their source references; different definitions sharing an ID
are errors. There is no last-writer-wins override. Prompts include purpose,
ownership, boundaries, component declarations, generated dependency provenance,
full benchmark text, rationale, examples, and actual source paths, including
packs stored at nonstandard filenames.

Rapport validates all discovered contexts and installed packs. Unknown fields,
unresolved includes, include cycles, duplicate namespaces, invalid IDs, invalid
repository-relative paths, missing generated producers or outputs, generated
dependency cycles, incompatible versions, and conflicting applicable standards
fail explicitly. It emits no partial review prompt on failure. Correct the named
declaration or install the missing pack; standards are never silently dropped. A
missing governing context reports how to create one.

## Compatibility and scope

The namespace format above requires no version, counters, or lifecycle state.
Legacy `[review]` and `[[signoffs]]` fields are rejected with migration guidance;
move acceptance thresholds and build commands into repository tooling. Existing version-1 contexts using `id` remain supported,
including their `<ID>_RULE_001` rule IDs. Do not mix `id` and `namespace` in one
file. Converting to `namespace` also changes the rule prefix to `<NAMESPACE>_001`.
Context commands preserve the identity format and component type when editing;
allocation counters are inferred when absent and written on mutation.
For compatibility, `context init` without `--namespace` still creates the `id` format.
Unsupported legacy array declarations still require explicit migration.

Planning, coding, tests, builds, and integration belong to each repository's own
process. Lifecycle commands have been removed. See the
[migration guide](docs/lifecycle-migration.md) for the removed commands and fields.

`rapport context validate [path]` validates all discovered schemas, namespaces,
includes, and ownership references, then checks effective benchmark conflicts for
each component at or below the selected path. Without a path it checks all
components. It reports errors without changing files or executing repository tools.

## Development

```bash
just check
just build
just test
just ci
```

When changing Rapport itself, use an installed or copied binary for context
validation and review prompts, and `just ci` for repository validation.

## License

Licensed under either Apache-2.0 or MIT, at your option.
