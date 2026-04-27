use std::path::Path;
use std::process::{Command, ExitCode};
use std::time::Instant;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.as_slice() {
        [verb, path] if verb == "build" => run_build(path),
        _ => {
            eprintln!("usage: rapport build <path>");
            ExitCode::from(2)
        }
    }
}

fn run_build(path: &str) -> ExitCode {
    let dir = Path::new(path);
    if !dir.is_dir() {
        eprintln!("You ran: rapport build {path}");
        eprintln!("{path} does not exist or is not a directory.");
        return ExitCode::from(2);
    }

    let started = Instant::now();
    let output = Command::new("cargo").arg("build").current_dir(dir).output();
    let duration = started.elapsed();

    let output = match output {
        Ok(o) => o,
        Err(err) => {
            eprintln!("You ran: rapport build {path}");
            eprintln!("Failed to invoke cargo: {err}");
            return ExitCode::from(2);
        }
    };

    if output.status.success() {
        println!("status: pass");
        println!("duration: {:.2}s", duration.as_secs_f64());
        ExitCode::SUCCESS
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !stdout.trim().is_empty() {
            eprint!("{stdout}");
        }
        if !stderr.trim().is_empty() {
            eprint!("{stderr}");
        }
        eprintln!();
        eprintln!("status: FAIL");
        eprintln!("duration: {:.2}s", duration.as_secs_f64());
        ExitCode::from(1)
    }
}
