use super::{DoctorCheck, LifecycleStep, Phase, Project, gradle, lifecycle_step, message_step};
use crate::{CommandRunner, Verb};
use rapport_cli::{FileSystem, Utf8Path, Utf8PathBuf};

const NAME: &str = "Android app";
const MARKERS: [&str; 2] = ["settings.gradle.kts", "settings.gradle"];
const PRIMARY_PROGRAM: &str = "./gradlew";
const TOOLCHAIN_INSTALL_HINT: &str = "Install a JDK and Android SDK, set `ANDROID_HOME` or `sdk.dir` when needed, and use the checked-in `./gradlew` wrapper.";

const NO_FIX_VERBS: [Verb; 5] = [
    Verb::Lint,
    Verb::Build,
    Verb::Test,
    Verb::Validate,
    Verb::Audit,
];

const ALL_LIFECYCLE_VERBS: [Verb; 6] = [
    Verb::Fix,
    Verb::Lint,
    Verb::Build,
    Verb::Test,
    Verb::Validate,
    Verb::Audit,
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct AndroidModule {
    path: String,
    dev_variant: String,
    release_variant: String,
    has_ktlint: bool,
    has_detekt: bool,
}

impl AndroidModule {
    fn task(&self, task: &str) -> String {
        if self.path.is_empty() {
            task.to_owned()
        } else {
            format!("{}:{task}", self.path)
        }
    }

    fn label(&self) -> String {
        let path = if self.path.is_empty() {
            "<root>".to_owned()
        } else {
            self.path.clone()
        };
        format!(
            "{path} (dev {}, release {})",
            self.dev_variant, self.release_variant
        )
    }
}

pub(super) fn name() -> &'static str {
    NAME
}

pub(super) fn markers() -> &'static [&'static str] {
    &MARKERS
}

pub(super) fn primary_program() -> &'static str {
    PRIMARY_PROGRAM
}

pub(super) fn toolchain_install_hint() -> &'static str {
    TOOLCHAIN_INSTALL_HINT
}

pub(super) fn matching_marker(root: &Utf8Path, files: &impl FileSystem) -> Option<&'static str> {
    markers().iter().copied().find(|marker| {
        files.is_file(root.join(marker))
            && app_modules_for_root(root, marker, files).is_ok_and(|modules| !modules.is_empty())
    })
}

pub(super) fn validate_manifest(project: &Project, files: &impl FileSystem) -> Result<(), String> {
    let marker = project.marker();
    files
        .read_to_string(project.manifest_path())
        .map_err(|err| format!("Failed to read `{marker}`: {err}"))?;

    if !files.is_file(project.root.join("gradlew")) {
        return Err(
            "Android app projects must include a checked-in `./gradlew` wrapper script at the Gradle root. Run `gradle wrapper` from the project root to generate it."
                .into(),
        );
    }

    let modules = app_modules(project, files)?;
    if modules.is_empty() {
        return Err(
            "Android app projects must include at least one root or included Gradle module that applies the `com.android.application` plugin."
                .into(),
        );
    }

    Ok(())
}

pub(super) fn fix(
    project: &Project,
    files: &impl FileSystem,
) -> Result<Vec<LifecycleStep>, String> {
    let tasks = modules(project, files)?
        .into_iter()
        .filter(|module| module.has_ktlint)
        .map(|module| module.task("ktlintFormat"))
        .collect::<Vec<_>>();

    if tasks.is_empty() {
        Ok(vec![message_step(
            Phase::Fix,
            "No Android formatter task is configured; leaving project unchanged.",
        )])
    } else {
        Ok(vec![gradle_step(Phase::Fix, tasks)])
    }
}

pub(super) fn lint(
    project: &Project,
    files: &impl FileSystem,
) -> Result<Vec<LifecycleStep>, String> {
    let tasks = modules(project, files)?
        .into_iter()
        .flat_map(|module| {
            let mut tasks = Vec::new();
            if module.has_ktlint {
                tasks.push(module.task("ktlintCheck"));
            }
            if module.has_detekt {
                tasks.push(module.task("detekt"));
            }
            tasks.push(module.task(&format!("lint{}", module.dev_variant)));
            tasks
        })
        .collect::<Vec<_>>();
    Ok(vec![gradle_step(Phase::Lint, tasks)])
}

