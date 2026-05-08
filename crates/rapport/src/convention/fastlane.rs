use super::{DoctorCheck, LifecycleStep, Phase, Project, lifecycle_step};
use crate::{CommandRunner, Verb};
use rapport_cli::FileSystem;
use std::collections::BTreeSet;

const REQUIRED_LANES: [&str; 6] = ["build", "test", "lint", "fix", "validate", "audit"];

pub(super) fn name() -> &'static str {
    "Fastlane"
}

pub(super) fn markers() -> &'static [&'static str] {
    &["fastlane/Fastfile"]
}

pub(super) fn primary_program() -> &'static str {
    "bundle"
}

pub(super) fn toolchain_install_hint() -> &'static str {
    "Install Bundler with `gem install bundler`, or through your Ruby toolchain, and make sure `bundle` is on PATH."
}

pub(super) fn validate_manifest(project: &Project, files: &impl FileSystem) -> Result<(), String> {
    if !files.is_file(project.root.join("Gemfile")) {
        return Err(
            "Fastlane projects must include a `Gemfile` pinning Fastlane so rapport can run `bundle exec fastlane`."
                .into(),
        );
    }

    let marker = project.marker();
    let manifest = project.manifest_path();
    let contents = files
        .read_to_string(&manifest)
        .map_err(|err| format!("Failed to read `{marker}`: {err}"))?;
    let lanes = parse_lane_names(&contents);
    let missing = REQUIRED_LANES
        .into_iter()
        .filter(|lane| !lanes.contains(*lane))
        .collect::<Vec<_>>();

    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "`{marker}` must define standard lanes named {}. Missing lane(s): {}.",
            format_lane_list(REQUIRED_LANES),
            format_lane_list(missing)
        ))
    }
}

pub(super) fn fix() -> Vec<LifecycleStep> {
    vec![fastlane_step(Phase::Fix, "fix")]
}

pub(super) fn lint() -> Vec<LifecycleStep> {
    vec![fastlane_step(Phase::Lint, "lint")]
}

pub(super) fn build() -> Vec<LifecycleStep> {
    vec![fastlane_step(Phase::Build, "build")]
}

pub(super) fn test() -> Vec<LifecycleStep> {
    vec![fastlane_step(Phase::Test, "test")]
}

pub(super) fn validate() -> Vec<LifecycleStep> {
    vec![fastlane_step(Phase::Validate, "validate")]
}

pub(super) fn audit() -> Vec<LifecycleStep> {
    vec![fastlane_step(Phase::Audit, "audit")]
}

pub(super) fn doctor_checks(
    project: &Project,
    runner: &dyn CommandRunner,
    files: &impl FileSystem,
) -> (Vec<DoctorCheck>, Vec<DoctorCheck>) {
    let tools = vec![super::tool_check(
        project,
        runner,
        "bundle",
        primary_program(),
        ["--version"],
        &super::ALL_VERBS,
        Some(toolchain_install_hint()),
    )];
    let mut configuration = vec![
        super::file_check(
            files,
            &project.root.join("Gemfile"),
            "Gemfile",
            &super::ALL_VERBS,
            "Add a `Gemfile` pinning Fastlane.",
        ),
        super::file_check(
            files,
            &project.root.join("fastlane/Fastfile"),
            "fastlane/Fastfile",
            &super::ALL_VERBS,
            "Add `fastlane/Fastfile` with the standard rapport lanes.",
        ),
    ];
    configuration.extend(lane_checks(project, files));
    configuration.push(super::convention_check(
        validate_manifest(project, files),
        "Fastlane convention",
        &super::ALL_VERBS,
        "Add `Gemfile` and standard `fix`, `lint`, `build`, `test`, `validate`, and `audit` lanes.",
    ));
    (tools, configuration)
}

