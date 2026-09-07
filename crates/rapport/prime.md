# rapport prime

## Purpose

- Rapport structures repository architecture and review benchmarks in context.toml.
- Generate sourced review prompts for humans and agents.

## Review

- `rapport context init <path> --purpose <text>` - create architecture context for a repository area
- `rapport context show <path>` - inspect architecture, direct declarations, and inherited standards
- `rapport ruleset catalog list` - discover reusable standards packs
- `rapport ruleset catalog install <ID>` - install a standards pack before including it
- `rapport review <path> [<path> ...]` - print a complete Markdown review prompt with source paths
- Pass the prompt and relevant code or diff to your reviewer.

## Repository ownership

- Edit context.toml directly or use context commands; commit architecture and standards with the repository.
- Ancestor context and included packs apply alongside local declarations. Conflicting or missing standards fail explicitly.
- `rapport context validate [path]` checks architecture and effective standards.

## Next

Run `rapport context show .`.
