use rapport_cli::RealCommandRunner;
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
