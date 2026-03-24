use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::book::{self, BookJson};
use crate::error::{GitStoreError, Result};
use crate::repo;

/// On-disk representation of a chapter JSON file (metadata only — content is in .md).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChapterJson {
    pub title: String,
    pub created_at: String,
}

/// Legacy format that included content in JSON (for backward-compat reads).
#[derive(Debug, Clone, Deserialize)]
struct ChapterJsonCompat {
    pub title: String,
    #[serde(default)]
    pub content: String,
    pub created_at: String,
}

/// Data returned from chapter operations.
#[derive(Debug, Clone)]
pub struct ChapterData {
    pub id: String,
    pub title: String,
    pub content: String,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
}

fn chapter_json_path(base_dir: &PathBuf, book_id: &str, chapter_id: &str) -> PathBuf {
    book::chapters_dir(base_dir, book_id).join(format!("{}.json", chapter_id))
}

fn chapter_md_path(base_dir: &PathBuf, book_id: &str, chapter_id: &str) -> PathBuf {
    book::chapters_dir(base_dir, book_id).join(format!("{}.md", chapter_id))
}

/// Read chapter content from the .md file, falling back to empty string.
fn read_content(base_dir: &PathBuf, book_id: &str, chapter_id: &str) -> String {
    let md_path = chapter_md_path(base_dir, book_id, chapter_id);
    std::fs::read_to_string(&md_path).unwrap_or_default()
}

/// Read chapter content at a specific commit. Tries .md first, falls back to
/// legacy JSON content field for commits before the storage migration.
fn read_content_at_commit(repo: &git2::Repository, oid: git2::Oid, chapter_id: &str) -> String {
    let md_path = format!("chapters/{}.md", chapter_id);
    if let Ok(content) = crate::repo::read_text_at_commit(repo, oid, &md_path) {
        return content;
    }
    // Fall back to legacy format (content embedded in JSON)
    let json_path = format!("chapters/{}.json", chapter_id);
    if let Ok(ch) = crate::repo::read_json_at_commit::<ChapterJsonCompat>(repo, oid, &json_path) {
        return ch.content;
    }
    String::new()
}

pub fn list_chapters(base_dir: &PathBuf, book_id: &str) -> Result<Vec<ChapterData>> {
    let book_path = book::book_json_path(base_dir, book_id);
    if !book_path.exists() {
        return Err(GitStoreError::BookNotFound(book_id.to_string()));
    }

    let book_json: BookJson = repo::read_json(&book_path)?;
    let mut chapters = Vec::new();

    for (i, chapter_id) in book_json.chapter_order.iter().enumerate() {
        let path = chapter_json_path(base_dir, book_id, chapter_id);
        if let Ok(ch) = repo::read_json::<ChapterJson>(&path) {
            let content = read_content(base_dir, book_id, chapter_id);
            let updated_at = book::file_mtime_str(&chapter_md_path(base_dir, book_id, chapter_id));
            chapters.push(ChapterData {
                id: chapter_id.clone(),
                title: ch.title,
                content,
                sort_order: i as i64,
                created_at: ch.created_at,
                updated_at,
            });
        }
    }

    Ok(chapters)
}

pub fn get_chapter(base_dir: &PathBuf, book_id: &str, chapter_id: &str) -> Result<ChapterData> {
    let book_path = book::book_json_path(base_dir, book_id);
    if !book_path.exists() {
        return Err(GitStoreError::BookNotFound(book_id.to_string()));
    }

    let json_path = chapter_json_path(base_dir, book_id, chapter_id);
    if !json_path.exists() {
        return Err(GitStoreError::ChapterNotFound(chapter_id.to_string()));
    }

    let book_json: BookJson = repo::read_json(&book_path)?;
    let sort_order = book_json
        .chapter_order
        .iter()
        .position(|id| id == chapter_id)
        .unwrap_or(0) as i64;

    let ch: ChapterJson = repo::read_json(&json_path)?;
    let content = read_content(base_dir, book_id, chapter_id);
    let updated_at = book::file_mtime_str(&chapter_md_path(base_dir, book_id, chapter_id));

    Ok(ChapterData {
        id: chapter_id.to_string(),
        title: ch.title,
        content,
        sort_order,
        created_at: ch.created_at,
        updated_at,
    })
}