pub(super) fn build(
    project: &Project,
    files: &impl FileSystem,
) -> Result<Vec<LifecycleStep>, String> {
    let tasks = modules(project, files)?
        .into_iter()
        .map(|module| module.task(&format!("assemble{}", module.dev_variant)))
        .collect::<Vec<_>>();
    Ok(vec![gradle_step(Phase::Build, tasks)])
}

pub(super) fn test(
    project: &Project,
    files: &impl FileSystem,
) -> Result<Vec<LifecycleStep>, String> {
    let tasks = modules(project, files)?
        .into_iter()
        .map(|module| module.task(&format!("test{}UnitTest", module.dev_variant)))
        .collect::<Vec<_>>();
    Ok(vec![gradle_step(Phase::Test, tasks)])
}

pub(super) fn validate(
    project: &Project,
    files: &impl FileSystem,
) -> Result<Vec<LifecycleStep>, String> {
    let tasks = validate_tasks(project, files)?;
    Ok(vec![gradle_step(Phase::Validate, tasks)])
}

pub(super) fn audit(
    project: &Project,
    files: &impl FileSystem,
) -> Result<Vec<LifecycleStep>, String> {
    let mut tasks = validate_tasks(project, files)?;
    tasks.extend(
        modules(project, files)?
            .into_iter()
            .map(|module| module.task(&format!("bundle{}", module.release_variant))),
    );
    Ok(vec![gradle_step(Phase::Audit, tasks)])
}

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
        &ALL_LIFECYCLE_VERBS,
        Some(toolchain_install_hint()),
    )];

    let mut configuration = vec![
        super::file_check(
            files,
            &project.manifest_path(),
            project.marker(),
            &NO_FIX_VERBS,
            "Add a Gradle settings file at the Android project root.",
        ),
        super::file_check(
            files,
            &project.root.join("gradlew"),
            "./gradlew",
            &ALL_LIFECYCLE_VERBS,
            "Run `gradle wrapper` from the Android project root and commit the wrapper script.",
        ),
        super::convention_check(
            validate_manifest(project, files),
            "Android app convention",
            &ALL_LIFECYCLE_VERBS,
            "Apply `com.android.application` in at least one app module and keep Android code generation wired into the Gradle task graph.",
        ),
    ];

    if let Ok(modules) = modules(project, files) {
        configuration.push(module_summary_check(&modules));
        configuration.push(optional_style_check(
            "ktlint",
            modules.iter().any(|module| module.has_ktlint),
            "configured app modules run `ktlintFormat` for fix and `ktlintCheck` during lint/validate/audit",
            "not configured; fix skips formatting and lint omits ktlint",
            &[Verb::Fix, Verb::Lint, Verb::Validate, Verb::Audit],
            "Apply the `org.jlleitschuh.gradle.ktlint` plugin to app modules when Kotlin formatting should be enforced.",
        ));
        configuration.push(optional_style_check(
            "detekt",
            modules.iter().any(|module| module.has_detekt),
            "configured app modules run `detekt` during lint/validate/audit",
            "not configured; lint omits detekt",
            &[Verb::Lint, Verb::Validate, Verb::Audit],
            "Apply the `io.gitlab.arturbosch.detekt` plugin to app modules when static analysis should be enforced.",
        ));
    }

    (tools, configuration)
}

pub(crate) fn curate_failure_output(output: &str) -> String {
    if is_missing_java(output) {
        return format!(
            "Android Gradle wrapper could not find Java.\n\n{}",
            toolchain_install_hint()
        );
    }
    if is_missing_android_sdk(output) {
        return format!("Android SDK was not found.\n\n{}", toolchain_install_hint());
    }
    if is_plugin_resolution_failure(output) {
        return "Android/Kotlin Gradle plugin resolution failed.\n\nMake sure the Android Gradle Plugin, Kotlin plugin, configured plugin repositories, and version catalog entries are available to the Gradle wrapper."
            .into();
    }
    if let Some(task) = parse_missing_task(output) {
        return format!(
            "Gradle did not find Android convention task `{task}`.\n\nRapport derives module-qualified Android tasks from app modules and variants, such as `:app:assembleLocalDebug`, `:app:testLocalDebugUnitTest`, `:app:lintLocalDebug`, and `:app:bundleProductionRelease`."
        );
    }

    gradle::curate_failure_output(output)
}

