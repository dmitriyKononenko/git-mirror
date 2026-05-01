// sync.rs — File copy logic
//
// This module handles copying changed files from source → destinations
// while preserving the directory structure.

use crate::config::Destination;
use crate::git::ChangedFile;
use crate::git::FileStatus;
use anyhow::{Context, Result};
use std::path::Path;

/// Result of a sync operation for one destination
pub struct SyncResult {
    #[allow(dead_code)]
    pub destination_label: String,
    pub files_copied: usize,
    pub files_deleted: usize,
    pub errors: Vec<String>,
}

/// Copy a list of changed files from the source repo root into a destination folder.
/// Preserves the relative directory structure.
///
/// Example:
///   source_root = "/repos/my-app"
///   dest_root   = "/deploy/server1"
///   file        = "src/api/routes.rs"
///
///   copies: /repos/my-app/src/api/routes.rs
///       to: /deploy/server1/src/api/routes.rs
pub fn sync_files(
    source_root: &str,
    dest: &Destination,
    files: &[ChangedFile],
) -> SyncResult {
    let mut result = SyncResult {
        destination_label: dest.label.clone(),
        files_copied: 0,
        files_deleted: 0,
        errors: Vec::new(),
    };

    for file in files {
        let outcome = match file.status {
            FileStatus::Deleted => delete_file(dest, &file.relative_path),
            _ => copy_file(source_root, dest, &file.relative_path),
        };

        // Match on the Result — like try/catch but exhaustive.
        // In Rust you MUST handle both Ok and Err — the compiler enforces this.
        match outcome {
            Ok(was_deleted) => {
                if was_deleted {
                    result.files_deleted += 1;
                } else {
                    result.files_copied += 1;
                }
            }
            Err(e) => {
                // Collect errors rather than aborting — we want to sync as many
                // files as possible even if a few fail.
                result.errors.push(format!("{}: {}", file.relative_path, e));
            }
        }
    }

    result
}

/// Copy a single file, creating any missing parent directories.
/// Returns Ok(false) to indicate a copy (not deletion).
fn copy_file(source_root: &str, dest: &Destination, relative_path: &str) -> Result<bool> {
    let source_file = Path::new(source_root).join(relative_path);
    let dest_file = Path::new(&dest.path).join(relative_path);

    // Create parent directories if they don't exist (like `mkdir -p` for the parent)
    if let Some(parent) = dest_file.parent() {
        std::fs::create_dir_all(parent)
            .context(format!("Failed to create directories for {:?}", parent))?;
    }

    std::fs::copy(&source_file, &dest_file)
        .context(format!("Failed to copy {:?} to {:?}", source_file, dest_file))?;

    Ok(false)
}

/// Delete a file from the destination if it was deleted in source.
/// Returns Ok(true) to indicate a deletion.
fn delete_file(dest: &Destination, relative_path: &str) -> Result<bool> {
    let dest_file = Path::new(&dest.path).join(relative_path);

    // Only attempt deletion if the file actually exists in destination.
    // `dest_file.exists()` would panic on permission errors — `try_exists` is safer.
    if dest_file.try_exists().unwrap_or(false) {
        std::fs::remove_file(&dest_file)
            .context(format!("Failed to delete {:?}", dest_file))?;
    }

    Ok(true)
}

/// Check if a destination folder is empty (used to decide full-copy vs diff-copy).
pub fn is_destination_empty(path: &str) -> bool {
    let dest = Path::new(path);
    if !dest.exists() {
        return true;
    }
    // `read_dir` returns an iterator of entries; `.next().is_none()` checks if empty
    std::fs::read_dir(dest)
        .map(|mut d| d.next().is_none())
        .unwrap_or(true)
}

