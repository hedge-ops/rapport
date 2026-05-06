use super::{LifecycleStep, Phase, Project};
use crate::Verb;
use rapport_cli::{FileSystem, Utf8Path, Utf8PathBuf};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

const MARKERS: [&str; 1] = ["package.json"];
const LOCKFILES: [&str; 2] = ["bun.lock", "bun.lockb"];
const STANDARD_SCRIPTS: [&str; 5] = ["build", "test", "lint", "fix", "audit"];
const SKIP_DIRECTORIES: [&str; 4] = ["node_modules", "dist", "build", "coverage"];

pub(super) fn name() -> &'static str {
    "Bun"
}

pub(super) fn markers() -> &'static [&'static str] {
    &MARKERS
}

pub(super) fn primary_program() -> &'static str {
    "bun"
}

pub(super) fn toolchain_install_hint() -> &'static str {
    "Install Bun from https://bun.sh/docs/installation and make sure `bun` is on PATH."
}

pub(super) fn matching_marker(root: &Utf8Path, files: &impl FileSystem) -> Option<&'static str> {
    files
        .is_file(root.join("package.json"))
        .then(|| find_lockfile_root(root, files))
        .flatten()
        .map(|_| "package.json")
}

pub(super) fn validate_manifest(project: &Project, files: &impl FileSystem) -> Result<(), String> {
    let package = read_package_json(&project.root, files)?;
    if !package.scripts_are_strings() {
        return Err("Bun `package.json` scripts must be string commands.".into());
    }
    if find_lockfile_root(&project.root, files).is_none() {
        return Err(
            "Bun projects must include `bun.lock` or `bun.lockb` at the package root or an ancestor workspace root."
                .into(),
        );
    }
    Ok(())
}

pub(super) fn fix(
    project: &Project,
    files: &impl FileSystem,
) -> Result<Vec<LifecycleStep>, String> {
    script_steps(project, files, &[Verb::Fix])
}

pub(super) fn lint(
    project: &Project,
    files: &impl FileSystem,
) -> Result<Vec<LifecycleStep>, String> {
    script_steps(project, files, &[Verb::Lint])
}

pub(super) fn build(
    project: &Project,
    files: &impl FileSystem,
) -> Result<Vec<LifecycleStep>, String> {
    script_steps(project, files, &[Verb::Build])
}

pub(super) fn test(
    project: &Project,
    files: &impl FileSystem,
) -> Result<Vec<LifecycleStep>, String> {
    script_steps(project, files, &[Verb::Test])
}

pub(super) fn validate(
    project: &Project,
    files: &impl FileSystem,
) -> Result<Vec<LifecycleStep>, String> {
    script_steps(project, files, &[Verb::Lint, Verb::Build, Verb::Test])
}

pub(super) fn audit(
    project: &Project,
    files: &impl FileSystem,
) -> Result<Vec<LifecycleStep>, String> {
    script_steps(project, files, &[Verb::Audit])
}

pub(super) fn has_no_standard_scripts(root: &Utf8Path, files: &impl FileSystem) -> bool {
    read_package_json(root, files).is_ok_and(|package| {
        STANDARD_SCRIPTS
            .iter()
            .all(|script| !package.has_script(script))
    })
}

pub(super) fn should_skip_directory(name: &str) -> bool {
    SKIP_DIRECTORIES.contains(&name)
}

pub(super) fn curate_failure_output(output: &str) -> String {
    let failure = BunFailure::parse(output);
    failure.render()
}

fn script_steps(
    project: &Project,
    files: &impl FileSystem,
    verbs: &[Verb],
) -> Result<Vec<LifecycleStep>, String> {
    let package = read_package_json(&project.root, files)?;
    let missing = verbs
        .iter()
        .map(|verb| script_name(*verb))
        .filter(|script| !package.has_script(script))
        .collect::<Vec<_>>();

    if !missing.is_empty() {
        return Err(format!(
            "Bun project must define {} in `package.json` for `rapport {}`.",
            format_scripts(&missing),
            if verbs.len() == 3 {
                "validate"
            } else {
                missing[0]
            },
        ));
    }

    Ok(verbs
        .iter()
        .map(|verb| {
            super::lifecycle_step(
                phase(*verb),
                primary_program(),
                ["run".to_owned(), script_name(*verb).to_owned()],
            )
        })
        .collect())
}

