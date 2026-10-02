//! Explicit component execution; context inspection never enters this module.

mod adapters;
pub(crate) mod declaration;
mod error;
mod freshness;
mod graph;

use crate::{CommandContext, policy_context};
use clap::Args;
use error::Error;
pub(crate) use graph::Target as BuildTarget;
use graph::{Artifact, Operation, Target};
use rapport_command::{CommandSpec, Runner, SystemRunner};
use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};
use std::{collections::BTreeMap, io::Write, process::ExitCode};

#[derive(Debug, Args)]
pub(crate) struct Cli {
    /// Explicit repository-relative components.
    #[arg(required = true, num_args = 1..)]
    paths: Vec<Utf8PathBuf>,
    /// Print the dependency plan without running tools.
    #[arg(long)]
    dry_run: bool,
    /// Regenerate disposable inputs even when their content receipt is current.
    #[arg(long)]
    force: bool,
}

#[derive(Debug, Args)]
pub(crate) struct GenerateCli {
    path: Utf8PathBuf,
    output: String,
    #[arg(long)]
    force: bool,
    #[arg(long)]
    dry_run: bool,
}

pub(crate) fn run<F: FileSystem, O: Write, E: Write>(
    cli: &Cli,
    target: Target,
    context: &mut CommandContext<'_, F, O, E>,
) -> ExitCode {
    let result = execute(cli, target, context);
    finish(result, context.err)
}

fn finish(result: Result<(), Error>, err: &mut impl Write) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(err, "{error}");
            ExitCode::FAILURE
        }
    }
}

fn execute<F: FileSystem, O: Write, E: Write>(
    cli: &Cli,
    target: Target,
    context: &mut CommandContext<'_, F, O, E>,
) -> Result<(), Error> {
    let declarations = policy_context::declarations(context.fs, &context.repo_root)?;
    let plan = graph::plan(&declarations, &cli.paths, target)?;
    let prepared = prepare(context.fs, &context.repo_root, &declarations, plan)?;
    run_prepared(context, prepared, cli.force, cli.dry_run, false)
}

pub(crate) fn generate<F: FileSystem, O: Write, E: Write>(
    cli: &GenerateCli,
    context: &mut CommandContext<'_, F, O, E>,
) -> ExitCode {
    let result = (|| {
        let declarations = policy_context::declarations(context.fs, &context.repo_root)?;
        let plan = graph::generate(
            &declarations,
            Artifact {
                component: cli.path.clone(),
                output: cli.output.clone(),
            },
        )?;
        let prepared = prepare(context.fs, &context.repo_root, &declarations, plan)?;
        run_prepared(context, prepared, cli.force, cli.dry_run, true)
    })();
    finish(result, context.err)
}

struct Prepared {
    operation: Operation,
    commands: Vec<CommandSpec>,
    generator: Option<declaration::Generator>,
}

fn prepare(
    fs: &impl FileSystem,
    root: &Utf8Path,
    contexts: &BTreeMap<Utf8PathBuf, policy_context::ComponentContext>,
    plan: Vec<Operation>,
) -> Result<Vec<Prepared>, Error> {
    plan.into_iter()
        .map(|operation| {
            let (commands, generator) = match &operation {
                Operation::Run { component, target } => {
                    let declaration = contexts
                        .get(component)
                        .ok_or_else(|| graph::Error::MissingComponent(component.clone()))?;
                    (
                        adapters::component(fs, root, component, declaration, *target)?,
                        None,
                    )
                }
                Operation::Generate(artifact) => {
                    let declaration = contexts.get(&artifact.component).ok_or_else(|| {
                        graph::Error::MissingComponent(artifact.component.clone())
                    })?;
                    let generator = declaration
                        .execution()
                        .and_then(|config| config.generators.get(&artifact.output))
                        .ok_or_else(|| Error::MissingGenerator {
                            component: artifact.component.clone(),
                            output: artifact.output.clone(),
                        })?;
                    let output = declaration
                        .generated_outputs()
                        .iter()
                        .find(|(name, _)| name.as_str() == artifact.output)
                        .map(|(_, output)| output)
                        .ok_or_else(|| graph::Error::MissingOutput {
                            component: artifact.component.clone(),
                            output: artifact.output.clone(),
                        })?;
                    generator
                        .adapter
                        .validate_output(output.tool(), output.target())?;
                    (
                        adapters::generation(root, &artifact.component, &generator.adapter)?,
                        Some(generator.clone()),
                    )
                }
            };
            Ok(Prepared {
                operation,
                commands,
                generator,
            })
        })
        .collect()
}

