//! Repository settings needed by GitHub integration.
//!
//! Rapport owns its acceptance policy and observes pull-request checks directly.
//! GitHub setup only enables the repository behaviors needed to publish Work.

use crate::{Clock, CommandContext, CommandSpec};
use clap::{Args, Subcommand};
use rapport_files::FileSystem;
use serde::Deserialize;
use std::io::Write;
use std::process::ExitCode;

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub(crate) struct Cli {
    #[command(subcommand)]
    action: Action,
}

#[derive(Debug, Subcommand)]
enum Action {
    /// Enable repository settings used by Rapport integration.
    Setup {
        /// Display the complete proposed changes without applying them.
        #[arg(long)]
        dry_run: bool,
        /// Deprecated compatibility flag; setup now applies by default.
        #[arg(long, hide = true)]
        confirm: bool,
    },
}

pub(crate) fn run<F, C, O, E>(cli: &Cli, context: &mut CommandContext<'_, F, C, O, E>) -> ExitCode
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let result = match cli.action {
        Action::Setup { dry_run, .. } => setup(context, dry_run),
    };
    match result {
        Ok(output) => {
            let _ = writeln!(context.out, "{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            let _ = writeln!(context.err, "{error}");
            ExitCode::from(2)
        }
    }
}

fn setup<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    dry_run: bool,
) -> Result<String, String>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    run_gh(context, ["auth", "status"])?;
    let identity = repository_identity(context)?;
    if !matches!(
        identity.viewer_permission.as_str(),
        "WRITE" | "MAINTAIN" | "ADMIN"
    ) {
        return Err("GitHub identity cannot publish commit statuses".to_owned());
    }
    let proposal = format!(
        "# rapport github setup\n\n- `repository` — {}\n- `branch rules` — unmanaged\n- `squash merge` — enabled\n- `delete merged branches` — enabled",
        identity.name_with_owner
    );
    if dry_run {
        return Ok(format!("{proposal}\n- `applied` — false"));
    }

    run_gh(
        context,
        [
            "repo",
            "edit",
            &identity.name_with_owner,
            "--enable-squash-merge",
            "--delete-branch-on-merge",
        ],
    )?;
    Ok(format!(
        "{proposal}\n- `applied` — true\n- `next` — `rapport doctor`"
    ))
}

pub(crate) fn diagnose<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
) -> Result<String, String>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    run_gh(context, ["auth", "status"])?;
    let identity = repository_identity(context)?;
    if !matches!(
        identity.viewer_permission.as_str(),
        "WRITE" | "MAINTAIN" | "ADMIN"
    ) {
        return Err("GitHub identity cannot publish commit statuses".to_owned());
    }
    if !identity.squash_merge_allowed {
        return Err("squash merge is disabled".to_owned());
    }
    if !identity.delete_branch_on_merge {
        return Err("automatic merged-branch deletion is disabled".to_owned());
    }
    Ok(format!(
        "{} commit statuses writable; branch rules unmanaged; squash and branch deletion enabled",
        identity.name_with_owner
    ))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepositoryIdentity {
    name_with_owner: String,
    squash_merge_allowed: bool,
    #[serde(default)]
    delete_branch_on_merge: bool,
    #[serde(default)]
    viewer_permission: String,
}

fn repository_identity<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
) -> Result<RepositoryIdentity, String>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    serde_json::from_str(&run_gh(
        context,
        [
            "repo",
            "view",
            "--json",
            "nameWithOwner,squashMergeAllowed,deleteBranchOnMerge,viewerPermission",
        ],
    )?)
    .map_err(|error| error.to_string())
}

fn run_gh<F, C, O, E, I, S>(
    context: &mut CommandContext<'_, F, C, O, E>,
    arguments: I,
) -> Result<String, String>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let outcome = context
        .runner
        .run(&CommandSpec::new("gh", arguments), &context.repo_root)
        .map_err(|error| error.to_string())?;
    if outcome.success {
        Ok(outcome.stdout)
    } else {
        Err([outcome.stderr.trim(), outcome.stdout.trim()]
            .into_iter()
            .find(|value| !value.is_empty())
            .unwrap_or("gh exited unsuccessfully")
            .to_owned())
    }
}
