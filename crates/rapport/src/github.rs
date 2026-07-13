//! Repository-owned GitHub integration policy.
//!
//! Owns a dedicated ruleset so Rapport can repair its requirements without
//! weakening or replacing unrelated repository protection.

use crate::{Clock, CommandContext, CommandSpec};
use clap::{Args, Subcommand};
use rapport_files::FileSystem;
use rapport_git::Git;
use serde::Deserialize;
use std::io::Write;
use std::process::ExitCode;

const BUILD_AGGREGATE: &str = "Rapport Build";

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub(crate) struct Cli {
    #[command(subcommand)]
    action: Action,
}

#[derive(Debug, Subcommand)]
enum Action {
    /// Propose or apply Rapport's target-branch integration ruleset.
    Setup {
        /// Apply the displayed repository changes.
        #[arg(long)]
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
        Action::Setup { confirm } => setup(context, confirm),
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
    confirm: bool,
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
    let git = Git::default();
    let repository = git
        .discover(&context.repo_root)
        .map_err(|error| error.to_string())?;
    let target = crate::work_ledger::active_target(context.fs, &context.repo_root)
        .map_err(|error| error.to_string())?
        .map_or_else(
            || {
                git.default_target(&repository)
                    .map(|branch| branch.to_string())
                    .map_err(|error| error.to_string())
            },
            Ok,
        )?;
    let name = format!("Rapport Integration ({target})");
    let existing = repository_rulesets(context, &identity.name_with_owner)?
        .into_iter()
        .find(|ruleset| {
            ruleset.name == name
                && ruleset.source_type == "Repository"
                && ruleset.source == identity.name_with_owner
        });
    let action = if existing.is_some() {
        "update"
    } else {
        "create"
    };
    let proposal = format!(
        "# rapport github setup\n\n- `repository` — {}\n- `target` — {}\n- `ruleset` — {} ({})\n- `pull requests required` — true\n- `required status` — {}\n- `Rapport Review status` — none\n- `required approvals added` — 0\n- `freshness` — loose unless an existing merge queue is effective\n- `squash merge` — enabled\n- `delete merged branches` — enabled",
        identity.name_with_owner, target, name, action, BUILD_AGGREGATE
    );
    if !confirm {
        return Ok(format!(
            "{proposal}\n- `applied` — false\n- `next` — `rapport github setup --confirm`"
        ));
    }

    let payload = ruleset_payload(&name, &target);
    let path = std::env::temp_dir().join(format!(
        "rapport-github-setup-{}-{}.json",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&payload).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("could not write GitHub setup request: {error}"))?;
    let endpoint = if let Some(existing) = existing {
        format!(
            "repos/{}/rulesets/{}",
            identity.name_with_owner, existing.id
        )
    } else {
        format!("repos/{}/rulesets", identity.name_with_owner)
    };
    let method = if action == "update" { "PUT" } else { "POST" };
    let request = run_gh(
        context,
        [
            "api",
            "--method",
            method,
            &endpoint,
            "--input",
            &path.to_string_lossy(),
        ],
    );
    let _ = std::fs::remove_file(&path);
    request?;
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

fn ruleset_payload(name: &str, target: &str) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "target": "branch",
        "enforcement": "active",
        "bypass_actors": [],
        "conditions": {
            "ref_name": {
                "include": [format!("refs/heads/{target}")],
                "exclude": []
            }
        },
        "rules": [
            {
                "type": "pull_request",
                "parameters": {
                    "allowed_merge_methods": ["squash"],
                    "dismiss_stale_reviews_on_push": false,
                    "require_code_owner_review": false,
                    "require_last_push_approval": false,
                    "required_approving_review_count": 0,
                    "required_review_thread_resolution": false
                }
            },
            {
                "type": "required_status_checks",
                "parameters": {
                    "do_not_enforce_on_create": false,
                    "required_status_checks": [{"context": BUILD_AGGREGATE}],
                    "strict_required_status_checks_policy": false
                }
            }
        ]
    })
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
    let active_target = crate::work_ledger::active_target(context.fs, &context.repo_root)
        .map_err(|error| error.to_string())?;
    let target = active_target.as_deref().or_else(|| {
        identity
            .default_branch_ref
            .as_ref()
            .map(|branch| branch.name.as_str())
    });
    let target = target.ok_or_else(|| "GitHub repository has no target branch".to_owned())?;
    let endpoint = format!(
        "repos/{}/rules/branches/{}",
        identity.name_with_owner,
        percent_encode(target)
    );
    let rules: Vec<EffectiveRule> = serde_json::from_str(&run_gh(context, ["api", &endpoint])?)
        .map_err(|error| error.to_string())?;
    if !rules.iter().any(|rule| rule.kind == "pull_request") {
        return Err(format!("pull requests are not required for `{target}`"));
    }
    let aggregate = rules.iter().any(|rule| {
        rule.kind == "required_status_checks"
            && rule
                .parameters
                .get("required_status_checks")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|checks| {
                    checks.iter().any(|check| {
                        check.get("context").and_then(serde_json::Value::as_str)
                            == Some(BUILD_AGGREGATE)
                    })
                })
    });
    if !aggregate {
        return Err(format!(
            "`{BUILD_AGGREGATE}` is not required for `{target}`"
        ));
    }
    let freshness = if rules.iter().any(|rule| rule.kind == "merge_queue") {
        "merge_queue"
    } else if rules.iter().any(|rule| {
        rule.kind == "required_status_checks"
            && rule
                .parameters
                .get("strict_required_status_checks_policy")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
    }) {
        "strict"
    } else {
        "loose"
    };
    Ok(format!(
        "{} target {target}; {BUILD_AGGREGATE} required; freshness {freshness}; squash and branch deletion enabled",
        identity.name_with_owner
    ))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepositoryIdentity {
    name_with_owner: String,
    default_branch_ref: Option<BranchIdentity>,
    #[serde(default)]
    squash_merge_allowed: bool,
    #[serde(default)]
    delete_branch_on_merge: bool,
    #[serde(default)]
    viewer_permission: String,
}

#[derive(Debug, Deserialize)]
struct BranchIdentity {
    name: String,
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
            "nameWithOwner,defaultBranchRef,squashMergeAllowed,deleteBranchOnMerge,viewerPermission",
        ],
    )?)
    .map_err(|error| error.to_string())
}

#[derive(Debug, Deserialize)]
struct RulesetSummary {
    id: u64,
    name: String,
    source: String,
    source_type: String,
}

fn repository_rulesets<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    repository: &str,
) -> Result<Vec<RulesetSummary>, String>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let endpoint = format!("repos/{repository}/rulesets?targets=branch");
    serde_json::from_str(&run_gh(context, ["api", &endpoint])?).map_err(|error| error.to_string())
}

#[derive(Debug, Deserialize)]
struct EffectiveRule {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    parameters: serde_json::Value,
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

fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_payload_requires_only_the_aggregate_build_and_pull_requests() {
        let payload = ruleset_payload("Rapport Integration (main)", "main");
        let rendered = payload.to_string();

        assert!(rendered.contains(BUILD_AGGREGATE));
        assert!(rendered.contains("pull_request"));
        assert!(rendered.contains("required_approving_review_count\":0"));
        assert!(!rendered.contains("Rapport Review"));
        assert!(!rendered.contains("bypass_mode"));
    }
}