fn run_prepared<F: FileSystem, O: Write, E: Write>(
    context: &mut CommandContext<'_, F, O, E>,
    operations: Vec<Prepared>,
    force: bool,
    dry_run: bool,
    explicit_generation: bool,
) -> Result<(), Error> {
    let _guard = if dry_run {
        None
    } else {
        let canonical = context
            .repo_root
            .canonicalize_utf8()
            .map_err(|source| Error::Io {
                path: context.repo_root.clone(),
                source,
            })?;
        let key = rapport_command::ResourceKey::new(freshness::digest([canonical
            .as_str()
            .as_bytes()
            .to_vec()]))
        .map_err(|error| Error::Declaration(error.to_string()))?;
        Some(
            rapport_command::MachineResources::rapport_default()
                .acquire(&key)
                .map_err(|source| Error::Io {
                    path: canonical,
                    source,
                })?,
        )
    };
    if !dry_run {
        for (label, args) in [
            ("revision", vec!["rev-parse", "HEAD"]),
            ("working tree", vec!["status", "--porcelain"]),
        ] {
            match SystemRunner.run(
                &CommandSpec::new("git")
                    .args(args)
                    .current_dir(&context.repo_root),
            ) {
                Ok(result) if result.success() => {
                    let _ = writeln!(context.out, "{label}: {}", result.stdout_lossy().trim());
                }
                _ => {
                    let _ = writeln!(context.out, "{label}: unavailable");
                }
            }
        }
    }
    run_with_runner(
        context,
        operations,
        force,
        dry_run,
        explicit_generation,
        &SystemRunner,
    )
}

fn run_with_runner<F: FileSystem, O: Write, E: Write>(
    context: &mut CommandContext<'_, F, O, E>,
    operations: Vec<Prepared>,
    force: bool,
    dry_run: bool,
    explicit_generation: bool,
    runner: &impl Runner,
) -> Result<(), Error> {
    for operation in operations {
        let _ = writeln!(context.out, "{:?}", operation.operation);
        if dry_run {
            for spec in &operation.commands {
                let _ = writeln!(context.out, "{spec:?}");
            }
            continue;
        }
        if let Some(config) = &operation.generator {
            generated(
                context,
                &operation,
                config,
                force,
                explicit_generation,
                runner,
            )?;
        } else {
            for spec in &operation.commands {
                invoke(runner, spec, context.out, context.err)?;
            }
        }
    }
    Ok(())
}

fn invoke(
    runner: &impl Runner,
    spec: &CommandSpec,
    out: &mut impl Write,
    err: &mut impl Write,
) -> Result<Vec<u8>, Error> {
    let result = runner.run(spec).map_err(|source| Error::Spawn {
        program: spec.program().to_owned(),
        source,
    })?;
    let _ = out.write_all(result.stdout());
    let _ = err.write_all(result.stderr());
    if !result.success() {
        return Err(Error::Failed {
            program: spec.program().to_owned(),
            code: result.exit_code(),
        });
    }
    Ok(result.stdout().to_vec())
}

