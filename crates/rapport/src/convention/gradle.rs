use super::{DoctorCheck, LifecycleStep, Project, declarative::ConventionDefinition};
use crate::{CommandRunner, Verb};
use rapport_cli::FileSystem;
use std::sync::LazyLock;

const STANDARD_TASKS: [&str; 4] = ["assemble", "test", "check", "build"];

static DEFINITION: LazyLock<ConventionDefinition> = LazyLock::new(|| {
    ConventionDefinition::parse("gradle", include_str!("definitions/gradle.toml"))
});

fn definition() -> &'static ConventionDefinition {
    &DEFINITION
}

pub(super) fn name() -> &'static str {
    definition().name()
}

pub(super) fn markers() -> Vec<&'static str> {
    definition().markers()
}

pub(super) fn primary_program() -> &'static str {
    definition().primary_program()
}

pub(super) fn toolchain_install_hint() -> &'static str {
    definition()
        .toolchain_install_hint()
        .unwrap_or("Install a JDK and make sure `java` is on PATH.")
}

pub(super) fn validate_manifest(project: &Project, files: &impl FileSystem) -> Result<(), String> {
    let marker = project.marker();
    let settings_path = project.manifest_path();
    files
        .read_to_string(&settings_path)
        .map_err(|err| format!("Failed to read `{marker}`: {err}"))?;

    if !files.is_file(project.root.join("gradlew")) {
        return Err(
            "Gradle projects must include a `./gradlew` wrapper script at the project root. Run `gradle wrapper` from the project root to generate it."
                .into(),
        );
    }
    Ok(())
}

pub(super) fn fix() -> Vec<LifecycleStep> {
    gradle_steps(Verb::Fix)
}

pub(super) fn lint() -> Vec<LifecycleStep> {
    gradle_steps(Verb::Lint)
}

pub(super) fn build() -> Vec<LifecycleStep> {
    gradle_steps(Verb::Build)
}

pub(super) fn test() -> Vec<LifecycleStep> {
    gradle_steps(Verb::Test)
}

pub(super) fn validate() -> Vec<LifecycleStep> {
    gradle_steps(Verb::Validate)
}

pub(super) fn audit() -> Vec<LifecycleStep> {
    gradle_steps(Verb::Audit)
}

const NO_FIX_VERBS: [Verb; 5] = [
    Verb::Lint,
    Verb::Build,
    Verb::Test,
    Verb::Validate,
    Verb::Audit,
];

pub(super) fn doctor_checks(
    project: &Project,
    runner: &dyn CommandRunner,
    files: &impl FileSystem,
) -> (Vec<DoctorCheck>, Vec<DoctorCheck>) {
    let tools = vec![super::tool_check(
        project,
        runner,
        "java",
        "java",
        ["-version"],
        &NO_FIX_VERBS,
        Some(toolchain_install_hint()),
    )];
    let configuration = vec![
        super::file_check(
            files,
            &project.manifest_path(),
            project.marker(),
            &NO_FIX_VERBS,
            "Add a Gradle settings file at the project root.",
        ),
        super::file_check(
            files,
            &project.root.join("gradlew"),
            "./gradlew",
            &NO_FIX_VERBS,
            "Run `gradle wrapper` from the project root and commit the wrapper script.",
        ),
        super::convention_check(
            validate_manifest(project, files),
            "Gradle wrapper convention",
            &NO_FIX_VERBS,
            "Keep a checked-in Gradle wrapper and settings file at the project root.",
        ),
    ];
    (tools, configuration)
}

pub(crate) fn curate_failure_output(output: &str) -> String {
    if is_missing_java(output) {
        return format!(
            "Gradle wrapper could not find Java.\n\n{}",
            toolchain_install_hint()
        );
    }
    if let Some(task) = parse_missing_task(output) {
        return format!(
            "Gradle did not find standard task `{task}`.\n\nRapport's Gradle convention uses standard Gradle lifecycle tasks: {}.",
            format_task_list(STANDARD_TASKS)
        );
    }

    let failure = GradleFailure::parse(output);
    failure.render()
}

fn gradle_steps(verb: Verb) -> Vec<LifecycleStep> {
    definition().steps(verb)
}

fn format_task_list<I>(tasks: I) -> String
where
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    tasks
        .into_iter()
        .map(|task| format!("`{}`", task.as_ref()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn is_missing_java(output: &str) -> bool {
    output.contains("JAVA_HOME is not set")
        || output.contains("no 'java' command could be found")
        || output.contains("java command could not be found")
        || output.contains("Unable to locate a Java Runtime")
}

fn parse_missing_task(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let trimmed = line.trim();
        let rest = trimmed.strip_prefix("Task '")?;
        let task = rest.split('\'').next()?.trim();
        (trimmed.contains("not found") && !task.is_empty()).then(|| task.to_owned())
    })
}

#[derive(Debug, Default)]
struct GradleFailure {
    tasks: Vec<String>,
    tests: Vec<String>,
    lint_findings: Vec<String>,
    details: Vec<String>,
}

