# rapport-prose

Talk to humans and agents in clean markdown.

Agents and humans read markdown well, so we need to write prose that reflects that, in a way where we don't lose our minds outputting a bunch of strings. This crate is an ergonomic way to write output for a CLI in a way that an agent and human will both understand, so you can get on with your day.

## Installation

Add this to your `Cargo.toml`:

```toml
rapport-prose = "0.1.0"
```

## Usage

```rust
use rapport_prose::OutputBuilder;

let output = OutputBuilder::new()
    .h1("Release")
    .field("version", "0.1.0")
    .field("date", "2026-04-27")
    .build();

println!("{output}");
```

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <http://opensource.org/licenses/MIT>)

at your option.
