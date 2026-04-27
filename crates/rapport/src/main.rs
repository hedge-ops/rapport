use std::process::ExitCode;

fn main() -> ExitCode {
    rapport::run(std::env::args().skip(1))
}
