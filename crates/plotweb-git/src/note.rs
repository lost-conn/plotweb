use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::book;
use crate::error::{GitStoreError, Result};
use crate::repo;

/// On-disk representation of a note JSON file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteJson {
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub color: Option<String>,
    pub created_at: String,
}

/// On-disk representation of notes.json (tree structure).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotesTreeJson {
    #[serde(default)]
    pub root_order: Vec<String>,
    #[serde(default)]
    pub children: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub collapsed: Vec<String>,
}

impl Default for NotesTreeJson {
    fn default() -> Self {
        Self {
            root_order: Vec::new(),
            children: HashMap::new(),
            collapsed: Vec::new(),
        }
    }
}

/// Data returned from note operations.
#[derive(Debug, Clone)]
pub struct NoteData {
    pub id: String,
    pub title: String,
    pub content: String,
    pub color: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub fn notes_dir(base_dir: &PathBuf, book_id: &str) -> PathBuf {
    book::book_dir(base_dir, book_id).join("notes")
}

fn notes_tree_path(base_dir: &PathBuf, book_id: &str) -> PathBuf {
    book::book_dir(base_dir, book_id).join("notes.json")
}

fn note_path(base_dir: &PathBuf, book_id: &str, note_id: &str) -> PathBuf {
    notes_dir(base_dir, book_id).join(format!("{}.json", note_id))
}

fn read_tree(base_dir: &PathBuf, book_id: &str) -> NotesTreeJson {
    let path = notes_tree_path(base_dir, book_id);
    if path.exists() {
        repo::read_json::<NotesTreeJson>(&path).unwrap_or_default()
    } else {
        NotesTreeJson::default()
    }
}

fn ensure_notes_dir(base_dir: &PathBuf, book_id: &str) {
    let dir = notes_dir(base_dir, book_id);
    std::fs::create_dir_all(&dir).ok();
}

fn file_mtime_str(path: &std::path::Path) -> String {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| {
            let dt: chrono::DateTime<chrono::Utc> = t.into();
            Some(dt.format("%Y-%m-%d %H:%M:%S").to_string())
        })
        .unwrap_or_default()
}

/// Collect all descendant IDs of a note (for recursive delete or circular ref check).
fn collect_descendants(tree: &NotesTreeJson, note_id: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut stack = vec![note_id.to_string()];
    while let Some(id) = stack.pop() {
        if let Some(children) = tree.children.get(&id) {
            for child_id in children {
                result.push(child_id.clone());
                stack.push(child_id.clone());
            }
        }
    }
    result
}

/// Remove a note ID from wherever it appears in the tree (root_order or a parent's children).
fn remove_from_tree(tree: &mut NotesTreeJson, note_id: &str) {
    tree.root_order.retain(|id| id != note_id);
    for children in tree.children.values_mut() {
        children.retain(|id| id != note_id);
    }
    // Clean up empty children entries
    tree.children.retain(|_, v| !v.is_empty());
}

pub fn list_notes(base_dir: &PathBuf, book_id: &str) -> Result<(Vec<NoteData>, NotesTreeJson)> {
    let book_path = book::book_json_path(base_dir, book_id);
    if !book_path.exists() {
        return Err(GitStoreError::BookNotFound(book_id.to_string()));
    }

    let tree = read_tree(base_dir, book_id);

    // Collect all note IDs from tree
    let mut all_ids: Vec<String> = tree.root_order.clone();
    for children in tree.children.values() {
        all_ids.extend(children.iter().cloned());
    }

    let mut notes = Vec::new();
    for note_id in &all_ids {
        let path = note_path(base_dir, book_id, note_id);
        if let Ok(n) = repo::read_json::<NoteJson>(&path) {
            let updated_at = file_mtime_str(&path);
            notes.push(NoteData {
                id: note_id.clone(),
                title: n.title,
                content: n.content,
                color: n.color,
                created_at: n.created_at,
                updated_at,
            });
        }
    }

    Ok((notes, tree))
}

pub fn get_note(base_dir: &PathBuf, book_id: &str, note_id: &str) -> Result<NoteData> {
    let path = note_path(base_dir, book_id, note_id);
    if !path.exists() {
        return Err(GitStoreError::NoteNotFound(note_id.to_string()));
    }

    let n: NoteJson = repo::read_json(&path)?;
    let updated_at = file_mtime_str(&path);

    Ok(NoteData {
        id: note_id.to_string(),
        title: n.title,
        content: n.content,
        color: n.color,
        created_at: n.created_at,
        updated_at,
    })
}

