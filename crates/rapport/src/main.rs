//! Rapport command-line executable.
//!
//! The binary owns process I/O wiring and delegates all workflow behavior to
//! the library entrypoint.

use rapport::RealCommandRunner;
use std::io;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    rapport::run(
        std::env::args().skip(1),
        &RealCommandRunner,
        &mut stdout,
        &mut stderr,
    )
}