fn lane_checks(project: &Project, files: &impl FileSystem) -> Vec<DoctorCheck> {
    let contents = files
        .read_to_string(project.root.join("fastlane/Fastfile"))
        .unwrap_or_default();
    let lanes = parse_lane_names(&contents);
    [
        ("fix", Verb::Fix),
        ("lint", Verb::Lint),
        ("build", Verb::Build),
        ("test", Verb::Test),
        ("validate", Verb::Validate),
        ("audit", Verb::Audit),
    ]
    .into_iter()
    .map(|(lane, verb)| {
        if lanes.contains(lane) {
            DoctorCheck::pass(format!("fastlane lane `{lane}`"), "present", &[verb], None)
        } else {
            DoctorCheck::fail(
                format!("fastlane lane `{lane}`"),
                "missing",
                &[verb],
                None,
                format!("Add `lane :{lane}` to `fastlane/Fastfile`."),
            )
        }
    })
    .collect()
}

fn fastlane_step(phase: Phase, lane: &'static str) -> LifecycleStep {
    lifecycle_step(phase, "bundle", ["exec", "fastlane", lane])
}

pub(crate) fn parse_lane_names(contents: &str) -> BTreeSet<String> {
    contents.lines().filter_map(parse_lane_name).collect()
}

fn parse_lane_name(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix("lane")?;
    if rest
        .chars()
        .next()
        .is_some_and(|ch| !ch.is_whitespace() && ch != '(')
    {
        return None;
    }

    let rest = rest.trim_start();
    let rest = rest.strip_prefix('(').unwrap_or(rest).trim_start();

    if let Some(symbol) = rest.strip_prefix(':') {
        let name = symbol
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
            .collect::<String>();
        return (!name.is_empty()).then_some(name);
    }

    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let name = rest[quote.len_utf8()..]
        .chars()
        .take_while(|ch| *ch != quote)
        .collect::<String>();
    (!name.is_empty()).then_some(name)
}

fn format_lane_list<I>(lanes: I) -> String
where
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    lanes
        .into_iter()
        .map(|lane| format!("`{}`", lane.as_ref()))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok};
    use indoc::indoc;
    use rapport_cli::{InMemoryFileSystem, Utf8PathBuf};

    fn fastlane_project(root: impl Into<Utf8PathBuf>) -> Project {
        Project {
            convention: super::super::ProjectConvention::Fastlane,
            marker: "fastlane/Fastfile",
            root: root.into(),
        }
    }

    #[test]
    fn lane_parser_accepts_common_fastfile_lane_forms() {
        let lanes = parse_lane_names(indoc! {r#"
            default_platform(:ios)

            lane :build do
            end

            lane(:test) do
            end

            lane "lint" do
            end

            lane('fix') do
            end
        "#});

        assert!(lanes.contains("build"));
        assert!(lanes.contains("test"));
        assert!(lanes.contains("lint"));
        assert!(lanes.contains("fix"));
    }

    #[test]
    fn manifest_validation_requires_gemfile() {
        let root = Utf8PathBuf::from("/work");
        let project = fastlane_project(root.clone());
        let mut files = InMemoryFileSystem::default();
        files.add_file(root.join("fastlane/Fastfile"));

        let err = assert_err!(validate_manifest(&project, &files));

        assert!(err.contains("Gemfile"));
    }

    #[test]
    fn manifest_validation_requires_standard_lanes() {
        let root = Utf8PathBuf::from("/work");
        let project = fastlane_project(root.clone());
        let mut files = InMemoryFileSystem::default();
        files.add_file(root.join("Gemfile"));
        files.add_file_with_contents(
            root.join("fastlane/Fastfile"),
            indoc! {"
                lane :build do
                end
            "},
        );

        let err = assert_err!(validate_manifest(&project, &files));

        assert!(err.contains("standard lanes"));
        assert!(err.contains("`test`"));
        assert!(err.contains("`audit`"));
    }

    #[test]
    fn manifest_validation_accepts_all_standard_lanes() {
        let root = Utf8PathBuf::from("/work");
        let project = fastlane_project(root.clone());
        let mut files = InMemoryFileSystem::default();
        files.add_file(root.join("Gemfile"));
        files.add_file_with_contents(
            root.join("fastlane/Fastfile"),
            indoc! {"
                lane :build do
                end

                lane :test do
                end

                lane :lint do
                end

                lane :fix do
                end

                lane :validate do
                end

                lane :audit do
                end
            "},
        );

        assert_ok!(validate_manifest(&project, &files));
    }
}
