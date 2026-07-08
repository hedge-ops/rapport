<!-- rapport:init:start -->
## Rapport

This repository uses Rapport for human-directed agent work. Start with `rapport work start`, inspect context with `rapport work status` and `rapport work rules list`, validate with `rapport build`, and integrate with `rapport integrate`.
<!-- rapport:init:end -->

## Dogfooding Rapport

When changing Rapport itself, run the Rapport workflow with an installed or copied Rapport binary rather than `cargo run -p rapport -- ...`; `rapport build` may need to rebuild the CLI executable. Prefer `cargo binstall rapport` once that path is available.
