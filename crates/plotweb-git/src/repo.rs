use git2::{Oid, Repository, Signature};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::path::Path;

use crate::error::Result;

/// Initialize a new git repo with an initial empty commit on "main".
pub fn init_repo(path: &Path) -> Result<Repository> {
    let repo = Repository::init(path)?;

    // Point HEAD at "main" BEFORE the first commit so the commit
    // creates refs/heads/main (not the default "master").
    repo.set_head("refs/heads/main")?;

    // Ignore the sibling temp files used by atomic_write so a `*.tmp` left by a
    // crash mid-write is never picked up by commit_all's add_all(".").
    std::fs::write(path.join(".gitignore"), "*.tmp\n")?;

    // Create initial empty commit on the (unborn) main branch
    let sig = default_signature();
    let tree_id = repo.index()?.write_tree()?;
    {
        let tree = repo.find_tree(tree_id)?;
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])?;
    }

    Ok(repo)
}

/// Stage all changes and create a commit.
pub fn commit_all(repo: &Repository, message: &str) -> Result<()> {
    let mut index = repo.index()?;
    // Use "." to match all files recursively — "*" only matches root-level
    // files because fnmatch treats "/" as a boundary.
    index.add_all(["."].iter(), git2::IndexAddOption::DEFAULT, None)?;
    index.write()?;

    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;
    let sig = default_signature();

    // Handle both unborn HEAD (first real commit) and normal case.
    match repo.head() {
        Ok(head) => {
            let parent = head.peel_to_commit()?;
            repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent])?;
        }
        Err(_) => {
            // HEAD is unborn — create a root commit
            repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[])?;
        }
    }
    Ok(())
}

fn default_signature() -> Signature<'static> {
    Signature::now("PlotWeb", "plotweb@local").unwrap()
}

/// Read and deserialize JSON from a file.
pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let data = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&data)?)
}

/// Atomically write `data` to `path`: write a sibling temp file, fsync it, then
/// rename over the target. Rename within a directory is atomic on POSIX, so a
/// crash can leave a stray `*.tmp` but never a truncated/half-written target —
/// which for `book.json` would make the entire book unreadable.
fn atomic_write(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    // Sibling temp keyed off the full filename (e.g. `book.json.tmp`,
    // `chapter.md.tmp`) so distinct targets never collide on a shared temp name.
    let mut tmp_os = path.as_os_str().to_os_string();
    tmp_os.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp_os);

    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// Atomically write a UTF-8 string to a file. See [`atomic_write`].
pub fn write_text(path: &Path, contents: &str) -> Result<()> {
    atomic_write(path, contents.as_bytes())?;
    Ok(())
}

/// Serialize and atomically write JSON to a file.
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let data = serde_json::to_string_pretty(value)?;
    atomic_write(path, data.as_bytes())?;
    Ok(())
}

/// Read and deserialize JSON from a file at a specific git commit.
pub fn read_json_at_commit<T: DeserializeOwned>(repo: &Repository, oid: Oid, path: &str) -> Result<T> {
    let commit = repo.find_commit(oid)?;
    let tree = commit.tree()?;
    let entry = tree.get_path(std::path::Path::new(path))?;
    let blob = repo.find_blob(entry.id())?;
    let content = std::str::from_utf8(blob.content())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(serde_json::from_str(content)?)
}

/// Read a raw text file at a specific git commit (for .md content files).
pub fn read_text_at_commit(repo: &Repository, oid: Oid, path: &str) -> Result<String> {
    let commit = repo.find_commit(oid)?;
    let tree = commit.tree()?;
    let entry = tree.get_path(std::path::Path::new(path))?;
    let blob = repo.find_blob(entry.id())?;
    let content = std::str::from_utf8(blob.content())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(content.to_string())
}

/// Return the HEAD commit OID for a repository.
pub fn head_oid(repo: &Repository) -> Result<Oid> {
    let head = repo.head()?;
    let commit = head.peel_to_commit()?;
    Ok(commit.id())
}

/// Diff a commit against its parent. Returns changed .md files with their hunks.
pub fn diff_commit(repo: &Repository, commit_oid: Oid) -> Result<Vec<(String, String, Vec<(Vec<(char, String)>,)>)>> {
    let commit = repo.find_commit(commit_oid)?;
    let new_tree = commit.tree()?;

    let old_tree = if commit.parent_count() > 0 {
        Some(commit.parent(0)?.tree()?)
    } else {
        None
    };

    let diff = repo.diff_tree_to_tree(
        old_tree.as_ref(),
        Some(&new_tree),
        None,
    )?;

    let mut results: Vec<(String, String, Vec<(Vec<(char, String)>,)>)> = Vec::new();

    for (idx, delta) in diff.deltas().enumerate() {
        let path = delta.new_file().path()
            .or_else(|| delta.old_file().path())
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_string();

        // Only include chapter .md files
        if !path.starts_with("chapters/") || !path.ends_with(".md") {
            continue;
        }

        let chapter_id = path
            .trim_start_matches("chapters/")
            .trim_end_matches(".md")
            .to_string();

        let change_type = match delta.status() {
            git2::Delta::Added => "added",
            git2::Delta::Deleted => "deleted",
            git2::Delta::Modified => "modified",
            _ => "modified",
        };

        let mut hunks: Vec<(Vec<(char, String)>,)> = Vec::new();

        if let Ok(patch) = git2::Patch::from_diff(&diff, idx) {
            if let Some(ref patch) = patch {
                for h in 0..patch.num_hunks() {
                    let mut lines = Vec::new();
                    let num_lines = patch.num_lines_in_hunk(h).unwrap_or(0);
                    for l in 0..num_lines {
                        if let Ok(line) = patch.line_in_hunk(h, l) {
                            let origin = match line.origin() {
                                '+' => '+',
                                '-' => '-',
                                _ => ' ',
                            };
                            let content = std::str::from_utf8(line.content())
                                .unwrap_or("")
                                .to_string();
                            lines.push((origin, content));
                        }
                    }
                    if !lines.is_empty() {
                        hunks.push((lines,));
                    }
                }
            }
        }

        results.push((chapter_id, change_type.to_string(), hunks));
    }

    Ok(results)
}

/// Walk commit history from HEAD, returning (oid_hex, message, unix_timestamp).
pub fn list_commits(repo: &Repository, limit: usize, offset: usize) -> Result<Vec<(String, String, i64)>> {
    let mut revwalk = repo.revwalk()?;
    revwalk.push_head()?;
    revwalk.set_sorting(git2::Sort::TIME)?;

    let commits: Vec<_> = revwalk
        .skip(offset)
        .take(limit)
        .filter_map(|oid| oid.ok())
        .filter_map(|oid| {
            let commit = repo.find_commit(oid).ok()?;
            Some((
                oid.to_string(),
                commit.message().unwrap_or("").to_string(),
                commit.time().seconds(),
            ))
        })
        .collect();
    Ok(commits)
}

/// Restore the working directory to match a previous commit's tree,
/// then create a new commit so history is preserved.
pub fn restore_to_commit(repo: &Repository, target_oid: Oid) -> Result<()> {
    let target_commit = repo.find_commit(target_oid)?;
    let target_tree = target_commit.tree()?;

    repo.checkout_tree(
        target_tree.as_object(),
        Some(
            git2::build::CheckoutBuilder::new()
                .force()
                .remove_untracked(true),
        ),
    )?;

    commit_all(
        repo,
        &format!(
            "Restored to: {}",
            target_commit.message().unwrap_or("previous version")
        ),
    )?;
    Ok(())
}
