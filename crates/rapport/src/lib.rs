//! Rapport architecture and review library.
//!
//! Exposes the CLI embedding boundary and delegates context, standards, and
//! prompt generation to their owned modules.

mod cli;
mod context;
mod init;
mod paths;
mod policy_context;
mod prime;
mod repository_files;
mod review;
mod shared_ruleset;
mod view;

pub use context::{CommandContext, find_repo_root};
pub use paths::RapportPaths;
pub use view::{Outcome, RunHint, View, ViewBuilder};

use clap::{CommandFactory, Parser, error::ErrorKind};
use cli::{Cli, Command};
use rapport_files::{FileSystem, RealFileSystem, Utf8PathBuf};
use std::io::Write;
use std::process::ExitCode;

/// Run the current `rapport` binary entrypoint.
pub fn run<I, O, E>(argv: I, out: &mut O, err: &mut E) -> ExitCode
where
    I: IntoIterator<Item = String>,
    O: Write,
    E: Write,
{
    let cwd = match current_utf8_dir() {
        Ok(cwd) => cwd,
        Err(error) => {
            let _ = writeln!(err, "{error}");
            return ExitCode::FAILURE;
        }
    };
    let mut fs = RealFileSystem;
    run_with_environment(argv, &mut fs, cwd, out, err)
}