// ── Unit tests ────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Destination;
    use crate::git::{ChangedFile, FileStatus};
    use std::fs;
    use tempfile::TempDir;

    fn dest(path: &std::path::Path) -> Destination {
        Destination { label: "Test".into(), path: path.to_string_lossy().into_owned() }
    }

    fn changed(path: &str, status: FileStatus) -> ChangedFile {
        ChangedFile { relative_path: path.to_string(), status }
    }

    // ── is_destination_empty ──────────────────────────────────────────────────

    #[test]
    fn test_empty_dir_is_considered_empty() {
        let tmp = TempDir::new().unwrap();
        assert!(is_destination_empty(tmp.path().to_str().unwrap()));
    }

    #[test]
    fn test_nonexistent_dir_is_considered_empty() {
        assert!(is_destination_empty("/this/path/does/not/exist/at/all"));
    }

    #[test]
    fn test_dir_with_files_is_not_empty() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("file.txt"), "content").unwrap();
        assert!(!is_destination_empty(tmp.path().to_str().unwrap()));
    }

    // ── sync_files ────────────────────────────────────────────────────────────

    #[test]
    fn test_sync_copies_file_preserving_structure() {
        let src = TempDir::new().unwrap();
        let dst = TempDir::new().unwrap();

        fs::create_dir_all(src.path().join("src/api")).unwrap();
        fs::write(src.path().join("src/api/routes.rs"), "routes").unwrap();

        let result = sync_files(
            src.path().to_str().unwrap(),
            &dest(dst.path()),
            &[changed("src/api/routes.rs", FileStatus::Modified)],
        );

        assert_eq!(result.files_copied, 1);
        assert!(result.errors.is_empty());
        // File must exist at the same relative path in destination
        assert_eq!(
            fs::read_to_string(dst.path().join("src/api/routes.rs")).unwrap(),
            "routes"
        );
    }

    #[test]
    fn test_sync_removes_deleted_file_from_dest() {
        let src = TempDir::new().unwrap();
        let dst = TempDir::new().unwrap();

        // Pre-populate destination with a file that was deleted in source
        fs::write(dst.path().join("old.rs"), "old content").unwrap();

        let result = sync_files(
            src.path().to_str().unwrap(),
            &dest(dst.path()),
            &[changed("old.rs", FileStatus::Deleted)],
        );

        assert_eq!(result.files_deleted, 1);
        assert!(result.errors.is_empty());
        assert!(!dst.path().join("old.rs").exists(), "deleted file must be removed from dest");
    }

    #[test]
    fn test_sync_tolerates_already_absent_deleted_file() {
        let src = TempDir::new().unwrap();
        let dst = TempDir::new().unwrap();
        // The file doesn't exist in dest — deletion should still succeed silently
        let result = sync_files(
            src.path().to_str().unwrap(),
            &dest(dst.path()),
            &[changed("ghost.rs", FileStatus::Deleted)],
        );
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_sync_creates_missing_parent_directories() {
        let src = TempDir::new().unwrap();
        let dst = TempDir::new().unwrap();

        fs::create_dir_all(src.path().join("a/b/c")).unwrap();
        fs::write(src.path().join("a/b/c/deep.rs"), "deep").unwrap();

        sync_files(
            src.path().to_str().unwrap(),
            &dest(dst.path()),
            &[changed("a/b/c/deep.rs", FileStatus::Added)],
        );

        assert!(dst.path().join("a/b/c/deep.rs").exists());
    }

    // ── SOURCE SAFETY: source files must never be modified or deleted ─────────

    #[test]
    fn test_source_files_unchanged_after_sync() {
        let src = TempDir::new().unwrap();
        let dst = TempDir::new().unwrap();

        fs::create_dir_all(src.path().join("src")).unwrap();
        fs::write(src.path().join("src/main.rs"), "original content").unwrap();
        fs::write(src.path().join("README.md"), "original readme").unwrap();

        sync_files(
            src.path().to_str().unwrap(),
            &dest(dst.path()),
            &[
                changed("src/main.rs", FileStatus::Modified),
                changed("README.md", FileStatus::Modified),
            ],
        );

        // Source files must be byte-for-byte identical after sync
        assert_eq!(
            fs::read_to_string(src.path().join("src/main.rs")).unwrap(),
            "original content",
            "sync must never modify source files"
        );
        assert_eq!(
            fs::read_to_string(src.path().join("README.md")).unwrap(),
            "original readme",
            "sync must never modify source files"
        );
    }

    #[test]
    fn test_source_files_not_deleted_after_sync() {
        let src = TempDir::new().unwrap();
        let dst = TempDir::new().unwrap();

        // Put the file in both src and dst
        fs::write(src.path().join("keep.rs"), "keep me").unwrap();
        fs::write(dst.path().join("keep.rs"), "keep me").unwrap();

        // Sync a deletion — only the DESTINATION copy should be removed
        sync_files(
            src.path().to_str().unwrap(),
            &dest(dst.path()),
            &[changed("keep.rs", FileStatus::Deleted)],
        );

        assert!(
            src.path().join("keep.rs").exists(),
            "sync must never delete files from the source"
        );
        assert!(
            !dst.path().join("keep.rs").exists(),
            "sync must delete files from the destination"
        );
    }

    // ── BYSTANDER SAFETY: files outside destination must not be touched ───────

    #[test]
    fn test_files_outside_destination_not_touched() {
        let src = TempDir::new().unwrap();
        let dst = TempDir::new().unwrap();
        let bystander = TempDir::new().unwrap();

        // Files in a completely separate directory
        fs::write(bystander.path().join("sensitive.txt"), "do not touch").unwrap();
        fs::write(bystander.path().join("data.json"), "{\"key\":\"value\"}").unwrap();

        fs::create_dir_all(src.path().join("src")).unwrap();
        fs::write(src.path().join("src/main.rs"), "fn main(){}").unwrap();

        sync_files(
            src.path().to_str().unwrap(),
            &dest(dst.path()),
            &[changed("src/main.rs", FileStatus::Added)],
        );

        // Bystander files must be completely untouched
        assert_eq!(
            fs::read_to_string(bystander.path().join("sensitive.txt")).unwrap(),
            "do not touch"
        );
        assert_eq!(
            fs::read_to_string(bystander.path().join("data.json")).unwrap(),
            "{\"key\":\"value\"}"
        );

        // Source files must not have appeared in the bystander directory
        assert!(!bystander.path().join("src/main.rs").exists());
    }

    #[test]
    fn test_sync_does_not_write_outside_destination_root() {
        let src = TempDir::new().unwrap();
        let dst = TempDir::new().unwrap();

        fs::write(src.path().join("file.rs"), "content").unwrap();

        sync_files(
            src.path().to_str().unwrap(),
            &dest(dst.path()),
            &[changed("file.rs", FileStatus::Added)],
        );

        // Only the destination should contain the file
        assert!(dst.path().join("file.rs").exists());
        assert!(!src.path().join("file.rs").metadata().unwrap().permissions().readonly());
        // Confirm no files written to src
        let src_entries: Vec<_> = fs::read_dir(src.path()).unwrap().collect();
        assert_eq!(src_entries.len(), 1, "sync must not create extra files in source");
    }

    // ── Multiple files & error isolation ─────────────────────────────────────

    #[test]
    fn test_sync_continues_after_individual_file_error() {
        let src = TempDir::new().unwrap();
        let dst = TempDir::new().unwrap();

        // Only "good.rs" exists in source — "missing.rs" will fail to copy
        fs::write(src.path().join("good.rs"), "good").unwrap();

        let result = sync_files(
            src.path().to_str().unwrap(),
            &dest(dst.path()),
            &[
                changed("missing.rs", FileStatus::Modified), // will error
                changed("good.rs", FileStatus::Modified),    // must still succeed
            ],
        );

        assert_eq!(result.files_copied, 1, "good file should still be copied");
        assert_eq!(result.errors.len(), 1, "one error for the missing file");
        assert!(dst.path().join("good.rs").exists());
    }
}
