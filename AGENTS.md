<!-- rapport:init:start -->
## Repository Architecture and Reviews

This project uses Rapport for structured architecture and review benchmarks. Call `rapport prime` for guidance, `rapport context validate` to check declarations, and `rapport review <path>` to generate a component review prompt.
<!-- rapport:init:end -->

## Dogfooding Rapport

When changing Rapport itself, use an installed or copied Rapport binary for context validation and review prompts. Use `just ci` for formatting, lint, build, and tests. Git and GitHub operations belong to the repository workflow.
