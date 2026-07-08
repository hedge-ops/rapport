<!-- rapport:init:start -->
## Software Factory

This project uses Rapport for planning, coding, testing, building, and reviewing code. Call `rapport prime` for all the details before doing any of these activities.
<!-- rapport:init:end -->

## Dogfooding Rapport

When changing Rapport itself, run the Rapport workflow with an installed or copied Rapport binary rather than `cargo run -p rapport -- ...`; `rapport build` may need to rebuild the CLI executable. Prefer `cargo binstall rapport` once that path is available.
