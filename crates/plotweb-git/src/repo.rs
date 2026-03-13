use git2::{Repository, Signature};
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

/// Serialize and write JSON to a file.
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let data = serde_json::to_string_pretty(value)?;
    std::fs::write(path, data)?;
    Ok(())
}