fn phase(verb: Verb) -> Phase {
    match verb {
        Verb::Fix => Phase::Fix,
        Verb::Lint => Phase::Lint,
        Verb::Build => Phase::Build,
        Verb::Test => Phase::Test,
        Verb::Validate => Phase::Validate,
        Verb::Audit => Phase::Audit,
    }
}

fn script_name(verb: Verb) -> &'static str {
    match verb {
        Verb::Fix => "fix",
        Verb::Lint => "lint",
        Verb::Build => "build",
        Verb::Test => "test",
        Verb::Validate => unreachable!("validate is composed from lint, build, and test"),
        Verb::Audit => "audit",
    }
}

fn format_scripts(scripts: &[&str]) -> String {
    let scripts = scripts
        .iter()
        .map(|script| format!("script `{script}`"))
        .collect::<Vec<_>>();
    match scripts.as_slice() {
        [] => String::new(),
        [script] => script.clone(),
        [a, b] => format!("{a} and {b}"),
        _ => {
            let last_index = scripts.len() - 1;
            let last = &scripts[last_index];
            format!("{}, and {last}", scripts[..last_index].join(", "))
        }
    }
}

fn read_package_json(root: &Utf8Path, files: &impl FileSystem) -> Result<PackageJson, String> {
    let path = root.join("package.json");
    let contents = files
        .read_to_string(&path)
        .map_err(|err| format!("Failed to read `package.json`: {err}"))?;
    serde_json::from_str(&contents).map_err(|err| format!("Failed to parse `package.json`: {err}"))
}

fn find_lockfile_root(root: &Utf8Path, files: &impl FileSystem) -> Option<Utf8PathBuf> {
    let mut current = root.to_owned();
    loop {
        if LOCKFILES
            .iter()
            .any(|lockfile| files.is_file(current.join(lockfile)))
        {
            return Some(current);
        }
        if files.exists(current.join(".git")) || !current.pop() {
            return None;
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct PackageJson {
    scripts: BTreeMap<String, serde_json::Value>,
}

impl PackageJson {
    fn has_script(&self, name: &str) -> bool {
        self.scripts
            .get(name)
            .is_some_and(serde_json::Value::is_string)
    }

    fn scripts_are_strings(&self) -> bool {
        self.scripts.values().all(serde_json::Value::is_string)
    }
}

#[derive(Debug, Default)]
struct BunFailure {
    build_errors: BTreeSet<String>,
    test_failures: BTreeSet<String>,
    lint_findings: BTreeSet<String>,
}

impl BunFailure {
    fn parse(output: &str) -> Self {
        let mut failure = Self::default();
        for line in output
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
        {
            let lower = line.to_ascii_lowercase();
            if is_test_failure(&lower) {
                failure.test_failures.insert(line.to_owned());
            } else if is_lint_finding(&lower) {
                failure.lint_findings.insert(line.to_owned());
            } else if is_build_error(&lower) {
                failure.build_errors.insert(line.to_owned());
            }
        }
        failure
    }

    fn render(&self) -> String {
        let mut sections = Vec::new();
        push_section(&mut sections, "Bun build error(s):", &self.build_errors);
        push_section(&mut sections, "Bun test failure(s):", &self.test_failures);
        push_section(&mut sections, "Bun lint finding(s):", &self.lint_findings);

        if sections.is_empty() {
            "Bun command failed, but no structured failure lines were found. Re-run the Bun script for full output."
                .into()
        } else {
            sections.join("\n\n")
        }
    }
}

fn is_build_error(lower: &str) -> bool {
    lower.contains("error:") || lower.contains("build failed") || lower.starts_with("error ")
}

fn is_test_failure(lower: &str) -> bool {
    lower.contains("fail") && (lower.contains("test") || lower.contains(".test."))
}

fn is_lint_finding(lower: &str) -> bool {
    (lower.contains("warning") || lower.contains("lint"))
        && (lower.contains(".ts") || lower.contains(".tsx") || lower.contains(".js"))
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
