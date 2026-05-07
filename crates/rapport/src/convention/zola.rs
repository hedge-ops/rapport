use super::{LifecycleStep, Phase, Project, bun, lifecycle_step, message_step};
use rapport_cli::{FileSystem, Utf8Path};
use std::collections::BTreeSet;
use toml_edit::DocumentMut;

const ZOLA: &str = "zola";
const MARKERS: [&str; 1] = ["config.toml"];
const ZOLA_SECTIONS: [&str; 5] = ["markdown", "markup", "search", "link_checker", "taxonomies"];
const SKIP_DIRECTORIES: [&str; 1] = ["public"];

pub(super) fn name() -> &'static str {
    "Zola"
}

pub(super) fn markers() -> &'static [&'static str] {
    &MARKERS
}

pub(super) fn primary_program() -> &'static str {
    ZOLA
}

pub(super) fn toolchain_install_hint() -> &'static str {
    "Install Zola from https://www.getzola.org/documentation/getting-started/installation/ and make sure `zola` is on PATH."
}

pub(super) fn matching_marker(root: &Utf8Path, files: &impl FileSystem) -> Option<&'static str> {
    let config = root.join("config.toml");
    if !files.is_file(&config) {
        return None;
    }
    let contents = files.read_to_string(&config).ok()?;
    is_zola_config(&contents).then_some("config.toml")
}

pub(super) fn validate_manifest(project: &Project, files: &impl FileSystem) -> Result<(), String> {
    let contents = files
        .read_to_string(project.manifest_path())
        .map_err(|err| format!("Failed to read `config.toml`: {err}"))?;
    let config = parse_config(&contents)?;
    if !has_zola_markers(&config) {
        return Err(zola_marker_error());
    }
    if !files.is_dir(project.root.join("content")) {
        return Err(
            "Zola projects must include a `content/` directory at the project root.".into(),
        );
    }
    if !files.is_dir(project.root.join("templates")) {
        return Err(
            "Zola projects must include a `templates/` directory at the project root.".into(),
        );
    }
    Ok(())
}

pub(super) fn fix(
    project: &Project,
    files: &impl FileSystem,
) -> Result<Vec<LifecycleStep>, String> {
    let mut steps = Vec::new();
    if !push_optional_bun_script(&mut steps, project, files, Phase::Fix, &["fix"])? {
        steps.push(message_step(
            Phase::Fix,
            "Zola has no autofix; leaving site content unchanged.",
        ));
    }
    Ok(steps)
}

pub(super) fn lint(
    project: &Project,
    files: &impl FileSystem,
) -> Result<Vec<LifecycleStep>, String> {
    let mut steps = Vec::new();
    push_optional_bun_script(&mut steps, project, files, Phase::Lint, &["lint", "check"])?;
    steps.push(zola_check_step(Phase::Lint));
    Ok(steps)
}

pub(super) fn build(
    project: &Project,
    files: &impl FileSystem,
) -> Result<Vec<LifecycleStep>, String> {
    let mut steps = Vec::new();
    push_optional_bun_script(&mut steps, project, files, Phase::Build, &["build"])?;
    steps.push(zola_build_step(Phase::Build));
    Ok(steps)
}

pub(super) fn test(
    project: &Project,
    files: &impl FileSystem,
) -> Result<Vec<LifecycleStep>, String> {
    let mut steps = Vec::new();
    push_optional_bun_script(&mut steps, project, files, Phase::Test, &["test"])?;
    steps.push(zola_check_step(Phase::Test));
    Ok(steps)
}

pub(super) fn validate(
    project: &Project,
    files: &impl FileSystem,
) -> Result<Vec<LifecycleStep>, String> {
    let mut steps = Vec::new();
    push_optional_bun_script(&mut steps, project, files, Phase::Lint, &["lint", "check"])?;
    steps.push(zola_check_step(Phase::Lint));
    push_optional_bun_script(&mut steps, project, files, Phase::Build, &["build"])?;
    steps.push(zola_build_step(Phase::Build));
    push_optional_bun_script(&mut steps, project, files, Phase::Test, &["test"])?;
    Ok(steps)
}

pub(super) fn audit(
    project: &Project,
    files: &impl FileSystem,
) -> Result<Vec<LifecycleStep>, String> {
    let mut steps = validate(project, files)?;
    steps.push(zola_build_step(Phase::ReleaseBuild));
    Ok(steps)
}

