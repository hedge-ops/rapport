//! Filesystem contract tests.

use super::*;
use claims::{assert_err, assert_ok};
use std::io;

#[test]
fn in_memory_file_system_recognizes_added_directory() {
    let mut fs = InMemoryFileSystem::default();
    fs.add_directory("/work");

    assert!(fs.is_dir("/work"));
}

#[test]
fn in_memory_file_system_does_not_treat_directory_as_file() {
    let mut fs = InMemoryFileSystem::default();
    fs.add_directory("/work");

    assert!(!fs.is_file("/work"));
}

#[test]
fn in_memory_file_system_recognizes_added_file() {
    let mut fs = InMemoryFileSystem::default();
    fs.add_file("/work/Cargo.toml");

    assert!(fs.is_file("/work/Cargo.toml"));
}

#[test]
fn in_memory_file_system_does_not_treat_file_as_directory() {
    let mut fs = InMemoryFileSystem::default();
    fs.add_file("/work/Cargo.toml");

    assert!(!fs.is_dir("/work/Cargo.toml"));
}

#[test]
fn in_memory_file_system_exists_recognizes_added_file() {
    let mut fs = InMemoryFileSystem::default();
    fs.add_file("/work/Cargo.toml");

    assert!(fs.exists("/work/Cargo.toml"));
}

#[test]
fn in_memory_file_system_reads_added_files() {
    let mut fs = InMemoryFileSystem::default();
    let path = Utf8PathBuf::from("/work/Package.swift");
    fs.add_file_with_contents(&path, "// swift-tools-version: 6.0\n");

    assert_eq!(
        assert_ok!(fs.read_to_string(&path)),
        "// swift-tools-version: 6.0\n"
    );
}

#[test]
fn in_memory_file_system_reports_missing_files() {
    let fs = InMemoryFileSystem::default();
    let path = Utf8PathBuf::from("/work/Package.swift");

    let err = assert_err!(fs.read_to_string(&path));

    assert_eq!(err.kind(), io::ErrorKind::NotFound);
}

#[test]
fn in_memory_file_system_reads_immediate_children_sorted() {
    let mut fs = InMemoryFileSystem::default();
    fs.add_file("/work/z.toml");
    fs.add_file("/work/a.toml");
    fs.add_file("/work/nested/config.toml");

    let children = assert_ok!(fs.read_dir("/work"));

    assert_eq!(
        children,
        vec![
            Utf8PathBuf::from("/work/a.toml"),
            Utf8PathBuf::from("/work/nested"),
            Utf8PathBuf::from("/work/z.toml"),
        ]
    );
}

#[test]
fn in_memory_file_system_writes_files() {
    let mut fs = InMemoryFileSystem::default();

    assert_ok!(fs.write_string("/work/.rapport/work.toml", "schema_version = 1\n"));

    assert!(fs.is_dir("/work/.rapport"));
    assert_eq!(
        assert_ok!(fs.read_to_string("/work/.rapport/work.toml")),
        "schema_version = 1\n"
    );
}

#[test]
fn in_memory_file_system_appends_lines() {
    let mut fs = InMemoryFileSystem::default();

    assert_ok!(fs.append_line("/work/journal.jsonl", "{\"one\":1}"));
    assert_ok!(fs.append_line("/work/journal.jsonl", "{\"two\":2}"));

    assert_eq!(
        assert_ok!(fs.read_to_string("/work/journal.jsonl")),
        "{\"one\":1}\n{\"two\":2}\n"
    );
}

#[test]
fn in_memory_file_system_removes_files() {
    let mut fs = InMemoryFileSystem::default();
    fs.add_file("/work/.rapport/work.toml");

    assert_ok!(fs.remove_file("/work/.rapport/work.toml"));

    assert!(!fs.is_file("/work/.rapport/work.toml"));
}

#[test]
fn rename_should_move_a_complete_directory_tree() {
    let mut fs = InMemoryFileSystem::default();
    fs.add_file_with_contents("/history/pending/work.toml", "version = 2\n");
    fs.add_file_with_contents("/history/pending/tasks/TASK_001.toml", "version = 1\n");

    assert_ok!(fs.rename("/history/pending", "/history/019f53"));

    assert!(
        !fs.exists("/history/pending"),
        "expecting atomic rename to remove the pending tree"
    );
    assert_eq!(
        assert_ok!(fs.read_to_string("/history/019f53/work.toml")),
        "version = 2\n",
        "expecting atomic rename to preserve file contents"
    );
    assert!(
        fs.is_file("/history/019f53/tasks/TASK_001.toml"),
        "expecting atomic rename to preserve nested Task files"
    );
}

#[test]
fn remove_dir_all_should_remove_every_descendant() {
    let mut fs = InMemoryFileSystem::default();
    fs.add_file("/history/019f53/work.toml");
    fs.add_file("/history/019f53/tasks/TASK_001.toml");

    assert_ok!(fs.remove_dir_all("/history/019f53"));

    assert!(
        !fs.exists("/history/019f53"),
        "expecting recursive removal to delete the selected record"
    );
    assert!(
        !fs.exists("/history/019f53/tasks/TASK_001.toml"),
        "expecting recursive removal to delete nested Task files"
    );
    assert!(
        fs.is_dir("/history"),
        "expecting recursive removal to preserve the parent history directory"
    );
}