fn run_with_environment<I, F, O, E>(
    argv: I,
    fs: &mut F,
    cwd: Utf8PathBuf,
    out: &mut O,
    err: &mut E,
) -> ExitCode
where
    I: IntoIterator<Item = String>,
    F: FileSystem,
    O: Write,
    E: Write,
{
    let arguments: Vec<String> = argv.into_iter().collect();
    if arguments.is_empty() {
        let _ = write!(out, "{}", Cli::command().render_help());
        return ExitCode::SUCCESS;
    }
    match Cli::try_parse_from(std::iter::once(String::from("rapport")).chain(arguments)) {
        Ok(cli) => {
            let mut context = CommandContext::new(cwd, fs, out, err);
            execute_command(&cli, &mut context)
        }
        Err(error) if error.kind() == ErrorKind::DisplayHelp => {
            let _ = write!(out, "{error}");
            ExitCode::SUCCESS
        }
        Err(error) if error.kind() == ErrorKind::DisplayVersion => {
            let _ = write!(out, "{error}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            let _ = write!(err, "{error}");
            ExitCode::from(2)
        }
    }
}

fn current_utf8_dir() -> Result<Utf8PathBuf, String> {
    let cwd =
        std::env::current_dir().map_err(|error| format!("cannot read current dir: {error}"))?;
    Utf8PathBuf::from_path_buf(cwd)
        .map_err(|path| format!("current dir is not valid UTF-8: {}", path.to_string_lossy()))
}

fn execute_command<F, O, E>(cli: &Cli, context: &mut CommandContext<'_, F, O, E>) -> ExitCode
where
    F: FileSystem,
    O: Write,
    E: Write,
{
    match &cli.command {
        Command::Prime => prime::run(context),
        Command::Init => init::run(context),
        Command::Ruleset(ruleset_args) => shared_ruleset::run(ruleset_args, context),
        Command::Context(context_args) => policy_context::run(context_args, context),
        Command::Review(review_args) => review::run(review_args, context),
    }
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "CLI acceptance tests unwrap deterministic in-memory files and fake-runner queues"
)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rapport_files::InMemoryFileSystem;

    fn run_with(args: &[&str]) -> (ExitCode, String, String) {
        let mut fs = InMemoryFileSystem::default();
        run_with_fs(args, &mut fs)
    }

    fn run_with_fs(args: &[&str], fs: &mut InMemoryFileSystem) -> (ExitCode, String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        fs.add_directory("/repo/.git");
        let code = run_with_environment(
            args.iter().map(|arg| (*arg).to_string()),
            fs,
            Utf8PathBuf::from("/repo"),
            &mut out,
            &mut err,
        );
        (
            code,
            String::from_utf8_lossy(&out).into_owned(),
            String::from_utf8_lossy(&err).into_owned(),
        )
    }

    #[test]
    fn no_args_renders_root_help() {
        let (code, out, err) = run_with(&[]);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("Repository architecture and review benchmarks"));
        assert!(out.contains("rapport review <path>"));
        assert!(out.contains("prime"));
        assert!(!out.contains("  doctor"));
        assert!(!out.contains("  work"));
        assert_eq!(err, "");
    }

    #[test]
    fn help_flag_renders_root_help() {
        let (code, out, err) = run_with(&["--help"]);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("Rapport turns repository-owned architecture"));
        assert!(out.contains("rapport review <path>"));
        assert_eq!(err, "");
    }

    #[test]
    fn version_flag_renders_package_version() {
        let (code, out, err) = run_with(&["--version"]);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(out, format!("rapport {}\n", env!("CARGO_PKG_VERSION")));
        assert_eq!(err, "");
    }

    #[test]
    fn prime_help_exists() {
        let (code, out, err) = run_with(&["prime", "--help"]);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("Show how agents should use Rapport"));
        assert_eq!(err, "");
    }

    #[test]
    fn prime_renders_workflow() {
        let (code, out, err) = run_with(&["prime"]);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("rapport prime"));
        assert!(out.contains("rapport review <path>"));
        assert!(out.contains("rapport context show"));
        assert!(out.contains("without Work"));
        assert_eq!(err, "");
    }

    /// Phase 1 manages catalog and repository Rulesets through the public CLI grammar.
    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "the Phase 1 CLI acceptance test preserves one complete sequential lifecycle"
    )]
    fn shared_ruleset_cli_should_complete_the_phase_one_lifecycle() {
        let mut fs = InMemoryFileSystem::default();

        let (catalog_list_code, catalog_list, catalog_list_error) =
            run_with_fs(&["ruleset", "catalog", "list"], &mut fs);
        assert_eq!(catalog_list_code, ExitCode::SUCCESS);
        assert!(
            catalog_list.contains("`RUST_CRATE`"),
            "expecting catalog list to include the Rust aggregate"
        );
        assert!(
            catalog_list_error.is_empty(),
            "expecting catalog list not to emit an error"
        );

        assert_eq!(
            run_with_fs(&["ruleset", "catalog", "install", "RUST_CRATE"], &mut fs).0,
            ExitCode::SUCCESS
        );
        assert_eq!(
            run_with_fs(&["ruleset", "catalog", "update", "RUST_CRATE"], &mut fs).0,
            ExitCode::SUCCESS
        );
        assert_eq!(
            run_with_fs(
                &[
                    "ruleset",
                    "catalog",
                    "show",
                    "RUST_CRATE",
                    "--rule",
                    "RUST_CODING_001"
                ],
                &mut fs
            )
            .0,
            ExitCode::SUCCESS
        );

        assert_eq!(
            run_with_fs(
                &[
                    "ruleset",
                    "init",
                    "CODE",
                    "--purpose",
                    "Shared coding expectations."
                ],
                &mut fs
            )
            .0,
            ExitCode::SUCCESS
        );
        assert_eq!(
            run_with_fs(
                &[
                    "ruleset",
                    "rule",
                    "add",
                    "CODE",
                    "--id",
                    "CODE_001",
                    "--text",
                    "Prefer explicit names.",
                    "--rationale",
                    "Names retain intent.",
                    "--avoid-example",
                    "let x = value;",
                    "--avoid-language",
                    "rust",
                    "--prefer-example",
                    "let person_count = value;",
                    "--prefer-language",
                    "rust",
                    "--reference",
                    "[Naming](https://example.com/naming)"
                ],
                &mut fs
            )
            .0,
            ExitCode::SUCCESS
        );
        assert_eq!(
            run_with_fs(
                &[
                    "ruleset",
                    "rule",
                    "update",
                    "CODE",
                    "--rule",
                    "CODE_001",
                    "--text",
                    "Use explicit names.",
                    "--clear-reference"
                ],
                &mut fs
            )
            .0,
            ExitCode::SUCCESS
        );
        assert_eq!(
            run_with_fs(
                &[
                    "ruleset",
                    "purpose",
                    "set",
                    "CODE",
                    "--purpose",
                    "Repository coding expectations."
                ],
                &mut fs
            )
            .0,
            ExitCode::SUCCESS
        );
        assert_eq!(
            run_with_fs(
                &[
                    "ruleset",
                    "init",
                    "APP",
                    "--purpose",
                    "Application expectations."
                ],
                &mut fs
            )
            .0,
            ExitCode::SUCCESS
        );
        assert_eq!(
            run_with_fs(
                &["ruleset", "compose", "add", "APP", "--ruleset", "CODE"],
                &mut fs
            )
            .0,
            ExitCode::SUCCESS
        );

        let (_, composition, _) = run_with_fs(&["ruleset", "compose", "list", "APP"], &mut fs);
        let (_, shown_rule, _) =
            run_with_fs(&["ruleset", "show", "APP", "--rule", "CODE_001"], &mut fs);
        let (_, listed, _) = run_with_fs(&["ruleset", "list"], &mut fs);
        assert!(
            composition.contains("`CODE`"),
            "expecting composition status to show the direct Ruleset"
        );
        assert!(
            shown_rule.contains("Use explicit names."),
            "expecting show to resolve a composed Rule"
        );
        assert!(
            listed.contains("Repository coding expectations."),
            "expecting list to show the updated Ruleset purpose"
        );

        assert_eq!(
            run_with_fs(
                &["ruleset", "compose", "remove", "APP", "--ruleset", "CODE"],
                &mut fs
            )
            .0,
            ExitCode::SUCCESS
        );
        assert_eq!(
            run_with_fs(
                &["ruleset", "rule", "remove", "CODE", "--rule", "CODE_001"],
                &mut fs
            )
            .0,
            ExitCode::SUCCESS
        );
        assert_eq!(
            run_with_fs(&["ruleset", "remove", "CODE"], &mut fs).0,
            ExitCode::SUCCESS
        );
        assert_eq!(
            run_with_fs(&["ruleset", "remove", "APP"], &mut fs).0,
            ExitCode::SUCCESS
        );
    }

    #[test]
    fn context_help_explains_project_context_intent() {
        let (code, out, err) = run_with(&["context", "--help"]);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("what a project area is about"));
        assert!(out.contains("Ownership records what belongs"));
        assert!(out.contains("numbered, reviewable benchmarks"));
        assert!(out.contains("context.toml"));
        assert_eq!(err, "");
    }

    #[test]
    fn init_help_exists() {
        let (code, out, err) = run_with(&["init", "--help"]);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("Record Rapport usage"));
        assert_eq!(err, "");
    }

    #[test]
    fn init_creates_root_agents_file() {
        let mut fs = InMemoryFileSystem::default();

        let (code, out, err) = run_with_fs(&["init"], &mut fs);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("status` — created"));
        assert!(out.contains("AGENTS.md"));
        assert!(!out.contains(".github/workflows/rapport-signoff.yml"));
        assert_eq!(err, "");
        let agents = fs.read_to_string("/repo/AGENTS.md").unwrap();

        assert!(agents.contains("## Repository Architecture and Reviews"));
        assert!(agents.contains("rapport prime"));
        assert!(!agents.contains("rapport work start"));
        assert!(!fs.exists("/repo/.github/workflows/rapport-signoff.yml"));
    }

    #[test]
    fn init_updates_existing_agents_file_idempotently() {
        let mut fs = InMemoryFileSystem::default();
        fs.write_string(
            "/repo/AGENTS.md",
            "# Agent Notes\n\nKeep local context current.\n",
        )
        .unwrap();

        let (first_code, first_out, first_err) = run_with_fs(&["init"], &mut fs);
        let first_agents = fs.read_to_string("/repo/AGENTS.md").unwrap();
        let (second_code, second_out, second_err) = run_with_fs(&["init"], &mut fs);
        let second_agents = fs.read_to_string("/repo/AGENTS.md").unwrap();

        assert_eq!(first_code, ExitCode::SUCCESS);
        assert!(first_out.contains("status` — updated"));
        assert_eq!(first_err, "");
        assert_eq!(second_code, ExitCode::SUCCESS);
        assert!(second_out.contains("status` — updated"));
        assert_eq!(second_err, "");
        assert_eq!(first_agents, second_agents);
        assert!(second_agents.contains("# Agent Notes"));
        assert_eq!(
            second_agents.matches("<!-- rapport:init:start -->").count(),
            1
        );
    }
}