pub fn create_chapter(
    base_dir: &PathBuf,
    book_id: &str,
    chapter_id: &str,
    title: &str,
    created_at: &str,
) -> Result<ChapterData> {
    let book_path = book::book_json_path(base_dir, book_id);
    if !book_path.exists() {
        return Err(GitStoreError::BookNotFound(book_id.to_string()));
    }

    // Write metadata JSON
    let ch = ChapterJson {
        title: title.to_string(),
        created_at: created_at.to_string(),
    };
    let json_path = chapter_json_path(base_dir, book_id, chapter_id);
    repo::write_json(&json_path, &ch)?;

    // Write empty .md content file
    let md_path = chapter_md_path(base_dir, book_id, chapter_id);
    std::fs::write(&md_path, "")?;

    // Update book.json chapter_order
    let mut book_json: BookJson = repo::read_json(&book_path)?;
    let sort_order = book_json.chapter_order.len() as i64;
    book_json.chapter_order.push(chapter_id.to_string());
    repo::write_json(&book_path, &book_json)?;

    // Commit
    let ms_dir = book::manuscript_dir(base_dir, book_id);
    let git_repo = git2::Repository::open(&ms_dir)?;
    repo::commit_all(&git_repo, &format!("Add chapter: {}", title))?;

    let updated_at = book::file_mtime_str(&md_path);

    Ok(ChapterData {
        id: chapter_id.to_string(),
        title: title.to_string(),
        content: String::new(),
        sort_order,
        created_at: created_at.to_string(),
        updated_at,
    })
}

pub fn update_chapter(
    base_dir: &PathBuf,
    book_id: &str,
    chapter_id: &str,
    title: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let json_path = chapter_json_path(base_dir, book_id, chapter_id);
    if !json_path.exists() {
        return Err(GitStoreError::ChapterNotFound(chapter_id.to_string()));
    }

    let mut ch: ChapterJson = repo::read_json(&json_path)?;

    if let Some(t) = title {
        ch.title = t.to_string();
        repo::write_json(&json_path, &ch)?;
    }
    if let Some(c) = content {
        let md_path = chapter_md_path(base_dir, book_id, chapter_id);
        std::fs::write(&md_path, c)?;
    }

    let ms_dir = book::manuscript_dir(base_dir, book_id);
    let git_repo = git2::Repository::open(&ms_dir)?;
    repo::commit_all(&git_repo, &format!("Update chapter: {}", ch.title))?;

    Ok(())
}

pub fn delete_chapter(base_dir: &PathBuf, book_id: &str, chapter_id: &str) -> Result<()> {
    let json_path = chapter_json_path(base_dir, book_id, chapter_id);
    if json_path.exists() {
        std::fs::remove_file(&json_path)?;
    }
    let md_path = chapter_md_path(base_dir, book_id, chapter_id);
    if md_path.exists() {
        std::fs::remove_file(&md_path)?;
    }

    // Remove from chapter_order
    let book_path = book::book_json_path(base_dir, book_id);
    if book_path.exists() {
        let mut book_json: BookJson = repo::read_json(&book_path)?;
        book_json.chapter_order.retain(|id| id != chapter_id);
        repo::write_json(&book_path, &book_json)?;
    }

    let ms_dir = book::manuscript_dir(base_dir, book_id);
    let git_repo = git2::Repository::open(&ms_dir)?;
    repo::commit_all(&git_repo, &format!("Delete chapter {}", chapter_id))?;

    Ok(())
}

