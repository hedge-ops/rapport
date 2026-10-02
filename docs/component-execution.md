# Component execution (draft)

`rapport build <path>`, `rapport validate
<path>`, and `rapport generate <path> <output>` explicitly execute component
work. Inspect `rapport build --help` for the current CLI. Context inspection
and review prompt generation do not execute tools. Just may invoke these
commands; Rapport invokes underlying tools directly.

Generation uses `generated_inputs` and `generated_outputs` to order and
deduplicate dependencies. The producer's `execution.generators.<output>`
declares freshness inputs, expected outputs, and a typed adapter. Component
operations currently support Rust crates, Swift packages, and MCP UI.

## Built-in conventions

Facet and BoltFFI are built-in adapters. Their typed language/platform variants
reject unsupported combinations while loading declarations. Adapter-specific
fields are required only on the variant that uses them. Existing output metadata
must agree with the chosen built-in adapter.

```toml
[generated_outputs.swift_types]
tool = "facet_generate"
target = "swift"

[execution.generators.swift_types]
inputs = ["Cargo.toml", "Cargo.lock", "producer/**/*.rs"]
outputs = [{ path = "generated/swift" }]

[execution.generators.swift_types.adapter]
kind = "facet"
language = "swift" # swift, kotlin, or csharp
destination = "generated/swift"
```

Facet invokes the producer's `codegen` binary with the `codegen,facet_typegen`
features and selected language. The destination is repository-relative.

```toml
[generated_outputs.android_ffi]
tool = "boltffi_generate"
target = "android"

[execution.generators.android_ffi]
inputs = ["Cargo.toml", "Cargo.lock", "producer/**/*.rs", "producer/boltffi.toml"]
outputs = [{ path = "producer/target/packaged-android" }]

[execution.generators.android_ffi.adapter]
kind = "boltffi"
platform = "android" # apple or android
```

BoltFFI runs `boltffi pack <platform>` from the producer component. Declare the
actual outputs configured by that producer; this example path is illustrative.
Apple packaging requires macOS.

## Repository-owned generators

Locale catalogs, preview conventions, and project-specific generator packages
belong to the repository. A command adapter declares a nonempty sequence of
commands with explicit working directories and literal argument vectors.
Rapport does not interpret shell expressions or interpolate paths into arguments.
All `cwd`, input, output, candidate, and destination paths are repository-relative;
command arguments follow the invoked tool's own path rules.

For example, a repository with its own locale generator can declare:

```toml
[generated_outputs.preview_strings]
tool = "locale_strings_generate"
target = "swift"

[execution.generators.preview_strings]
inputs = ["Cargo.toml", "Cargo.lock", "tooling/generate-strings/**/*.rs", "app/core/strings/**/*.yml"]
outputs = [{ path = "generated/LocaleStrings.swift" }]

[execution.generators.preview_strings.adapter]
kind = "command"
commands = [
  { program = "cargo", args = ["run", "--package", "generate-strings", "--", "swift", "--locales", "app/core/strings", "--output", "generated/LocaleStrings.swift"], cwd = "." },
]
```

The `tool` and `target` metadata describe a custom output; they do not select
built-in behavior for a command adapter. Add formatting as a subsequent command
when needed. A direct `just` executable is rejected. Repositories must also keep
scripts called by Rapport independent of Just to preserve the dependency direction.
Commands must support `--version` because the prototype includes tool versions
in freshness receipts; arbitrary scripts without that interface are not yet
supported.

## Freshness and committed outputs

All adapters share the same dependency graph, content-based freshness receipts,
and failure handling. Inputs and outputs must be nonempty lists. Declare every
source/configuration dependency and any environment variable names under
`environment`. A failed generation cannot retain a reusable receipt.

For committed output, declare each candidate alongside its destination:

```toml
[execution.generators.html]
inputs = ["ui/src/**/*", "ui/package.json", "ui/bun.lock"]
outputs = [{ path = "ui/generated/app.html", candidate = "ui/bundle/app.html" }]
committed = true

[execution.generators.html.adapter]
kind = "command"
commands = [
  { program = "bun", args = ["install", "--frozen-lockfile"], cwd = "ui" },
  { program = "bun", args = ["run", "build"], cwd = "ui" },
]
```

Validation compares the
candidate to committed bytes; explicit `generate` copies the candidate to its
destination. Commands must write the candidate, not the committed file. Without
candidates for every committed output, validation requires an existing current
receipt instead of running the generator.

This draft replaces the initial prototype's destination/locales/package fields
and separate output/candidate lists. It has not been released. Native consumer
CI, complete adapter parity, and process timeout/cancellation remain unfinished.
