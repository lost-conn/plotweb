use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::book::{self, BookJson};
use crate::error::{GitStoreError, Result};
use crate::repo;

/// On-disk representation of a chapter JSON file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChapterJson {
    pub title: String,
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

fn chapter_path(base_dir: &PathBuf, book_id: &str, chapter_id: &str) -> PathBuf {
    book::chapters_dir(base_dir, book_id).join(format!("{}.json", chapter_id))
}

pub fn list_chapters(base_dir: &PathBuf, book_id: &str) -> Result<Vec<ChapterData>> {
    let book_path = book::book_json_path(base_dir, book_id);
    if !book_path.exists() {
        return Err(GitStoreError::BookNotFound(book_id.to_string()));
    }

    let book_json: BookJson = repo::read_json(&book_path)?;
    let mut chapters = Vec::new();

    for (i, chapter_id) in book_json.chapter_order.iter().enumerate() {
        let path = chapter_path(base_dir, book_id, chapter_id);
        if let Ok(ch) = repo::read_json::<ChapterJson>(&path) {
            let updated_at = file_mtime_str(&path);
            chapters.push(ChapterData {
                id: chapter_id.clone(),
                title: ch.title,
                content: ch.content,
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

    let path = chapter_path(base_dir, book_id, chapter_id);
    if !path.exists() {
        return Err(GitStoreError::ChapterNotFound(chapter_id.to_string()));
    }

    let book_json: BookJson = repo::read_json(&book_path)?;
    let sort_order = book_json
        .chapter_order
        .iter()
        .position(|id| id == chapter_id)
        .unwrap_or(0) as i64;

    let ch: ChapterJson = repo::read_json(&path)?;
    let updated_at = file_mtime_str(&path);

    Ok(ChapterData {
        id: chapter_id.to_string(),
        title: ch.title,
        content: ch.content,
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

    // Write chapter file
    let ch = ChapterJson {
        title: title.to_string(),
        content: String::new(),
        created_at: created_at.to_string(),
    };
    let path = chapter_path(base_dir, book_id, chapter_id);
    repo::write_json(&path, &ch)?;

    // Update book.json chapter_order
    let mut book_json: BookJson = repo::read_json(&book_path)?;
    let sort_order = book_json.chapter_order.len() as i64;
    book_json.chapter_order.push(chapter_id.to_string());
    repo::write_json(&book_path, &book_json)?;

    // Commit
    let dir = book::book_dir(base_dir, book_id);
    let git_repo = git2::Repository::open(&dir)?;
    repo::commit_all(&git_repo, &format!("Add chapter: {}", title))?;

    let updated_at = file_mtime_str(&path);

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
    let path = chapter_path(base_dir, book_id, chapter_id);
    if !path.exists() {
        return Err(GitStoreError::ChapterNotFound(chapter_id.to_string()));
    }

    let mut ch: ChapterJson = repo::read_json(&path)?;

    if let Some(t) = title {
        ch.title = t.to_string();
    }
    if let Some(c) = content {
        ch.content = c.to_string();
    }

    repo::write_json(&path, &ch)?;

    let dir = book::book_dir(base_dir, book_id);
    let git_repo = git2::Repository::open(&dir)?;
    repo::commit_all(&git_repo, &format!("Update chapter: {}", ch.title))?;

    Ok(())
}

pub fn delete_chapter(base_dir: &PathBuf, book_id: &str, chapter_id: &str) -> Result<()> {
    let path = chapter_path(base_dir, book_id, chapter_id);
    if path.exists() {
        std::fs::remove_file(&path)?;
    }

    // Remove from chapter_order
    let book_path = book::book_json_path(base_dir, book_id);
    if book_path.exists() {
        let mut book_json: BookJson = repo::read_json(&book_path)?;
        book_json.chapter_order.retain(|id| id != chapter_id);
        repo::write_json(&book_path, &book_json)?;
    }

    let dir = book::book_dir(base_dir, book_id);
    let git_repo = git2::Repository::open(&dir)?;
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
            content: ch.content.clone(),
            created_at: now.clone(),
        };
        let path = chapter_path(base_dir, book_id, &id);
        repo::write_json(&path, &chapter_json)?;

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

    let dir = book::book_dir(base_dir, book_id);
    let git_repo = git2::Repository::open(&dir)?;
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

    let dir = book::book_dir(base_dir, book_id);
    let git_repo = git2::Repository::open(&dir)?;
    repo::commit_all(&git_repo, "Reorder chapters")?;

    Ok(())
}

pub fn get_chapter_at_commit(
    base_dir: &PathBuf,
    book_id: &str,
    chapter_id: &str,
    commit_hex: &str,
) -> Result<ChapterData> {
    let dir = book::book_dir(base_dir, book_id);
    let git_repo = git2::Repository::open(&dir)?;
    let oid = git2::Oid::from_str(commit_hex)?;

    let book_json: BookJson = crate::repo::read_json_at_commit(&git_repo, oid, "book.json")?;
    let sort_order = book_json
        .chapter_order
        .iter()
        .position(|id| id == chapter_id)
        .unwrap_or(0) as i64;

    let ch_path = format!("chapters/{}.json", chapter_id);
    let ch: ChapterJson = crate::repo::read_json_at_commit(&git_repo, oid, &ch_path)?;

    Ok(ChapterData {
        id: chapter_id.to_string(),
        title: ch.title,
        content: ch.content,
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
    let dir = book::book_dir(base_dir, book_id);
    let git_repo = git2::Repository::open(&dir)?;
    let oid = git2::Oid::from_str(commit_hex)?;

    let book_json: BookJson = crate::repo::read_json_at_commit(&git_repo, oid, "book.json")?;
    let mut chapters = Vec::new();

    for (i, chapter_id) in book_json.chapter_order.iter().enumerate() {
        let ch_path = format!("chapters/{}.json", chapter_id);
        if let Ok(ch) = crate::repo::read_json_at_commit::<ChapterJson>(&git_repo, oid, &ch_path) {
            chapters.push(ChapterData {
                id: chapter_id.clone(),
                title: ch.title,
                content: ch.content,
                sort_order: i as i64,
                created_at: ch.created_at,
                updated_at: String::new(),
            });
        }
    }

    Ok(chapters)
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
