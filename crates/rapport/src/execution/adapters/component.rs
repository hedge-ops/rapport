//! Prepare supported component commands from typed operations.
use super::require_host;
use crate::execution::{
    Error,
    declaration::ComponentKind,
    graph::{Step, Target},
};
use crate::policy_context::ComponentContext;
use rapport_command::CommandSpec;
use rapport_files::{FileSystem, Utf8Path};

pub(crate) fn component(
    fs: &impl FileSystem,
    root: &Utf8Path,
    path: &Utf8Path,
    context: &ComponentContext,
    target: Target,
) -> Result<Vec<CommandSpec>, Error> {
    let kind = ComponentKind::parse(context.component_type(), path)?;
    let cwd = root.join(path);
    let command = |program: &str, args: &[&str]| {
        CommandSpec::new(program)
            .args(args.iter().copied())
            .current_dir(&cwd)
    };
    let mut commands = Vec::new();
    for &target in target.steps() {
        match kind {
            ComponentKind::Crate => {
                let package = package_name(fs, &cwd)?;
                let cargo = |args: &[&str]| {
                    let mut spec = CommandSpec::new("cargo").current_dir(root);
                    if let Some(toolchain) = context
                        .execution()
                        .and_then(|config| config.toolchain.as_ref())
                    {
                        spec = spec.arg(format!("+{toolchain}"));
                    }
                    spec.args(args.iter().copied())
                        .args(["--package", &package])
                };
                if target == Step::Check {
                    commands.push(cargo(&["fmt"]).args(["--", "--check"]));
                }
                let mut spec = match target {
                    Step::Check => cargo(&["clippy", "--all-targets"]),
                    Step::Build => cargo(&["build"]),
                    Step::Test => cargo(&["nextest", "run"]),
                };
                if let Some(config) = context.execution() {
                    if !config.features.is_empty() {
                        spec = spec.args(["--features", &config.features.join(",")]);
                    }
                    if target == Step::Test
                        && let Some(config_file) = &config.test_config
                    {
                        spec = spec.args(["--config-file", config_file]);
                    }
                }
                if target == Step::Check {
                    spec = spec.args(["--", "-D", "warnings"]);
                }
                commands.push(spec);
            }
            ComponentKind::SwiftPackage => {
                require_host("macos")?;
                match target {
                    Step::Check => {
                        commands.push(command(
                            "swiftformat",
                            &[".", "--lint", "--swiftversion", "6"],
                        ));
                        commands.push(command("swiftlint", &["lint", "--strict"]));
                    }
                    Step::Build => commands.push(command("swift", &["build"])),
                    Step::Test => commands.push(command("swift", &["test"])),
                }
            }
            ComponentKind::McpUi => {
                if commands.is_empty() {
                    commands.push(command("bun", &["install", "--frozen-lockfile"]));
                }
                match target {
                    Step::Check => {
                        commands.push(command("bun", &["run", "format:check"]));
                        commands.push(command("bun", &["run", "check"]));
                    }
                    Step::Build => commands.push(command("bun", &["run", "build"])),
                    Step::Test => commands.push(command("bun", &["run", "test"])),
                }
            }
        }
    }
    Ok(commands)
}

fn package_name(fs: &impl FileSystem, path: &Utf8Path) -> Result<String, Error> {
    let path = path.join("Cargo.toml");
    let contents = fs.read_to_string(&path).map_err(|source| Error::Io {
        path: path.clone(),
        source,
    })?;
    let manifest: toml::Value = toml::from_str(&contents).map_err(|source| Error::Toml {
        path: path.clone(),
        source,
    })?;
    manifest
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| Error::Declaration(format!("{path} has no package.name")))
}
