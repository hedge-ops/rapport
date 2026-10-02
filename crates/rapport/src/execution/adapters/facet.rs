//! Facet code generation conventions.
use crate::execution::{Error, declaration::FacetLanguage, freshness};
use rapport_command::CommandSpec;
use rapport_files::Utf8Path;

pub(super) fn commands(
    root: &Utf8Path,
    path: &Utf8Path,
    language: FacetLanguage,
    destination: &str,
) -> Result<Vec<CommandSpec>, Error> {
    let destination = freshness::inside(root, destination)?;
    Ok(vec![
        CommandSpec::new("cargo")
            .current_dir(root)
            .args(["run", "--manifest-path"])
            .arg(root.join(path).join("Cargo.toml").as_str())
            .args([
                "--bin",
                "codegen",
                "--features",
                "codegen,facet_typegen",
                "--",
                "--language",
                language.as_str(),
                "--output-dir",
            ])
            .arg(destination.as_str()),
    ])
}
