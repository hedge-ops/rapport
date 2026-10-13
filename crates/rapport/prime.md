# rapport prime

## Purpose

- Rapport structures repository architecture and review benchmarks in context.toml.
- Generate sourced review prompts for humans and agents.

## Review

- `rapport context init <path> --purpose <text>` - create architecture context for a repository area
- `rapport context show <path>` - inspect architecture, direct declarations, and inherited standards
- `rapport ruleset catalog list` - discover reusable standards packs
- `rapport ruleset catalog install <ID>` - install a standards pack before including it
- `rapport context affected --pending` - select components owning staged, unstaged, and non-ignored untracked changes
- `rapport context affected --base <ref> [--pending]` - select branch changes since the merge base, optionally combined with pending changes
- `rapport context affected --base <base-sha> --head <head-sha> --json` - select components with revision and policy provenance for automation
- `rapport review <path> [<path> ...]` - print a complete Markdown review prompt with source paths
- Pass the prompt and relevant code or diff to your reviewer.

## Repository ownership

- Edit context.toml directly or use context commands; commit architecture and standards with the repository.
- Ancestor context and included packs apply alongside local declarations. Conflicting or missing standards fail explicitly.
- `rapport context validate [path]` checks architecture and effective standards.

## Authoring rules

This is the authoritative authoring guidance for both shared repository rulesets
under `.rapport/rules/` and rules owned by component `context.toml` files. Apply
it when adding, updating, or directly editing a rule.

Write like a senior engineer explaining a recommendation to a junior engineer,
adopting the style of the Effective C#, Java, Rust, and TypeScript books. These
books are references for the teaching style, not sources of rules to copy.
See the [Effective Rust item titles](https://effective-rust.com/types.html),
the [type-system explanation and examples](https://effective-rust.com/use-types.html),
and the [discussion of generics and trait objects](https://effective-rust.com/generics.html)
for examples of concise recommendations, practical teaching, and tradeoffs.
Author original recommendations, rationale, and examples for the repository:

- `text` is a concise, specific engineering recommendation in natural language.
  Choose the verb that expresses the judgment: use, prefer, avoid, consider,
  minimize, or another suitable verb. Reserve always and never for justified
  invariants. Put the explanation in `rationale`; do not impose a fixed verb
  template or word count, or weaken a real invariant to fit a style.
- `rationale` is a short explanatory paragraph that teaches why the
  recommendation matters. Include the failure it prevents, its scope, or its
  tradeoffs where useful, rather than mechanically covering a checklist.
  Teach judgment the reader can apply in another situation; do not just repeat
  the title.
- `avoid` and `prefer` are paired, concrete examples in the target language,
  demonstrating the same lesson. Show why the discouraged approach causes
  trouble in the stated context and how the recommended approach helps; an
  alternative need not be universally wrong. Keep the scenarios comparable
  and show actual code rather than describing what code should do.
  For tools or configuration, use the relevant shell, TOML, YAML, or other syntax
  rather than vague prose. Set each `language` to the example's actual syntax.
- Keep the tone direct, patient, and practical. Explain one actionable lesson
  per rule; avoid slogans and unrelated improvements in the examples.

For example, a complete repository-owned ruleset at `.rapport/rules/api.toml`:

```toml
version = 1
id = "API"
purpose = "Error handling at Rust API boundaries."
includes = []

[rules.API_001]
text = "Prefer typed errors at reusable API boundaries."
rationale = "Callers parsing a port may need to distinguish malformed digits from an out-of-range value so they can explain how to correct the input. Converting the parse failure to a string discards its structured cause and encourages callers to parse messages. Preserve that cause in a domain error when callers act on the failure; a formatted message can still be sufficient at a final display boundary."

[rules.API_001.avoid]
language = "rust"
text = '''
fn load_port(input: &str) -> Result<u16, String> {
    input.parse().map_err(|error| format!("invalid port: {error}"))
}
'''

[rules.API_001.prefer]
language = "rust"
text = '''
enum PortError {
    InvalidPort(std::num::ParseIntError),
}

fn load_port(input: &str) -> Result<u16, PortError> {
    input.parse().map_err(PortError::InvalidPort)
}
'''
```

For a component with `namespace = "API"`, put the same rule in
`[ruleset.rules.API_001]`, with examples in `[ruleset.rules.API_001.avoid]` and
`[ruleset.rules.API_001.prefer]`. Keep the component's purpose and architecture;
do not copy the shared ruleset's top-level `version`, `id`, or `includes` into
that context. Legacy contexts using `id = "API"` use `API_RULE_001` instead.

Use `rapport ruleset rule add <ID>` or `rapport ruleset rule update <ID>` for
shared rules, and `rapport context ruleset rule add <path>` or
`rapport context ruleset rule update <path>` for component rules. Pass the title
as `--text`, the paragraph as `--rationale`, and the paired examples as
`--avoid-example`/`--avoid-language` and `--prefer-example`/`--prefer-language`.
When updating a lesson, keep its title, paragraph, and examples consistent.
Run `rapport context validate` and inspect the rule with `rapport ruleset show
<ID> --rule <RULE_ID>` or `rapport context show <path> --declared`.

## Next

Run `rapport context show .`.