impl GradleFailure {
    fn parse(output: &str) -> Self {
        let mut failure = Self::default();
        for line in output.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(task) = parse_failing_task(trimmed) {
                push_unique(&mut failure.tasks, task);
            } else if let Some(test) = parse_failing_test(trimmed) {
                push_unique(&mut failure.tests, test);
            } else if is_lint_finding(trimmed) {
                push_unique(&mut failure.lint_findings, trimmed.to_owned());
            } else if let Some(detail) = parse_gradle_detail(trimmed) {
                push_unique(&mut failure.details, detail);
            }
        }
        failure
    }

    fn render(&self) -> String {
        let mut sections = Vec::new();
        push_section(&mut sections, "Failing task(s):", &self.tasks);
        push_section(&mut sections, "Failing test(s):", &self.tests);
        push_section(&mut sections, "Lint finding(s):", &self.lint_findings);
        push_section(&mut sections, "Gradle detail(s):", &self.details);

        if sections.is_empty() {
            "Gradle failed, but no structured failure lines were found. Re-run the Gradle wrapper for full output."
                .into()
        } else {
            sections.join("\n\n")
        }
    }
}

fn parse_failing_task(line: &str) -> Option<String> {
    if let Some(rest) = line.strip_prefix("> Task ") {
        return rest
            .strip_suffix(" FAILED")
            .map(str::trim)
            .filter(|task| !task.is_empty())
            .map(ToOwned::to_owned);
    }
    let prefix = "Execution failed for task '";
    let rest = line.strip_prefix(prefix)?;
    let task = rest.split('\'').next()?.trim();
    (!task.is_empty()).then(|| task.to_owned())
}

fn parse_failing_test(line: &str) -> Option<String> {
    line.strip_suffix(" FAILED")
        .filter(|candidate| candidate.contains(" > "))
        .map(str::trim)
        .filter(|candidate| !candidate.is_empty())
        .map(ToOwned::to_owned)
}

fn is_lint_finding(line: &str) -> bool {
    (line.contains(": Error: ") || line.contains(": Warning: "))
        && line
            .rsplit_once('[')
            .is_some_and(|(_, issue)| issue.ends_with(']'))
}

fn parse_gradle_detail(line: &str) -> Option<String> {
    line.strip_prefix("> ")
        .map(str::trim)
        .filter(|detail| {
            !detail.is_empty()
                && !detail.starts_with("Task ")
                && *detail != "Run with --scan to get full insights."
        })
        .map(ToOwned::to_owned)
}

fn push_unique(items: &mut Vec<String>, item: String) {
    if !items.contains(&item) {
        items.push(item);
    }
}

fn push_section(sections: &mut Vec<String>, title: &str, items: &[String]) {
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

#[cfg(test)]
mod tests {
    use super::super::ProjectConvention;
    use super::*;
    use claims::{assert_err, assert_ok};
    use indoc::indoc;
    use rapport_cli::{InMemoryFileSystem, Utf8PathBuf};

    fn gradle_project(root: impl Into<Utf8PathBuf>) -> Project {
        Project {
            convention: ProjectConvention::Gradle,
            marker: "settings.gradle.kts",
            root: root.into(),
        }
    }

    #[test]
    fn manifest_validation_requires_wrapper() {
        let root = Utf8PathBuf::from("/work");
        let project = gradle_project(root.clone());
        let mut files = InMemoryFileSystem::default();
        files.add_file_with_contents(
            root.join("settings.gradle.kts"),
            "rootProject.name = \"app\"\n",
        );

        let err = assert_err!(validate_manifest(&project, &files));

        assert!(err.contains("`./gradlew`"));
        assert!(err.contains("gradle wrapper"));
    }

    #[test]
    fn manifest_validation_accepts_settings_and_wrapper() {
        let root = Utf8PathBuf::from("/work");
        let project = gradle_project(root.clone());
        let mut files = InMemoryFileSystem::default();
        files.add_file_with_contents(
            root.join("settings.gradle.kts"),
            "rootProject.name = \"app\"\n",
        );
        files.add_file(root.join("gradlew"));

        assert_ok!(validate_manifest(&project, &files));
    }

    #[test]
    fn missing_task_output_reports_convention() {
        let curated = curate_failure_output("Task 'assemble' not found in root project 'app'.");

        assert!(curated.contains("Gradle did not find standard task `assemble`"));
        assert!(curated.contains("`build`"));
    }

    #[test]
    fn missing_java_output_reports_jdk_hint() {
        let curated = curate_failure_output(
            "ERROR: JAVA_HOME is not set and no 'java' command could be found in your PATH.",
        );

        assert!(curated.contains("Gradle wrapper could not find Java"));
        assert!(curated.contains("Install a JDK"));
    }

    #[test]
    fn failure_output_is_curated() {
        let output = indoc! {"
            noise line one
            > Task :app:compileLocalDebugKotlin FAILED
            Execution failed for task ':app:compileLocalDebugKotlin'.
            > Compilation error. See log for details.
            com.example.GreeterTest > saysHello FAILED
            app/src/main/AndroidManifest.xml:12: Error: Missing icon [IconMissingDensityFolder]
            noise line two
        "};

        let curated = curate_failure_output(output);

        assert!(curated.contains("Failing task(s):"));
        assert!(curated.contains("- :app:compileLocalDebugKotlin"));
        assert!(curated.contains("Failing test(s):"));
        assert!(curated.contains("- com.example.GreeterTest > saysHello"));
        assert!(curated.contains("Lint finding(s):"));
        assert!(curated.contains("IconMissingDensityFolder"));
        assert!(!curated.contains("noise line"));
    }
}
