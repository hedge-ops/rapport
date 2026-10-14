# Component Review

Review the selected components and the changes supplied by the caller. Inspect the code before drawing conclusions. Use the architecture below to assess responsibilities and boundaries, and evaluate every applicable benchmark.

Report actionable findings with severity, code path and line, benchmark or architecture identifier, source context path, evidence, and a concrete correction. Distinguish verified issues from questions and state any review limitations. If no issues are found, say so. This prompt does not contain a diff or execute a review.

## Selected Paths by Context

- `ROOT` — .

## Effective Context

### `ROOT`

Purpose: Repository architecture.

Ownership — Prefer Here:


Boundaries — Avoid Here:


Source: `context.toml`

## Applicable Shared Rulesets

- `CHEF_TESTING` — Chef directory standards. — source `.rapport/rules/chef/testing.toml`

## Review Benchmarks

### `CHEF_TESTING_001`

Declare directory permissions explicitly.

Rationale: Explicit permissions keep Chef directory resources reproducible.

Avoid (ruby):

```ruby
directory '/var/lib/people-work' do
  owner 'root'
end

```

Prefer (ruby):

```ruby
directory '/var/lib/people-work' do
  owner 'root'
  mode '0700'
end

```

Sources: `.rapport/rules/chef/testing.toml`