fn generated<F: FileSystem, O: Write, E: Write>(
    context: &mut CommandContext<'_, F, O, E>,
    prepared: &Prepared,
    config: &declaration::Generator,
    force: bool,
    explicit: bool,
    runner: &impl Runner,
) -> Result<(), Error> {
    let root = &context.repo_root;
    let identity = format!(
        "{}:{:?}:{:?}",
        env!("CARGO_PKG_VERSION"),
        prepared.operation,
        prepared.commands
    );
    let mut identity_parts = vec![identity.into_bytes()];
    let mut programs = prepared
        .commands
        .iter()
        .map(CommandSpec::program)
        .collect::<std::collections::BTreeSet<_>>();
    if programs.contains("cargo") {
        programs.insert("rustc");
    }
    for program in programs {
        identity_parts.push(invoke(
            runner,
            &CommandSpec::new(program).arg("--version").current_dir(root),
            &mut std::io::sink(),
            context.err,
        )?);
    }
    for name in &config.environment {
        identity_parts.push(name.as_bytes().to_vec());
        identity_parts.push(std::env::var(name).unwrap_or_default().into_bytes());
    }
    let basis = freshness::basis(root, config, freshness::digest(identity_parts).as_bytes())?;
    let key = freshness::digest([format!("{:?}", prepared.operation).into_bytes()]);
    let receipt_path = freshness::inside(root, &format!("target/rapport/generation/{key}.json"))?;
    let previous: Option<freshness::Receipt> = std::fs::read(&receipt_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    let current = freshness::outputs(root, config).ok();
    if !force
        && previous.as_ref().is_some_and(|receipt| {
            receipt.basis == basis && Some(&receipt.outputs) == current.as_ref()
        })
    {
        let _ = writeln!(context.out, "reused");
        return Ok(());
    }
    if config.committed
        && !explicit
        && config
            .outputs
            .iter()
            .any(|output| output.candidate.is_none())
    {
        return Err(Error::Stale(format!("{:?}", prepared.operation)));
    }
    if receipt_path.exists() {
        std::fs::remove_file(&receipt_path).map_err(|source| Error::Io {
            path: receipt_path.clone(),
            source,
        })?;
    }
    for output in config.outputs.iter() {
        let output = freshness::inside(root, &output.path)?;
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent).map_err(|source| Error::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
    }
    for spec in &prepared.commands {
        invoke(runner, spec, context.out, context.err)?;
    }
    reconcile_candidates(root, config, explicit)?;
    let outputs = freshness::outputs(root, config)?;
    let receipt = freshness::Receipt { basis, outputs };
    if let Some(parent) = receipt_path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| Error::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let temporary = receipt_path.with_extension("tmp");
    std::fs::write(&temporary, serde_json::to_vec(&receipt)?).map_err(|source| Error::Io {
        path: temporary.clone(),
        source,
    })?;
    std::fs::rename(&temporary, &receipt_path).map_err(|source| Error::Io {
        path: receipt_path,
        source,
    })?;
    let _ = writeln!(context.out, "generated");
    Ok(())
}

#[cfg(test)]
mod tests;

fn reconcile_candidates(
    root: &Utf8Path,
    config: &declaration::Generator,
    explicit: bool,
) -> Result<(), Error> {
    for output in config.outputs.iter() {
        let Some(candidate) = &output.candidate else {
            continue;
        };
        let candidate = freshness::inside(root, candidate)?;
        let output = freshness::inside(root, &output.path)?;
        let bytes = std::fs::read(&candidate).map_err(|source| Error::Io {
            path: candidate.clone(),
            source,
        })?;
        if config.committed && !explicit {
            if std::fs::read(&output).ok().as_ref() != Some(&bytes) {
                return Err(Error::Stale(output.to_string()));
            }
        } else {
            if let Some(parent) = output.parent() {
                std::fs::create_dir_all(parent).map_err(|source| Error::Io {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            std::fs::write(&output, bytes).map_err(|source| Error::Io {
                path: output,
                source,
            })?;
        }
    }
    Ok(())
}
