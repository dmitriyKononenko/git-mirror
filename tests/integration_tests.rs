//! Integration tests — full sync workflow end-to-end.
//!
//! These tests exercise the full pipeline: git repo → worktree changes → sync → destination.
//! They intentionally avoid testing egui/UI code; that layer is thin and
//! just dispatches to the functions tested here.

use git2::{IndexAddOption, Repository, Signature};
use git_mirror::{
    config::{Config, Destination},
    git,
    sync,
};
use std::{fs, path::Path};
use tempfile::TempDir;

// ── Shared helpers ─────────────────────────────────────────────────────────────

fn sig() -> Signature<'static> {
    Signature::now("Tester", "test@example.com").unwrap()
}

fn init_repo(dir: &Path) -> Repository {
    let repo = Repository::init(dir).unwrap();
    let mut cfg = repo.config().unwrap();
    cfg.set_str("user.name", "Tester").unwrap();
    cfg.set_str("user.email", "test@example.com").unwrap();
    repo
}

fn commit_all(repo: &Repository, msg: &str) {
    let mut index = repo.index().unwrap();
    index.add_all(["*"].iter(), IndexAddOption::DEFAULT, None).unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let s = sig();
    let parents: Vec<git2::Commit> = repo
        .head().ok()
        .and_then(|h| h.peel_to_commit().ok())
        .into_iter().collect();
    let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
    repo.commit(Some("HEAD"), &s, &s, msg, &tree, &parent_refs).unwrap();
}

