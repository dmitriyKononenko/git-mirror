// git.rs — Git operations via libgit2 (no system git required)

use anyhow::{bail, Context, Result};
use git2::{Repository, StatusOptions};
use std::path::Path;

/// Represents a single changed file
#[derive(Clone, Debug)]
pub struct ChangedFile {
    /// Relative path from repo root e.g. "src/main.rs"
    pub relative_path: String,
    pub status: FileStatus,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FileStatus {
    Added,
    Modified,
    Deleted,
}

/// Opens a git repository at the given path.
pub fn open_repo(path: &str) -> Result<Repository> {
    Repository::open(path).context(format!("'{}' is not a valid git repository", path))
}

/// Returns files that are modified in the worktree (staged OR unstaged vs HEAD).
///
/// This is equivalent to `git status --short` — it shows what's dirty right now,
/// regardless of whether it's been committed. This is what we always sync.
///
/// WHY worktree instead of commit diff:
///   The user works on files, hits Sync → changed files go to destinations.
///   Commit history doesn't matter — only "what's different on disk right now".
pub fn get_worktree_changes(repo: &Repository) -> Result<Vec<ChangedFile>> {
    // `StatusOptions` controls what `repo.statuses()` looks at.
    // We want both: changes staged for commit AND changes not yet staged.
    let mut opts = StatusOptions::new();
    opts.include_untracked(false)  // don't show brand-new untracked files
        .include_ignored(false)    // respect .gitignore
        .recurse_untracked_dirs(false);

    let statuses = repo.statuses(Some(&mut opts)).context("Failed to get git status")?;

    let mut files = Vec::new();

    for entry in statuses.iter() {
        let flags = entry.status();

        // Combine staged (INDEX_*) and unstaged (WT_*) flags.
        // A file counts as changed if it's different in any way from HEAD.
        let is_added   = flags.intersects(git2::Status::INDEX_NEW | git2::Status::WT_NEW);
        let is_deleted = flags.intersects(git2::Status::INDEX_DELETED | git2::Status::WT_DELETED);
        let is_modified = flags.intersects(
            git2::Status::INDEX_MODIFIED | git2::Status::WT_MODIFIED |
            git2::Status::INDEX_RENAMED  | git2::Status::WT_RENAMED  |
            git2::Status::INDEX_TYPECHANGE
        );

        if !is_added && !is_deleted && !is_modified {
            continue; // skip unmodified entries
        }

        let path = entry.path().unwrap_or("").to_string();
        let status = if is_deleted {
            FileStatus::Deleted
        } else if is_added {
            FileStatus::Added
        } else {
            FileStatus::Modified
        };

        files.push(ChangedFile { relative_path: path, status });
    }

    Ok(files)
}

/// Returns every file tracked by git in HEAD — used for a full copy.
///
/// WHY walk the tree instead of the filesystem:
///   We only want files git knows about. Walking the filesystem would copy
///   build artifacts, .DS_Store, node_modules etc. even if they're gitignored.
pub fn get_all_tracked_files(repo: &Repository) -> Result<Vec<ChangedFile>> {
    let head = repo.head().context("Repo has no commits yet")?;
    let tree = head.peel_to_tree().context("Failed to peel HEAD to tree")?;

    let mut files = Vec::new();

    tree.walk(git2::TreeWalkMode::PreOrder, |root, entry| {
        if entry.kind() == Some(git2::ObjectType::Blob) {
            let name = entry.name().unwrap_or("");
            let path = if root.is_empty() {
                name.to_string()
            } else {
                format!("{}{}", root, name)
            };
            files.push(ChangedFile { relative_path: path, status: FileStatus::Added });
        }
        git2::TreeWalkResult::Ok
    })?;

    Ok(files)
}

/// Validates that a path is a readable git repository.
#[allow(dead_code)]
pub fn validate_repo(path: &str) -> Result<()> {
    if path.trim().is_empty() {
        bail!("Repository path cannot be empty");
    }
    if !Path::new(path).exists() {
        bail!("Path does not exist: {}", path);
    }
    open_repo(path)?;
    Ok(())
}

// ── Unit tests ────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use git2::{IndexAddOption, Repository, Signature};
    use std::{fs, path::Path};
    use tempfile::TempDir;

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn sig() -> Signature<'static> {
        Signature::now("Tester", "test@example.com").unwrap()
    }

    /// Init a bare-minimum git repo with a user identity configured.
    /// Without name+email, libgit2 refuses to create commits.
    fn init_repo(dir: &Path) -> Repository {
        let repo = Repository::init(dir).unwrap();
        let mut cfg = repo.config().unwrap();
        cfg.set_str("user.name", "Tester").unwrap();
        cfg.set_str("user.email", "test@example.com").unwrap();
        repo
    }

    /// Stage everything in the working tree and commit.
    fn commit_all(repo: &Repository, msg: &str) {
        let mut index = repo.index().unwrap();
        index.add_all(["*"].iter(), IndexAddOption::DEFAULT, None).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let s = sig();
        // Collect parents — empty Vec for the very first commit (no HEAD yet)
        let parents: Vec<git2::Commit> = repo
            .head()
            .ok()
            .and_then(|h| h.peel_to_commit().ok())
            .into_iter()
            .collect();
        let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
        repo.commit(Some("HEAD"), &s, &s, msg, &tree, &parent_refs).unwrap();
    }

    /// Create a repo with a committed initial set of files.
    fn setup_repo_with_files(dir: &Path) -> Repository {
        let repo = init_repo(dir);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(dir.join("src/lib.rs"), "pub fn hello() {}").unwrap();
        fs::write(dir.join("README.md"), "# Hello").unwrap();
        commit_all(&repo, "Initial commit");
        repo
    }

    // ── open_repo ─────────────────────────────────────────────────────────────

    #[test]
    fn test_open_repo_succeeds_on_valid_repo() {
        let tmp = TempDir::new().unwrap();
        Repository::init(tmp.path()).unwrap();
        assert!(open_repo(tmp.path().to_str().unwrap()).is_ok());
    }

    #[test]
    fn test_open_repo_fails_on_nonexistent_path() {
        let result = open_repo("/this/path/does/not/exist/at/all");
        assert!(result.is_err());
    }

    #[test]
    fn test_open_repo_fails_on_non_git_directory() {
        let tmp = TempDir::new().unwrap(); // a plain folder, no .git
        let result = open_repo(tmp.path().to_str().unwrap());
        assert!(result.is_err());
    }

    // ── get_all_tracked_files ─────────────────────────────────────────────────

    #[test]
    fn test_all_tracked_files_returns_committed_files() {
        let tmp = TempDir::new().unwrap();
        let repo = setup_repo_with_files(tmp.path());
        let files = get_all_tracked_files(&repo).unwrap();
        let paths: Vec<_> = files.iter().map(|f| f.relative_path.as_str()).collect();
        assert!(paths.contains(&"src/main.rs"));
        assert!(paths.contains(&"src/lib.rs"));
        assert!(paths.contains(&"README.md"));
    }

    #[test]
    fn test_all_tracked_files_excludes_untracked_files() {
        let tmp = TempDir::new().unwrap();
        let repo = setup_repo_with_files(tmp.path());
        // Write a new file but do NOT add/commit it
        fs::write(tmp.path().join("secret.txt"), "untracked").unwrap();
        let files = get_all_tracked_files(&repo).unwrap();
        let paths: Vec<_> = files.iter().map(|f| f.relative_path.as_str()).collect();
        // Untracked files must NOT appear — they're not in the git tree
        assert!(!paths.contains(&"secret.txt"));
    }

    #[test]
    fn test_all_tracked_files_preserves_nested_paths() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo(tmp.path());
        fs::create_dir_all(tmp.path().join("a/b/c")).unwrap();
        fs::write(tmp.path().join("a/b/c/deep.rs"), "deep").unwrap();
        commit_all(&repo, "deep file");
        let files = get_all_tracked_files(&repo).unwrap();
        let paths: Vec<_> = files.iter().map(|f| f.relative_path.as_str()).collect();
        assert!(paths.contains(&"a/b/c/deep.rs"));
    }

    // ── get_worktree_changes ──────────────────────────────────────────────────

    #[test]
    fn test_worktree_detects_modified_file() {
        let tmp = TempDir::new().unwrap();
        let repo = setup_repo_with_files(tmp.path());
        // Modify a tracked file without committing
        fs::write(tmp.path().join("src/main.rs"), "fn main() { println!(\"hi\"); }").unwrap();
        let changes = get_worktree_changes(&repo).unwrap();
        let modified: Vec<_> = changes.iter()
            .filter(|f| f.relative_path == "src/main.rs")
            .collect();
        assert_eq!(modified.len(), 1, "modified file should appear in worktree changes");
        assert_eq!(modified[0].status, FileStatus::Modified);
    }

    #[test]
    fn test_worktree_detects_deleted_file() {
        let tmp = TempDir::new().unwrap();
        let repo = setup_repo_with_files(tmp.path());
        // Delete a tracked file without `git rm` — shows as WT_DELETED
        fs::remove_file(tmp.path().join("README.md")).unwrap();
        let changes = get_worktree_changes(&repo).unwrap();
        let deleted: Vec<_> = changes.iter()
            .filter(|f| f.relative_path == "README.md")
            .collect();
        assert_eq!(deleted.len(), 1, "deleted file should appear in worktree changes");
        assert_eq!(deleted[0].status, FileStatus::Deleted);
    }

    #[test]
    fn test_worktree_detects_staged_new_file() {
        let tmp = TempDir::new().unwrap();
        let repo = setup_repo_with_files(tmp.path());
        // Write a new file and stage it (INDEX_NEW)
        fs::write(tmp.path().join("new_feature.rs"), "pub fn new() {}").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("new_feature.rs")).unwrap();
        index.write().unwrap();
        let changes = get_worktree_changes(&repo).unwrap();
        let added: Vec<_> = changes.iter()
            .filter(|f| f.relative_path == "new_feature.rs")
            .collect();
        assert_eq!(added.len(), 1, "staged new file should appear in worktree changes");
        assert_eq!(added[0].status, FileStatus::Added);
    }

    #[test]
    fn test_worktree_is_empty_when_nothing_changed() {
        let tmp = TempDir::new().unwrap();
        let repo = setup_repo_with_files(tmp.path());
        // Nothing changed after commit
        let changes = get_worktree_changes(&repo).unwrap();
        assert!(changes.is_empty(), "clean worktree should have no changes");
    }

    #[test]
    fn test_worktree_ignores_untracked_files() {
        let tmp = TempDir::new().unwrap();
        let repo = setup_repo_with_files(tmp.path());
        // New file, not staged — should NOT appear (include_untracked = false)
        fs::write(tmp.path().join("untracked.txt"), "nope").unwrap();
        let changes = get_worktree_changes(&repo).unwrap();
        assert!(
            changes.iter().all(|f| f.relative_path != "untracked.txt"),
            "untracked file must not appear in worktree changes"
        );
    }
}