pub(super) fn should_skip_directory(name: &str) -> bool {
    SKIP_DIRECTORIES.contains(&name)
}

pub(super) fn curate_failure_output(output: &str) -> String {
    let failure = ZolaFailure::parse(output);
    failure.render()
}

fn push_optional_bun_script(
    steps: &mut Vec<LifecycleStep>,
    project: &Project,
    files: &impl FileSystem,
    phase: Phase,
    scripts: &[&'static str],
) -> Result<bool, String> {
    if !bun::has_package_with_lockfile(&project.root, files) {
        return Ok(false);
    }

    for script in scripts {
        if bun::has_script(&project.root, files, script)? {
            steps.push(bun::script_step(phase, script));
            return Ok(true);
        }
    }
    Ok(false)
}

fn zola_check_step(phase: Phase) -> LifecycleStep {
    lifecycle_step(phase, ZOLA, ["check"])
}

fn zola_build_step(phase: Phase) -> LifecycleStep {
    lifecycle_step(phase, ZOLA, ["build"])
}

fn is_zola_config(contents: &str) -> bool {
    parse_config(contents).is_ok_and(|config| has_zola_markers(&config))
}

fn parse_config(contents: &str) -> Result<DocumentMut, String> {
    contents
        .parse::<DocumentMut>()
        .map_err(|err| format!("Failed to parse `config.toml`: {err}"))
}

fn has_zola_markers(config: &DocumentMut) -> bool {
    has_string_key(config, "base_url")
        && ZOLA_SECTIONS
            .iter()
            .any(|section| config.get(section).is_some())
}

fn has_string_key(config: &DocumentMut, key: &str) -> bool {
    config
        .get(key)
        .and_then(toml_edit::Item::as_value)
        .and_then(toml_edit::Value::as_str)
        .is_some()
}

fn zola_marker_error() -> String {
    "Zola `config.toml` must define string `base_url` and at least one recognized Zola section (`[markdown]`, `[markup]`, `[search]`, `[link_checker]`, or `[[taxonomies]]`)."
        .into()
}

#[derive(Debug, Default)]
struct ZolaFailure {
    template_errors: BTreeSet<String>,
    link_errors: BTreeSet<String>,
    missing_files: BTreeSet<String>,
    details: BTreeSet<String>,
}

impl ZolaFailure {
    fn parse(output: &str) -> Self {
        let mut failure = Self::default();
        for line in output
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
        {
            let lower = line.to_ascii_lowercase();
            if is_template_error(&lower) {
                failure.template_errors.insert(line.to_owned());
            } else if is_link_error(&lower) {
                failure.link_errors.insert(line.to_owned());
            } else if is_missing_file_error(&lower) {
                failure.missing_files.insert(line.to_owned());
            } else if is_zola_detail(&lower) {
                failure.details.insert(line.to_owned());
            }
        }
        failure
    }

    fn render(&self) -> String {
        let mut sections = Vec::new();
        push_section(
            &mut sections,
            "Zola template error(s):",
            &self.template_errors,
        );
        push_section(&mut sections, "Zola link error(s):", &self.link_errors);
        push_section(&mut sections, "Zola missing file(s):", &self.missing_files);
        push_section(&mut sections, "Zola detail(s):", &self.details);

        if sections.is_empty() {
            "Zola command failed, but no structured failure lines were found. Re-run the Zola command for full output."
                .into()
        } else {
            sections.join("\n\n")
        }
    }
}

fn is_template_error(lower: &str) -> bool {
    lower.contains("template") || lower.contains("tera")
}

fn is_link_error(lower: &str) -> bool {
    lower.contains("broken link") || lower.contains("link checker") || lower.contains("dead link")
}

fn is_missing_file_error(lower: &str) -> bool {
    (lower.contains("not found") || lower.contains("no such file")) && !is_link_error(lower)
}

fn is_zola_detail(lower: &str) -> bool {
    lower.starts_with("error:")
        || lower.starts_with("reason:")
        || lower.contains("failed to")
        || lower.contains("error building site")
}

fn push_section(sections: &mut Vec<String>, title: &str, items: &BTreeSet<String>) {
    if items.is_empty() {
        return;
    }
    let body = items
        .iter()
        .map(|item| format!("- {item}"))
        .collect::<Vec<_>>()
        .join("\n");
    sections.push(format!("{title}\n{body}"));
}