/// Creates a repo with three committed files:
///   src/main.rs, src/lib.rs, README.md
fn setup_repo(dir: &Path) -> Repository {
    let repo = init_repo(dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();
    fs::write(dir.join("src/lib.rs"), "pub fn hello() {}").unwrap();
    fs::write(dir.join("README.md"), "# Hello").unwrap();
    commit_all(&repo, "Initial commit");
    repo
}

fn dest(path: &Path) -> Destination {
    Destination { label: "Test".into(), path: path.to_string_lossy().into_owned() }
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

// ── Feature: empty destination → full copy ────────────────────────────────────

#[test]
fn test_empty_dest_gets_full_copy_of_all_tracked_files() {
    let src_dir = TempDir::new().unwrap();
    let dst_dir = TempDir::new().unwrap();
    let repo = setup_repo(src_dir.path());

    let all_files = git::get_all_tracked_files(&repo).unwrap();
    assert!(sync::is_destination_empty(dst_dir.path().to_str().unwrap()));

    sync::sync_files(src_dir.path().to_str().unwrap(), &dest(dst_dir.path()), &all_files);

    assert!(dst_dir.path().join("src/main.rs").exists());
    assert!(dst_dir.path().join("src/lib.rs").exists());
    assert!(dst_dir.path().join("README.md").exists());
}

#[test]
fn test_full_copy_preserves_file_contents() {
    let src_dir = TempDir::new().unwrap();
    let dst_dir = TempDir::new().unwrap();
    let repo = setup_repo(src_dir.path());

    let all_files = git::get_all_tracked_files(&repo).unwrap();
    sync::sync_files(src_dir.path().to_str().unwrap(), &dest(dst_dir.path()), &all_files);

    assert_eq!(read(&dst_dir.path().join("src/main.rs")), "fn main() {}");
    assert_eq!(read(&dst_dir.path().join("README.md")), "# Hello");
}

// ── Feature: non-empty destination → only worktree changes ────────────────────

#[test]
fn test_nonempty_dest_only_receives_worktree_changes() {
    let src_dir = TempDir::new().unwrap();
    let dst_dir = TempDir::new().unwrap();
    let repo = setup_repo(src_dir.path());

    // Simulate a previous full copy: all files already in destination
    fs::create_dir_all(dst_dir.path().join("src")).unwrap();
    fs::write(dst_dir.path().join("src/main.rs"), "fn main() {}").unwrap();
    fs::write(dst_dir.path().join("src/lib.rs"), "pub fn hello() {}").unwrap();
    fs::write(dst_dir.path().join("README.md"), "# Hello").unwrap();

    // Now modify only main.rs in the worktree (not committed)
    fs::write(src_dir.path().join("src/main.rs"), "fn main() { println!(\"updated\"); }").unwrap();

    let changes = git::get_worktree_changes(&repo).unwrap();
    assert_eq!(changes.len(), 1, "only one file changed");
    assert_eq!(changes[0].relative_path, "src/main.rs");

    sync::sync_files(src_dir.path().to_str().unwrap(), &dest(dst_dir.path()), &changes);

    // Changed file was synced
    assert_eq!(
        read(&dst_dir.path().join("src/main.rs")),
        "fn main() { println!(\"updated\"); }"
    );
    // Unchanged file was NOT touched
    assert_eq!(
        read(&dst_dir.path().join("src/lib.rs")),
        "pub fn hello() {}"
    );
}

// ── Feature: force sync → all tracked files regardless ────────────────────────

#[test]
fn test_force_sync_copies_all_tracked_files_to_nonempty_dest() {
    let src_dir = TempDir::new().unwrap();
    let dst_dir = TempDir::new().unwrap();
    let repo = setup_repo(src_dir.path());

    // Destination has some stale content (simulates drift)
    fs::create_dir_all(dst_dir.path().join("src")).unwrap();
    fs::write(dst_dir.path().join("src/main.rs"), "stale content").unwrap();

    let all_files = git::get_all_tracked_files(&repo).unwrap();
    sync::sync_files(src_dir.path().to_str().unwrap(), &dest(dst_dir.path()), &all_files);

    // Force sync must overwrite stale files
    assert_eq!(read(&dst_dir.path().join("src/main.rs")), "fn main() {}");
    assert_eq!(read(&dst_dir.path().join("README.md")), "# Hello");
}

// ── Feature: file structure is preserved ──────────────────────────────────────

#[test]
fn test_deeply_nested_file_structure_is_preserved() {
    let src_dir = TempDir::new().unwrap();
    let dst_dir = TempDir::new().unwrap();
    let repo = init_repo(src_dir.path());

    fs::create_dir_all(src_dir.path().join("a/b/c/d")).unwrap();
    fs::write(src_dir.path().join("a/b/c/d/deep.rs"), "deep content").unwrap();
    commit_all(&repo, "deep file");

    let all_files = git::get_all_tracked_files(&repo).unwrap();
    sync::sync_files(src_dir.path().to_str().unwrap(), &dest(dst_dir.path()), &all_files);

    assert_eq!(
        read(&dst_dir.path().join("a/b/c/d/deep.rs")),
        "deep content",
        "directory structure must be replicated exactly"
    );
}

// ── Feature: deleted files are removed from destination ───────────────────────

#[test]
fn test_deleted_file_is_removed_from_destination() {
    let src_dir = TempDir::new().unwrap();
    let dst_dir = TempDir::new().unwrap();
    let repo = setup_repo(src_dir.path());

    // Initial full copy
    let all_files = git::get_all_tracked_files(&repo).unwrap();
    sync::sync_files(src_dir.path().to_str().unwrap(), &dest(dst_dir.path()), &all_files);
    assert!(dst_dir.path().join("README.md").exists());

    // Delete a tracked file in source worktree
    fs::remove_file(src_dir.path().join("README.md")).unwrap();

    let changes = git::get_worktree_changes(&repo).unwrap();
    sync::sync_files(src_dir.path().to_str().unwrap(), &dest(dst_dir.path()), &changes);

    assert!(!dst_dir.path().join("README.md").exists(), "deleted file must be removed from dest");
    // Other files must still be intact
    assert!(dst_dir.path().join("src/main.rs").exists());
}

// ── SAFETY: source files must never be modified or deleted ────────────────────

#[test]
fn test_source_files_byte_identical_after_normal_sync() {
    let src_dir = TempDir::new().unwrap();
    let dst_dir = TempDir::new().unwrap();
    let repo = setup_repo(src_dir.path());

    let before_main   = read(&src_dir.path().join("src/main.rs"));
    let before_lib    = read(&src_dir.path().join("src/lib.rs"));
    let before_readme = read(&src_dir.path().join("README.md"));

    let all_files = git::get_all_tracked_files(&repo).unwrap();
    sync::sync_files(src_dir.path().to_str().unwrap(), &dest(dst_dir.path()), &all_files);

    assert_eq!(read(&src_dir.path().join("src/main.rs")), before_main,   "source must not be modified");
    assert_eq!(read(&src_dir.path().join("src/lib.rs")),  before_lib,    "source must not be modified");
    assert_eq!(read(&src_dir.path().join("README.md")),   before_readme, "source must not be modified");
}

#[test]
fn test_source_files_byte_identical_after_force_sync() {
    let src_dir = TempDir::new().unwrap();
    let dst_dir = TempDir::new().unwrap();
    let repo = setup_repo(src_dir.path());

    // Modify worktree and force-sync
    fs::write(src_dir.path().join("src/main.rs"), "modified in worktree").unwrap();
    let before = read(&src_dir.path().join("src/main.rs"));

    let all_files = git::get_all_tracked_files(&repo).unwrap();
    sync::sync_files(src_dir.path().to_str().unwrap(), &dest(dst_dir.path()), &all_files);

    assert_eq!(read(&src_dir.path().join("src/main.rs")), before, "source must not be modified");
}

#[test]
fn test_source_directory_entry_count_unchanged_after_sync() {
    let src_dir = TempDir::new().unwrap();
    let dst_dir = TempDir::new().unwrap();
    let repo = setup_repo(src_dir.path());

    // Count entries in src root before
    let count_before = fs::read_dir(src_dir.path()).unwrap().count();

    let all_files = git::get_all_tracked_files(&repo).unwrap();
    sync::sync_files(src_dir.path().to_str().unwrap(), &dest(dst_dir.path()), &all_files);

    let count_after = fs::read_dir(src_dir.path()).unwrap().count();
    assert_eq!(count_before, count_after, "sync must not create or delete files in source");
}

// ── SAFETY: files outside the destination must not be touched ─────────────────

#[test]
fn test_bystander_directory_completely_untouched() {
    let src_dir    = TempDir::new().unwrap();
    let dst_dir    = TempDir::new().unwrap();
    let bystander  = TempDir::new().unwrap();
    let repo = setup_repo(src_dir.path());

    fs::write(bystander.path().join("sensitive.txt"), "confidential").unwrap();
    fs::write(bystander.path().join("data.json"), r#"{"important": true}"#).unwrap();

    let all_files = git::get_all_tracked_files(&repo).unwrap();
    sync::sync_files(src_dir.path().to_str().unwrap(), &dest(dst_dir.path()), &all_files);

    assert_eq!(read(&bystander.path().join("sensitive.txt")), "confidential");
    assert_eq!(read(&bystander.path().join("data.json")), r#"{"important": true}"#);
    assert!(!bystander.path().join("src/main.rs").exists(), "source files must not appear in bystander dir");
    assert!(!bystander.path().join("README.md").exists());
}

#[test]
fn test_bystander_file_count_unchanged_after_sync() {
    let src_dir   = TempDir::new().unwrap();
    let dst_dir   = TempDir::new().unwrap();
    let bystander = TempDir::new().unwrap();
    let repo = setup_repo(src_dir.path());

    fs::write(bystander.path().join("a.txt"), "a").unwrap();
    fs::write(bystander.path().join("b.txt"), "b").unwrap();
    let count_before = fs::read_dir(bystander.path()).unwrap().count();

    let all_files = git::get_all_tracked_files(&repo).unwrap();
    sync::sync_files(src_dir.path().to_str().unwrap(), &dest(dst_dir.path()), &all_files);

    let count_after = fs::read_dir(bystander.path()).unwrap().count();
    assert_eq!(count_before, count_after, "sync must not change files in unrelated directories");
}

// ── Feature: per-destination sync isolation ───────────────────────────────────

#[test]
fn test_syncing_one_dest_does_not_affect_other_dest() {
    let src_dir  = TempDir::new().unwrap();
    let dst1_dir = TempDir::new().unwrap();
    let dst2_dir = TempDir::new().unwrap();
    let repo = setup_repo(src_dir.path());

    // Full copy to both destinations
    let all_files = git::get_all_tracked_files(&repo).unwrap();
    sync::sync_files(src_dir.path().to_str().unwrap(), &dest(dst1_dir.path()), &all_files);
    sync::sync_files(src_dir.path().to_str().unwrap(), &dest(dst2_dir.path()), &all_files);

    // Manually corrupt dst2 to simulate drift
    fs::write(dst2_dir.path().join("README.md"), "corrupted").unwrap();

    // Sync only dst1 with a worktree change
    fs::write(src_dir.path().join("src/main.rs"), "fn main() { /* updated */ }").unwrap();
    let changes = git::get_worktree_changes(&repo).unwrap();
    sync::sync_files(src_dir.path().to_str().unwrap(), &dest(dst1_dir.path()), &changes);

    // dst1 must have the update
    assert_eq!(
        read(&dst1_dir.path().join("src/main.rs")),
        "fn main() { /* updated */ }"
    );
    // dst2 must be completely unaffected — still has its corrupted README
    assert_eq!(
        read(&dst2_dir.path().join("README.md")),
        "corrupted",
        "syncing dst1 must not affect dst2"
    );
    // dst2's main.rs must also be untouched
    assert_eq!(
        read(&dst2_dir.path().join("src/main.rs")),
        "fn main() {}",
        "dst2 main.rs must not be updated when only dst1 was synced"
    );
}

// ── Feature: config persistence ───────────────────────────────────────────────

#[test]
fn test_config_with_destinations_persists_across_save_load() {
    let tmp = TempDir::new().unwrap();
    let config_path = tmp.path().join("config.toml");

    let src_dir = TempDir::new().unwrap();
    let dst_dir = TempDir::new().unwrap();

    let config = Config {
        source_repo: Some(src_dir.path().to_string_lossy().into_owned()),
        destinations: vec![
            Destination {
                label: "Production".into(),
                path: dst_dir.path().to_string_lossy().into_owned(),
            },
        ],
    };

    config.save_to(&config_path).unwrap();
    let loaded = Config::load_from(&config_path).unwrap();

    assert_eq!(loaded.source_repo, config.source_repo);
    assert_eq!(loaded.destinations.len(), 1);
    assert_eq!(loaded.destinations[0].label, "Production");
    assert_eq!(loaded.destinations[0].path, config.destinations[0].path);
}
