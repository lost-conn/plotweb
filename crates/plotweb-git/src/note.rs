use plotweb_common::{RelativeTime, TimeSpan};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::book;
use crate::error::{GitStoreError, Result};
use crate::repo;

/// On-disk representation of a note JSON file.
///
/// Every facet field defaults, so a note written before the notes revamp — which is
/// every note that exists — loads as lore: no span, no entity mark, no event parent,
/// and its tree position untouched.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteJson {
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub color: Option<String>,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<TimeSpan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relative: Option<RelativeTime>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_entity: bool,
    /// Containment in time — see `plotweb_common::Note::event_parent`. Emphatically
    /// not the tree parent, which lives in `notes.json` and is untouched by this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_parent: Option<String>,
}

/// A patch over a note's facet fields: `None` leaves the stored value alone,
/// `Some(None)` clears it, `Some(Some(v))` sets it.
///
/// A flat `Option` would be enough to *set* a span and not enough to clear one, and
/// "undate this event" is an ordinary gesture.
#[derive(Debug, Clone, Default)]
pub struct NoteFacetPatch {
    pub span: Option<Option<TimeSpan>>,
    pub relative: Option<Option<RelativeTime>>,
    pub is_entity: Option<bool>,
    pub event_parent: Option<Option<String>>,
}

impl NoteFacetPatch {
    /// Whether this patch would change anything at all — the cheap check the write
    /// paths use before deciding a note file needs rewriting.
    pub fn is_empty(&self) -> bool {
        self.span.is_none()
            && self.relative.is_none()
            && self.is_entity.is_none()
            && self.event_parent.is_none()
    }

    /// The patch that sets every facet to what `note` carries — what the mirror needs
    /// when it is writing the canonical document's view of a note back into git.
    pub fn setting_all(
        span: Option<TimeSpan>,
        relative: Option<RelativeTime>,
        is_entity: bool,
        event_parent: Option<String>,
    ) -> Self {
        Self {
            span: Some(span),
            relative: Some(relative),
            is_entity: Some(is_entity),
            event_parent: Some(event_parent),
        }
    }
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
    pub span: Option<TimeSpan>,
    pub relative: Option<RelativeTime>,
    pub is_entity: bool,
    pub event_parent: Option<String>,
}

/// Git repo directory for notes (separate from manuscript).
pub fn notes_repo_dir(base_dir: &PathBuf, book_id: &str) -> PathBuf {
    book::book_dir(base_dir, book_id).join("notes")
}

fn notes_tree_path(base_dir: &PathBuf, book_id: &str) -> PathBuf {
    notes_repo_dir(base_dir, book_id).join("notes.json")
}

/// Validate that an id is a well-formed UUID. All note ids in this app are
/// `Uuid::new_v4().to_string()`, so anything else (notably path-traversal
/// sequences) must be rejected before it reaches a filesystem path.
fn valid_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok()
}

fn note_path(base_dir: &PathBuf, book_id: &str, note_id: &str) -> PathBuf {
    notes_repo_dir(base_dir, book_id).join(format!("{}.json", note_id))
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
    let dir = notes_repo_dir(base_dir, book_id);
    std::fs::create_dir_all(&dir).ok();
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
    // Check book exists via manuscript
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
            let updated_at = book::file_mtime_str(&path);
            notes.push(note_data(note_id.clone(), n, updated_at));
        }
    }

    Ok((notes, tree))
}

pub fn get_note(base_dir: &PathBuf, book_id: &str, note_id: &str) -> Result<NoteData> {
    if !valid_id(note_id) {
        return Err(GitStoreError::NoteNotFound(note_id.to_string()));
    }
    let path = note_path(base_dir, book_id, note_id);
    if !path.exists() {
        return Err(GitStoreError::NoteNotFound(note_id.to_string()));
    }

    let n: NoteJson = repo::read_json(&path)?;
    let updated_at = book::file_mtime_str(&path);

    Ok(note_data(note_id.to_string(), n, updated_at))
}