fn validate_tasks(project: &Project, files: &impl FileSystem) -> Result<Vec<String>, String> {
    Ok(modules(project, files)?
        .into_iter()
        .flat_map(|module| {
            let mut tasks = Vec::new();
            if module.has_ktlint {
                tasks.push(module.task("ktlintCheck"));
            }
            if module.has_detekt {
                tasks.push(module.task("detekt"));
            }
            tasks.push(module.task(&format!("lint{}", module.dev_variant)));
            tasks.push(module.task(&format!("assemble{}", module.dev_variant)));
            tasks.push(module.task(&format!("test{}UnitTest", module.dev_variant)));
            tasks
        })
        .collect())
}

fn gradle_step(phase: Phase, tasks: Vec<String>) -> LifecycleStep {
    let args = std::iter::once("--no-daemon".to_owned())
        .chain(tasks)
        .collect::<Vec<_>>();
    lifecycle_step(phase, PRIMARY_PROGRAM, args)
}

fn module_summary_check(modules: &[AndroidModule]) -> DoctorCheck {
    DoctorCheck::pass(
        "Android app module(s)",
        modules
            .iter()
            .map(AndroidModule::label)
            .collect::<Vec<_>>()
            .join(", "),
        &ALL_LIFECYCLE_VERBS,
        None,
    )
}

fn optional_style_check(
    name: &str,
    configured: bool,
    configured_detail: &'static str,
    missing_detail: &'static str,
    affects: &[Verb],
    remediation: &'static str,
) -> DoctorCheck {
    if configured {
        DoctorCheck::pass(name, configured_detail, affects, None)
    } else {
        DoctorCheck::warn(name, missing_detail, affects, None, remediation)
    }
}

fn modules(project: &Project, files: &impl FileSystem) -> Result<Vec<AndroidModule>, String> {
    app_modules(project, files)
}

fn app_modules(project: &Project, files: &impl FileSystem) -> Result<Vec<AndroidModule>, String> {
    app_modules_for_root(&project.root, project.marker(), files)
}

fn app_modules_for_root(
    root: &Utf8Path,
    marker: &str,
    files: &impl FileSystem,
) -> Result<Vec<AndroidModule>, String> {
    let settings_path = root.join(marker);
    let settings = files
        .read_to_string(&settings_path)
        .map_err(|err| format!("Failed to read `{marker}`: {err}"))?;

    let mut module_paths = included_module_paths(&settings);
    module_paths.push(String::new());
    module_paths.sort();
    module_paths.dedup();

    let mut modules = module_paths
        .into_iter()
        .filter_map(|module_path| {
            let module_root = module_root(root, &module_path);
            let (_, contents) = read_build_script(&module_root, files)?;
            is_android_app_build_script(&contents).then(|| AndroidModule {
                path: module_path,
                dev_variant: dev_variant(&contents),
                release_variant: release_variant(&contents),
                has_ktlint: has_configured_plugin(&contents, "ktlint")
                    || has_configured_plugin(&contents, "org.jlleitschuh.gradle.ktlint"),
                has_detekt: has_configured_plugin(&contents, "detekt")
                    || has_configured_plugin(&contents, "io.gitlab.arturbosch.detekt"),
            })
        })
        .collect::<Vec<_>>();
    modules.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(modules)
}

fn included_module_paths(settings: &str) -> Vec<String> {
    let mut modules = Vec::new();
    for line in settings.lines() {
        let line = line.trim();
        if line.starts_with("include") {
            modules.extend(
                quoted_strings(line).filter(|value| {
                    value.starts_with(':') && value.len() > 1 && !value.contains('*')
                }),
            );
        }
    }
    modules.sort();
    modules.dedup();
    modules
}

fn quoted_strings(line: &str) -> impl Iterator<Item = String> + '_ {
    let mut values = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '"' && ch != '\'' {
            continue;
        }
        let quote = ch;
        let mut value = String::new();
        for ch in chars.by_ref() {
            if ch == quote {
                break;
            }
            value.push(ch);
        }
        values.push(value);
    }
    values.into_iter()
}

