//! Built-in tool invocations for component operations and generation.

use super::{Error, declaration::Generator, graph::Target};
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
    let kind = context.component_type().unwrap_or("unspecified");
    let cwd = root.join(path);
    let command = |program: &str, args: &[&str]| {
        CommandSpec::new(program)
            .args(args.iter().copied())
            .current_dir(&cwd)
    };
    let targets = if target == Target::Validate {
        vec![Target::Check, Target::Build, Target::Test]
    } else {
        vec![target]
    };
    let mut commands = Vec::new();
    for target in targets {
        match kind {
            "crate" => {
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
                if target == Target::Check {
                    commands.push(cargo(&["fmt"]).args(["--", "--check"]));
                }
                let mut spec = match target {
                    Target::Check => cargo(&["clippy", "--all-targets"]),
                    Target::Build => cargo(&["build"]),
                    Target::Test => cargo(&["nextest", "run"]),
                    Target::Validate => unreachable!(),
                };
                if let Some(config) = context.execution() {
                    if !config.features.is_empty() {
                        spec = spec.args(["--features", &config.features.join(",")]);
                    }
                    if target == Target::Test
                        && let Some(config_file) = &config.test_config
                    {
                        spec = spec.args(["--config-file", config_file]);
                    }
                }
                if target == Target::Check {
                    spec = spec.args(["--", "-D", "warnings"]);
                }
                commands.push(spec);
            }
            "swift_package" => {
                require_host("macos")?;
                match target {
                    Target::Check => {
                        commands.push(command(
                            "swiftformat",
                            &[".", "--lint", "--swiftversion", "6"],
                        ));
                        commands.push(command("swiftlint", &["lint", "--strict"]));
                    }
                    Target::Build => commands.push(command("swift", &["build"])),
                    Target::Test => commands.push(command("swift", &["test"])),
                    Target::Validate => unreachable!(),
                }
            }
            "mcp_ui" => {
                if commands.is_empty() {
                    commands.push(command("bun", &["install", "--frozen-lockfile"]));
                }
                match target {
                    Target::Check => {
                        commands.push(command("bun", &["run", "format:check"]));
                        commands.push(command("bun", &["run", "check"]));
                    }
                    Target::Build => commands.push(command("bun", &["run", "build"])),
                    Target::Test => commands.push(command("bun", &["run", "test"])),
                    Target::Validate => unreachable!(),
                }
            }
            _ => {
                return Err(Error::UnsupportedComponent {
                    component: path.to_path_buf(),
                    kind: kind.to_owned(),
                });
            }
        }
    }
    Ok(commands)
}

pub(crate) fn generation(
    root: &Utf8Path,
    path: &Utf8Path,
    tool: &str,
    target: &str,
    config: &Generator,
) -> Result<Vec<CommandSpec>, Error> {
    let destination = super::freshness::inside(root, &config.destination)?;
    let cargo = CommandSpec::new("cargo").current_dir(root);
    match (tool, target) {
        ("facet_generate", "swift" | "kotlin" | "csharp") => Ok(vec![
            cargo
                .args(["run", "--manifest-path"])
                .arg(root.join(path).join("Cargo.toml").as_str())
                .args([
                    "--bin",
                    "codegen",
                    "--features",
                    "codegen,facet_typegen",
                    "--",
                    "--language",
                    target,
                    "--output-dir",
                ])
                .arg(destination.as_str()),
        ]),
        ("locale_strings_generate", "swift" | "kotlin") => {
            let locales = config.locales.as_ref().ok_or_else(|| {
                Error::Declaration("locale generator requires locales".to_owned())
            })?;
            let locales = super::freshness::inside(root, locales)?;
            let package = config.package.as_deref().unwrap_or("generate-strings");
            let mut commands = vec![
                cargo
                    .args(["run", "--manifest-path"])
                    .arg(root.join("Cargo.toml").as_str())
                    .args(["--package", package, "--", target, "--locales"])
                    .arg(locales.as_str())
                    .arg("--output")
                    .arg(destination.as_str()),
            ];
            if let Some(format) = &config.format_config {
                let format = super::freshness::inside(root, format)?;
                commands.push(
                    CommandSpec::new("swiftformat")
                        .current_dir(root)
                        .arg(destination.as_str())
                        .arg("--config")
                        .arg(format.as_str())
                        .args(["--swiftversion", "6"]),
                );
            }
            Ok(commands)
        }
        ("boltffi_generate", "apple" | "android") => {
            if target == "apple" {
                require_host("macos")?;
            }
            Ok(vec![
                CommandSpec::new("boltffi")
                    .args(["pack", target])
                    .current_dir(root.join(path)),
            ])
        }
        ("mcp_ui_generate", "html") => {
            if config.candidates.len() != config.outputs.len() || config.candidates.is_empty() {
                return Err(Error::Declaration(
                    "MCP UI generation requires one candidate per output".to_owned(),
                ));
            }
            Ok(vec![
                CommandSpec::new("bun")
                    .args(["install", "--frozen-lockfile"])
                    .current_dir(root.join(path)),
                CommandSpec::new("bun")
                    .args(["run", "build"])
                    .current_dir(root.join(path)),
            ])
        }
        _ => Err(Error::UnsupportedGenerator {
            tool: tool.to_owned(),
            target: target.to_owned(),
        }),
    }
}

fn require_host(required: &'static str) -> Result<(), Error> {
    let actual = std::env::consts::OS;
    if actual == required {
        Ok(())
    } else {
        Err(Error::Platform { required, actual })
    }
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
