//! Preserves and renders finalized Work outside its repository.
//!
//! Owns explicit history inspection and removal. Atomic publication remains a
//! storage responsibility, while human-readable evidence stays in rendering.

mod render;
mod repository;

use super::Error;
use crate::context::{Clock, CommandContext};
use clap::{Args, Subcommand};
use rapport_files::FileSystem;
use std::io::Write;

pub(super) use repository::HistoryStore;

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub(super) struct Cli {
    #[command(subcommand)]
    action: Action,
}

#[derive(Debug, Subcommand)]
enum Action {
    /// List finalized Work newest first.
    List,
    /// Show one complete historical Work record.
    Show { work_id: String },
    /// Permanently remove one historical Work record.
    Remove {
        work_id: String,
        /// Apply the displayed permanent removal.
        #[arg(long)]
        confirm: bool,
    },
    /// Permanently remove all Work History.
    Clear {
        /// Apply the displayed permanent removal.
        #[arg(long)]
        confirm: bool,
    },
}

pub(super) fn execute<F, C, O, E>(
    cli: &Cli,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let history = HistoryStore::new(&context.repo_root)?;
    match &cli.action {
        Action::List => list(context.fs, &history),
        Action::Show { work_id } => show(context.fs, &history, work_id),
        Action::Remove { work_id, confirm } => remove(context.fs, &history, work_id, *confirm),
        Action::Clear { confirm } => clear(context.fs, &history, *confirm),
    }
}

fn list(fs: &impl FileSystem, history: &HistoryStore) -> Result<String, Error> {
    let lines = history
        .records(fs)?
        .iter()
        .map(|record| {
            format!(
                "{}  {}",
                short_id(record.work.id),
                one_line(&record.work.title)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!(
        "# rapport work history list\n\n{}",
        if lines.is_empty() { "none" } else { &lines }
    ))
}

fn show(fs: &impl FileSystem, history: &HistoryStore, prefix: &str) -> Result<String, Error> {
    let record = history.resolve(fs, prefix)?;
    render::record(&record)
}

fn remove(
    fs: &mut impl FileSystem,
    history: &HistoryStore,
    prefix: &str,
    confirm: bool,
) -> Result<String, Error> {
    let record = history.resolve(fs, prefix)?;
    let proposal = format!(
        "# rapport work history remove\n\n- `records` — 1\n- `work` — {}\n- `title` — {}\n- `archive` — {}\n- `permanent` — true",
        record.work.id, record.work.title, record.path
    );
    if !confirm {
        return Ok(format!(
            "{proposal}\n- `removed` — false\n- `next` — `rapport work history remove {} --confirm`",
            record.work.id
        ));
    }
    fs.remove_dir_all(&record.path)
        .map_err(|source| Error::Io {
            path: record.path,
            source,
        })?;
    Ok(format!(
        "{proposal}\n- `removed` — true\n- `repository changed` — false"
    ))
}

fn clear(fs: &mut impl FileSystem, history: &HistoryStore, confirm: bool) -> Result<String, Error> {
    let count = history.records(fs)?.len();
    let proposal =
        format!("# rapport work history clear\n\n- `records` — {count}\n- `permanent` — true");
    if !confirm {
        return Ok(format!(
            "{proposal}\n- `removed` — false\n- `next` — `rapport work history clear --confirm`"
        ));
    }
    if fs.is_dir(&history.root) {
        fs.remove_dir_all(&history.root)
            .map_err(|source| Error::Io {
                path: history.root.clone(),
                source,
            })?;
    }
    Ok(format!(
        "{proposal}\n- `removed` — true\n- `repository changed` — false"
    ))
}

fn short_id(id: uuid::Uuid) -> String {
    id.to_string().chars().take(6).collect()
}

fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}
