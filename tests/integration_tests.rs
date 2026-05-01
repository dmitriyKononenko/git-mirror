//! Integration tests — config persistence and copy logic.

use git_mirror::config::{Config, CopyGroup};
use std::fs;
use tempfile::TempDir;

// ── Config round-trip ─────────────────────────────────────────────────────────

#[test]
fn test_config_roundtrip_with_groups() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("config.toml");

    let original = Config {
        groups: vec![
            CopyGroup { source: "/src/a".into(), destination: "/dst/a".into() },
            CopyGroup { source: "/src/b".into(), destination: "/dst/b".into() },
        ],
    };
    original.save_to(&path).unwrap();
    let loaded = Config::load_from(&path).unwrap();
    assert_eq!(original, loaded);
}

#[test]
fn test_empty_config_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("config.toml");
    let original = Config::default();
    original.save_to(&path).unwrap();
    let loaded = Config::load_from(&path).unwrap();
    assert_eq!(original, loaded);
    assert!(loaded.groups.is_empty());
}

// ── Copy logic ────────────────────────────────────────────────────────────────

#[test]
fn test_copy_file_to_destination() {
    let src_dir = TempDir::new().unwrap();
    let dst_dir = TempDir::new().unwrap();

    let src_file = src_dir.path().join("hello.txt");
    fs::write(&src_file, "hello world").unwrap();

    let opts = fs_extra::file::CopyOptions::new().overwrite(true);
    fs_extra::file::copy(&src_file, dst_dir.path().join("hello.txt"), &opts).unwrap();

    assert_eq!(
        fs::read_to_string(dst_dir.path().join("hello.txt")).unwrap(),
        "hello world"
    );
}

#[test]
fn test_copy_directory_recursively() {
    let src_dir = TempDir::new().unwrap();
    let dst_dir = TempDir::new().unwrap();

    fs::create_dir_all(src_dir.path().join("sub")).unwrap();
    fs::write(src_dir.path().join("sub/file.txt"), "nested").unwrap();
    fs::write(src_dir.path().join("root.txt"), "root").unwrap();

    let opts = fs_extra::dir::CopyOptions::new().overwrite(true).copy_inside(true);
    fs_extra::dir::copy(src_dir.path(), dst_dir.path(), &opts).unwrap();

    let base = dst_dir.path().join(src_dir.path().file_name().unwrap());
    assert_eq!(fs::read_to_string(base.join("sub/file.txt")).unwrap(), "nested");
    assert_eq!(fs::read_to_string(base.join("root.txt")).unwrap(), "root");
}

#[test]
fn test_copy_overwrites_existing_file() {
    let src_dir = TempDir::new().unwrap();
    let dst_dir = TempDir::new().unwrap();

    let src_file = src_dir.path().join("data.txt");
    let dst_file = dst_dir.path().join("data.txt");
    fs::write(&src_file, "new content").unwrap();
    fs::write(&dst_file, "old content").unwrap();

    let opts = fs_extra::file::CopyOptions::new().overwrite(true);
    fs_extra::file::copy(&src_file, &dst_file, &opts).unwrap();

    assert_eq!(fs::read_to_string(&dst_file).unwrap(), "new content");
}