/// Bulk-import chapters with content in a single commit.
pub fn import_chapters(
    base_dir: &PathBuf,
    book_id: &str,
    chapters: &[plotweb_common::ImportChapter],
) -> Result<Vec<ChapterData>> {
    let book_path = book::book_json_path(base_dir, book_id);
    if !book_path.exists() {
        return Err(GitStoreError::BookNotFound(book_id.to_string()));
    }

    let mut book_json: BookJson = repo::read_json(&book_path)?;
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let mut result = Vec::new();

    for ch in chapters {
        let id = uuid::Uuid::new_v4().to_string();
        let sort_order = book_json.chapter_order.len() as i64;

        let chapter_json = ChapterJson {
            title: ch.title.clone(),
            created_at: now.clone(),
        };
        let json_path = chapter_json_path(base_dir, book_id, &id);
        repo::write_json(&json_path, &chapter_json)?;

        let md_path = chapter_md_path(base_dir, book_id, &id);
        std::fs::write(&md_path, &ch.content)?;

        book_json.chapter_order.push(id.clone());

        result.push(ChapterData {
            id,
            title: ch.title.clone(),
            content: ch.content.clone(),
            sort_order,
            created_at: now.clone(),
            updated_at: now.clone(),
        });
    }

    repo::write_json(&book_path, &book_json)?;

    let ms_dir = book::manuscript_dir(base_dir, book_id);
    let git_repo = git2::Repository::open(&ms_dir)?;
    repo::commit_all(
        &git_repo,
        &format!("Import {} chapters", chapters.len()),
    )?;

    Ok(result)
}

pub fn reorder_chapters(base_dir: &PathBuf, book_id: &str, chapter_ids: &[String]) -> Result<()> {
    let book_path = book::book_json_path(base_dir, book_id);
    if !book_path.exists() {
        return Err(GitStoreError::BookNotFound(book_id.to_string()));
    }

    let mut book_json: BookJson = repo::read_json(&book_path)?;
    book_json.chapter_order = chapter_ids.to_vec();
    repo::write_json(&book_path, &book_json)?;

    let ms_dir = book::manuscript_dir(base_dir, book_id);
    let git_repo = git2::Repository::open(&ms_dir)?;
    repo::commit_all(&git_repo, "Reorder chapters")?;

    Ok(())
}

pub fn get_chapter_at_commit(
    base_dir: &PathBuf,
    book_id: &str,
    chapter_id: &str,
    commit_hex: &str,
) -> Result<ChapterData> {
    let ms_dir = book::manuscript_dir(base_dir, book_id);
    let git_repo = git2::Repository::open(&ms_dir)?;
    let oid = git2::Oid::from_str(commit_hex)?;

    let book_json: BookJson = crate::repo::read_json_at_commit(&git_repo, oid, "book.json")?;
    let sort_order = book_json
        .chapter_order
        .iter()
        .position(|id| id == chapter_id)
        .unwrap_or(0) as i64;

    // Read metadata from JSON (compat struct handles both old and new format)
    let ch_path = format!("chapters/{}.json", chapter_id);
    let ch: ChapterJsonCompat = crate::repo::read_json_at_commit(&git_repo, oid, &ch_path)?;

    // Read content: try .md first, fall back to legacy JSON content
    let content = read_content_at_commit(&git_repo, oid, chapter_id);

    Ok(ChapterData {
        id: chapter_id.to_string(),
        title: ch.title,
        content,
        sort_order,
        created_at: ch.created_at,
        updated_at: String::new(),
    })
}

pub fn list_chapters_at_commit(
    base_dir: &PathBuf,
    book_id: &str,
    commit_hex: &str,
) -> Result<Vec<ChapterData>> {
    let ms_dir = book::manuscript_dir(base_dir, book_id);
    let git_repo = git2::Repository::open(&ms_dir)?;
    let oid = git2::Oid::from_str(commit_hex)?;

    let book_json: BookJson = crate::repo::read_json_at_commit(&git_repo, oid, "book.json")?;
    let mut chapters = Vec::new();

    for (i, chapter_id) in book_json.chapter_order.iter().enumerate() {
        let ch_path = format!("chapters/{}.json", chapter_id);
        if let Ok(ch) = crate::repo::read_json_at_commit::<ChapterJsonCompat>(&git_repo, oid, &ch_path) {
            let content = read_content_at_commit(&git_repo, oid, chapter_id);
            chapters.push(ChapterData {
                id: chapter_id.clone(),
                title: ch.title,
                content,
                sort_order: i as i64,
                created_at: ch.created_at,
                updated_at: String::new(),
            });
        }
    }

    Ok(chapters)
}