fn module_root(root: &Utf8Path, module_path: &str) -> Utf8PathBuf {
    if module_path.is_empty() {
        root.to_owned()
    } else {
        root.join(module_path.trim_start_matches(':').replace(':', "/"))
    }
}

fn read_build_script(root: &Utf8Path, files: &impl FileSystem) -> Option<(Utf8PathBuf, String)> {
    ["build.gradle.kts", "build.gradle"]
        .into_iter()
        .find_map(|name| {
            let path = root.join(name);
            files
                .read_to_string(&path)
                .ok()
                .map(|contents| (path, contents))
        })
}

fn is_android_app_build_script(contents: &str) -> bool {
    contents.lines().any(|line| {
        let line = code_line(line);
        !line.contains("apply false")
            && (line.contains("com.android.application") || line.contains("android.application"))
    })
}

fn has_configured_plugin(contents: &str, needle: &str) -> bool {
    contents.lines().any(|line| {
        let line = code_line(line);
        !line.contains("apply false") && line.contains(needle)
    })
}

fn dev_variant(contents: &str) -> String {
    if declares_flavor(contents, "local") {
        "LocalDebug".to_owned()
    } else {
        "Debug".to_owned()
    }
}

fn release_variant(contents: &str) -> String {
    if declares_flavor(contents, "production") {
        "ProductionRelease".to_owned()
    } else {
        "Release".to_owned()
    }
}

fn declares_flavor(contents: &str, name: &str) -> bool {
    contents.lines().any(|line| {
        let line = code_line(line);
        line.contains(&format!("create(\"{name}\")"))
            || line.contains(&format!("create('{name}')"))
            || line.contains(&format!("maybeCreate(\"{name}\")"))
            || line.contains(&format!("maybeCreate('{name}')"))
            || line.trim_start().starts_with(&format!("{name} {{"))
    })
}

fn code_line(line: &str) -> &str {
    line.split("//").next().unwrap_or("").trim()
}

fn is_missing_android_sdk(output: &str) -> bool {
    output.contains("SDK location not found")
        || output.contains("Android SDK location not found")
        || output.contains("ANDROID_HOME is not set")
        || output.contains("ANDROID_SDK_ROOT is not set")
}

fn is_missing_java(output: &str) -> bool {
    output.contains("JAVA_HOME is not set")
        || output.contains("no 'java' command could be found")
        || output.contains("java command could not be found")
        || output.contains("Unable to locate a Java Runtime")
}

fn is_plugin_resolution_failure(output: &str) -> bool {
    (output.contains("Plugin [id: 'com.android.application']")
        || output.contains("Plugin with id 'com.android.application'")
        || output.contains("Plugin [id: 'org.jetbrains.kotlin.android']")
        || output.contains("Plugin with id 'org.jetbrains.kotlin.android'"))
        && output.contains("not found")
}

