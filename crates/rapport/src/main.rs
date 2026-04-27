use std::path::Path;
use std::process::{Command, ExitCode};
use std::time::Instant;

const USAGE: &str = "usage: rapport <build|fix|lint> <path>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let [v, p] = args.as_slice() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let (verb, path) = (v.as_str(), p.as_str());

    let steps: &[&[&str]] = match verb {
        "build" => &[&["build"]],
        "fix" => &[&["fmt"]],
        "lint" => &[
            &["fmt", "--", "--check"],
            &["clippy", "--all-targets", "--", "-D", "warnings"],
        ],
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };

    run_verb(verb, path, steps)
}

fn run_verb(verb: &str, path: &str, steps: &[&[&str]]) -> ExitCode {
    let dir = Path::new(path);
    if !dir.is_dir() {
        eprintln!("You ran: rapport {verb} {path}");
        eprintln!("{path} does not exist or is not a directory.");
        return ExitCode::from(2);
    }

    let started = Instant::now();
    for step in steps {
        let output = Command::new("cargo").args(*step).current_dir(dir).output();
        let output = match output {
            Ok(o) => o,
            Err(err) => {
                eprintln!("You ran: rapport {verb} {path}");
                eprintln!("Failed to invoke cargo: {err}");
                return ExitCode::from(2);
            }
        };

        if !output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stdout.trim().is_empty() {
                eprint!("{stdout}");
            }
            if !stderr.trim().is_empty() {
                eprint!("{stderr}");
            }
            eprintln!();
            eprintln!("status: FAIL");
            eprintln!("duration: {:.2}s", started.elapsed().as_secs_f64());
            return ExitCode::from(1);
        }
    }

    println!("status: pass");
    println!("duration: {:.2}s", started.elapsed().as_secs_f64());
    ExitCode::SUCCESS
}