pub fn create_note(
    base_dir: &PathBuf,
    book_id: &str,
    note_id: &str,
    title: &str,
    parent_id: Option<&str>,
    color: Option<&str>,
    created_at: &str,
) -> Result<NoteData> {
    let book_path = book::book_json_path(base_dir, book_id);
    if !book_path.exists() {
        return Err(GitStoreError::BookNotFound(book_id.to_string()));
    }

    ensure_notes_dir(base_dir, book_id);

    // Write note file
    let n = NoteJson {
        title: title.to_string(),
        content: String::new(),
        color: color.map(|s| s.to_string()),
        created_at: created_at.to_string(),
    };
    let path = note_path(base_dir, book_id, note_id);
    repo::write_json(&path, &n)?;

    // Update tree
    let mut tree = read_tree(base_dir, book_id);
    match parent_id {
        Some(pid) => {
            tree.children
                .entry(pid.to_string())
                .or_default()
                .push(note_id.to_string());
        }
        None => {
            tree.root_order.push(note_id.to_string());
        }
    }
    repo::write_json(&notes_tree_path(base_dir, book_id), &tree)?;

    // Commit
    let dir = book::book_dir(base_dir, book_id);
    let git_repo = git2::Repository::open(&dir)?;
    repo::commit_all(&git_repo, &format!("Add note: {}", title))?;

    let updated_at = file_mtime_str(&path);

    Ok(NoteData {
        id: note_id.to_string(),
        title: title.to_string(),
        content: String::new(),
        color: color.map(|s| s.to_string()),
        created_at: created_at.to_string(),
        updated_at,
    })
}

pub fn update_note(
    base_dir: &PathBuf,
    book_id: &str,
    note_id: &str,
    title: Option<&str>,
    content: Option<&str>,
    color: Option<Option<&str>>,
) -> Result<()> {
    let path = note_path(base_dir, book_id, note_id);
    if !path.exists() {
        return Err(GitStoreError::NoteNotFound(note_id.to_string()));
    }

    let mut n: NoteJson = repo::read_json(&path)?;

    if let Some(t) = title {
        n.title = t.to_string();
    }
    if let Some(c) = content {
        n.content = c.to_string();
    }
    if let Some(c) = color {
        n.color = c.map(|s| s.to_string());
    }

    repo::write_json(&path, &n)?;

    let dir = book::book_dir(base_dir, book_id);
    let git_repo = git2::Repository::open(&dir)?;
    repo::commit_all(&git_repo, &format!("Update note: {}", n.title))?;

    Ok(())
}

pub fn delete_note(base_dir: &PathBuf, book_id: &str, note_id: &str) -> Result<()> {
    let mut tree = read_tree(base_dir, book_id);

    // Collect all descendants to delete
    let descendants = collect_descendants(&tree, note_id);

    // Delete note files (note + all descendants)
    let path = note_path(base_dir, book_id, note_id);
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    for desc_id in &descendants {
        let desc_path = note_path(base_dir, book_id, desc_id);
        if desc_path.exists() {
            std::fs::remove_file(&desc_path)?;
        }
    }

    // Remove from tree
    remove_from_tree(&mut tree, note_id);
    for desc_id in &descendants {
        remove_from_tree(&mut tree, desc_id);
    }
    // Also remove this note's children entry
    tree.children.remove(note_id);
    for desc_id in &descendants {
        tree.children.remove(desc_id.as_str());
    }
    // Remove from collapsed
    tree.collapsed.retain(|id| id != note_id && !descendants.contains(id));

    repo::write_json(&notes_tree_path(base_dir, book_id), &tree)?;

    let dir = book::book_dir(base_dir, book_id);
    let git_repo = git2::Repository::open(&dir)?;
    repo::commit_all(&git_repo, &format!("Delete note {}", note_id))?;

    Ok(())
}

pub fn move_note(
    base_dir: &PathBuf,
    book_id: &str,
    note_id: &str,
    new_parent_id: Option<&str>,
    index: usize,
) -> Result<()> {
    let mut tree = read_tree(base_dir, book_id);

    // Validate: cannot move into own subtree
    if let Some(pid) = new_parent_id {
        if pid == note_id {
            return Err(GitStoreError::CircularReference);
        }
        let descendants = collect_descendants(&tree, note_id);
        if descendants.contains(&pid.to_string()) {
            return Err(GitStoreError::CircularReference);
        }
    }

    // Remove from old position
    remove_from_tree(&mut tree, note_id);

    // Insert at new position
    match new_parent_id {
        Some(pid) => {
            let children = tree.children.entry(pid.to_string()).or_default();
            let idx = index.min(children.len());
            children.insert(idx, note_id.to_string());
        }
        None => {
            let idx = index.min(tree.root_order.len());
            tree.root_order.insert(idx, note_id.to_string());
        }
    }

    repo::write_json(&notes_tree_path(base_dir, book_id), &tree)?;

    let dir = book::book_dir(base_dir, book_id);
    let git_repo = git2::Repository::open(&dir)?;
    repo::commit_all(&git_repo, "Move note")?;

    Ok(())
}

pub fn update_note_tree(base_dir: &PathBuf, book_id: &str, tree: &NotesTreeJson) -> Result<()> {
    let tree_path = notes_tree_path(base_dir, book_id);

    ensure_notes_dir(base_dir, book_id);
    repo::write_json(&tree_path, tree)?;

    let dir = book::book_dir(base_dir, book_id);
    let git_repo = git2::Repository::open(&dir)?;
    repo::commit_all(&git_repo, "Update note tree")?;

    Ok(())
}