fn parse_missing_task(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let trimmed = line.trim();
        let rest = trimmed.strip_prefix("Task '")?;
        let task = rest.split('\'').next()?.trim();
        (trimmed.contains("not found") && !task.is_empty()).then(|| task.to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::super::LifecycleAction;
    use super::super::ProjectConvention;
    use super::*;
    use claims::{assert_err, assert_ok};
    use rapport_cli::InMemoryFileSystem;

    fn android_project(root: impl Into<Utf8PathBuf>) -> Project {
        Project {
            convention: ProjectConvention::AndroidApp,
            marker: "settings.gradle.kts",
            root: root.into(),
        }
    }

    fn add_basic_android_app(files: &mut InMemoryFileSystem, root: &Utf8Path) {
        files.add_file_with_contents(
            root.join("settings.gradle.kts"),
            "rootProject.name = \"app\"\ninclude(\":app\")\ninclude(\":shared\")\n",
        );
        files.add_file(root.join("gradlew"));
        files.add_file_with_contents(
            root.join("build.gradle.kts"),
            "plugins { alias(libs.plugins.android.application) apply false }\n",
        );
        files.add_file_with_contents(
            root.join("app/build.gradle.kts"),
            r#"
plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.ktlint)
    alias(libs.plugins.detekt)
}

android {
    productFlavors {
        create("local")
        create("production")
    }
}
"#,
        );
        files.add_file_with_contents(
            root.join("shared/build.gradle.kts"),
            "plugins { alias(libs.plugins.android.library) }\n",
        );
    }

    #[test]
    fn discovery_recognizes_android_app_module_beyond_generic_gradle() {
        let root = Utf8PathBuf::from("/work");
        let mut files = InMemoryFileSystem::default();
        add_basic_android_app(&mut files, &root);

        assert_eq!(matching_marker(&root, &files), Some("settings.gradle.kts"));
    }

    #[test]
    fn discovery_ignores_generic_gradle_without_android_app_module() {
        let root = Utf8PathBuf::from("/work");
        let mut files = InMemoryFileSystem::default();
        files.add_file_with_contents(
            root.join("settings.gradle.kts"),
            "rootProject.name = \"lib\"\n",
        );
        files.add_file_with_contents(
            root.join("build.gradle.kts"),
            "plugins { id(\"java-library\") }\n",
        );

        assert_eq!(matching_marker(&root, &files), None);
    }

    #[test]
    fn manifest_validation_requires_wrapper_and_app_module() {
        let root = Utf8PathBuf::from("/work");
        let project = android_project(root.clone());
        let mut files = InMemoryFileSystem::default();
        files.add_file_with_contents(root.join("settings.gradle.kts"), "include(\":app\")\n");
        files.add_file_with_contents(
            root.join("app/build.gradle.kts"),
            "plugins { id(\"com.android.application\") }\n",
        );

        let err = assert_err!(validate_manifest(&project, &files));

        assert!(err.contains("`./gradlew`"));
    }

    #[test]
    fn manifest_validation_accepts_app_module() {
        let root = Utf8PathBuf::from("/work");
        let project = android_project(root.clone());
        let mut files = InMemoryFileSystem::default();
        add_basic_android_app(&mut files, &root);

        assert_ok!(validate_manifest(&project, &files));
    }

    #[test]
    fn lint_tasks_include_optional_style_and_android_lint() {
        let root = Utf8PathBuf::from("/work");
        let project = android_project(root.clone());
        let mut files = InMemoryFileSystem::default();
        add_basic_android_app(&mut files, &root);

        let steps = assert_ok!(lint(&project, &files));

        assert_eq!(steps.len(), 1);
        let LifecycleAction::Command(spec) = &steps[0].action else {
            panic!("lint should run gradle");
        };
        assert_eq!(spec.program, "./gradlew");
        assert_eq!(
            spec.args,
            vec![
                "--no-daemon",
                ":app:ktlintCheck",
                ":app:detekt",
                ":app:lintLocalDebug"
            ]
        );
    }

    #[test]
    fn audit_extends_validate_with_release_bundle() {
        let root = Utf8PathBuf::from("/work");
        let project = android_project(root.clone());
        let mut files = InMemoryFileSystem::default();
        add_basic_android_app(&mut files, &root);

        let steps = assert_ok!(audit(&project, &files));

        let LifecycleAction::Command(spec) = &steps[0].action else {
            panic!("audit should run gradle");
        };
        assert!(spec.args.contains(&":app:lintLocalDebug".to_owned()));
        assert!(spec.args.contains(&":app:assembleLocalDebug".to_owned()));
        assert!(
            spec.args
                .contains(&":app:testLocalDebugUnitTest".to_owned())
        );
        assert!(
            spec.args
                .contains(&":app:bundleProductionRelease".to_owned())
        );
    }

    #[test]
    fn missing_task_output_reports_android_convention() {
        let curated = curate_failure_output(
            "Task ':app:assembleLocalDebug' not found in root project 'app'.",
        );

        assert!(curated.contains("Android convention task `:app:assembleLocalDebug`"));
        assert!(curated.contains(":app:bundleProductionRelease"));
    }

    #[test]
    fn missing_android_sdk_output_reports_install_hint() {
        let curated = curate_failure_output("SDK location not found. Define a valid SDK location.");

        assert!(curated.contains("Android SDK was not found"));
        assert!(curated.contains("ANDROID_HOME"));
    }

    #[test]
    fn missing_java_output_reports_android_toolchain_hint() {
        let curated = curate_failure_output(
            "ERROR: JAVA_HOME is not set and no 'java' command could be found in your PATH.",
        );

        assert!(curated.contains("Android Gradle wrapper could not find Java"));
        assert!(curated.contains("Android SDK"));
    }
}