/// Adapt a stored note file into the shape the stores hand out. One place, so a facet
/// added later cannot reach `list_notes` and miss `get_note`.
fn note_data(id: String, n: NoteJson, updated_at: String) -> NoteData {
    NoteData {
        id,
        title: n.title,
        content: n.content,
        color: n.color,
        created_at: n.created_at,
        updated_at,
        span: n.span,
        relative: n.relative,
        is_entity: n.is_entity,
        event_parent: n.event_parent,
    }
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

    // Write note file. A new note is lore: no span, no entity mark, no event parent.
    // Facets arrive by a later edit, never at creation.
    let n = NoteJson {
        title: title.to_string(),
        content: String::new(),
        color: color.map(|s| s.to_string()),
        created_at: created_at.to_string(),
        span: None,
        relative: None,
        is_entity: false,
        event_parent: None,
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

    // Commit to notes repo
    let nr_dir = notes_repo_dir(base_dir, book_id);
    let git_repo = git2::Repository::open(&nr_dir)?;
    repo::commit_all(&git_repo, &format!("Add note: {}", title))?;

    let updated_at = book::file_mtime_str(&path);

    Ok(NoteData {
        id: note_id.to_string(),
        title: title.to_string(),
        content: String::new(),
        color: color.map(|s| s.to_string()),
        created_at: created_at.to_string(),
        updated_at,
        span: None,
        relative: None,
        is_entity: false,
        event_parent: None,
    })
}

pub fn update_note(
    base_dir: &PathBuf,
    book_id: &str,
    note_id: &str,
    title: Option<&str>,
    content: Option<&str>,
    color: Option<Option<&str>>,
    facets: &NoteFacetPatch,
) -> Result<()> {
    if !valid_id(note_id) {
        return Err(GitStoreError::NoteNotFound(note_id.to_string()));
    }
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
    if let Some(span) = facets.span.clone() {
        n.span = span;
    }
    if let Some(relative) = facets.relative.clone() {
        n.relative = relative;
    }
    if let Some(is_entity) = facets.is_entity {
        n.is_entity = is_entity;
    }
    if let Some(event_parent) = facets.event_parent.clone() {
        n.event_parent = event_parent;
    }

    repo::write_json(&path, &n)?;

    // Stage only this note's file so consecutive autosaves of the same note
    // coalesce into one commit (see commit_paths) instead of one per keystroke-
    // batch.
    let nr_dir = notes_repo_dir(base_dir, book_id);
    let git_repo = git2::Repository::open(&nr_dir)?;
    repo::commit_paths(&git_repo, &[format!("{}.json", note_id)], &format!("Update note: {}", n.title))?;

    Ok(())
}

pub fn delete_note(base_dir: &PathBuf, book_id: &str, note_id: &str) -> Result<()> {
    if !valid_id(note_id) {
        return Err(GitStoreError::NoteNotFound(note_id.to_string()));
    }
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

    let nr_dir = notes_repo_dir(base_dir, book_id);
    let git_repo = git2::Repository::open(&nr_dir)?;
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
    if !valid_id(note_id) {
        return Err(GitStoreError::NoteNotFound(note_id.to_string()));
    }
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

    // Detect whether this is a reorder within the SAME list the note already
    // lives in, and where it currently sits. remove_from_tree shifts indices,
    // so a downward move within the same list would otherwise land one slot too
    // late (off-by-one).
    let same_list = match new_parent_id {
        Some(pid) => tree
            .children
            .get(pid)
            .map_or(false, |c| c.contains(&note_id.to_string())),
        None => tree.root_order.contains(&note_id.to_string()),
    };
    let old_pos = match new_parent_id {
        Some(pid) => tree
            .children
            .get(pid)
            .and_then(|c| c.iter().position(|id| id == note_id)),
        None => tree.root_order.iter().position(|id| id == note_id),
    };

    // Remove from old position
    remove_from_tree(&mut tree, note_id);

    let mut index = index;
    if same_list {
        if let Some(op) = old_pos {
            if op < index {
                index -= 1;
            }
        }
    }

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

    let nr_dir = notes_repo_dir(base_dir, book_id);
    let git_repo = git2::Repository::open(&nr_dir)?;
    repo::commit_all(&git_repo, "Move note")?;

    Ok(())
}

pub fn update_note_tree(base_dir: &PathBuf, book_id: &str, tree: &NotesTreeJson) -> Result<()> {
    let tree_path = notes_tree_path(base_dir, book_id);

    ensure_notes_dir(base_dir, book_id);
    repo::write_json(&tree_path, tree)?;

    let nr_dir = notes_repo_dir(base_dir, book_id);
    let git_repo = git2::Repository::open(&nr_dir)?;
    repo::commit_all(&git_repo, "Update note tree")?;

    Ok(())
}
