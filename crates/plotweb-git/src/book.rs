use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::error::{GitStoreError, Result};
use crate::repo;
use plotweb_common::FontSettings;

/// On-disk representation of book.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookJson {
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub font_settings: Option<FontSettings>,
    #[serde(default)]
    pub chapter_order: Vec<String>,
    pub created_at: String,
}

/// Data returned from BookStore book operations
#[derive(Debug, Clone)]
pub struct BookData {
    pub title: String,
    pub description: String,
    pub font_settings: Option<FontSettings>,
    pub chapter_order: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub fn book_dir(base_dir: &PathBuf, book_id: &str) -> PathBuf {
    base_dir.join(book_id)
}

pub fn book_json_path(base_dir: &PathBuf, book_id: &str) -> PathBuf {
    book_dir(base_dir, book_id).join("book.json")
}

pub fn chapters_dir(base_dir: &PathBuf, book_id: &str) -> PathBuf {
    book_dir(base_dir, book_id).join("chapters")
}

pub fn create_book(
    base_dir: &PathBuf,
    book_id: &str,
    title: &str,
    description: &str,
    created_at: &str,
) -> Result<()> {
    let dir = book_dir(base_dir, book_id);
    let chapters = chapters_dir(base_dir, book_id);
    std::fs::create_dir_all(&chapters)?;

    let book = BookJson {
        title: title.to_string(),
        description: description.to_string(),
        font_settings: None,
        chapter_order: Vec::new(),
        created_at: created_at.to_string(),
    };

    repo::write_json(&dir.join("book.json"), &book)?;
    let git_repo = repo::init_repo(&dir)?;
    repo::commit_all(&git_repo, "Create book")?;

    Ok(())
}

pub fn get_book(base_dir: &PathBuf, book_id: &str) -> Result<BookData> {
    let path = book_json_path(base_dir, book_id);
    if !path.exists() {
        return Err(GitStoreError::BookNotFound(book_id.to_string()));
    }

    let book: BookJson = repo::read_json(&path)?;
    let updated_at = file_mtime_str(&path);

    Ok(BookData {
        title: book.title,
        description: book.description,
        font_settings: book.font_settings,
        chapter_order: book.chapter_order,
        created_at: book.created_at,
        updated_at,
    })
}

pub fn update_book(
    base_dir: &PathBuf,
    book_id: &str,
    title: Option<&str>,
    description: Option<&str>,
    font_settings: Option<&FontSettings>,
) -> Result<()> {
    let path = book_json_path(base_dir, book_id);
    if !path.exists() {
        return Err(GitStoreError::BookNotFound(book_id.to_string()));
    }

    let mut book: BookJson = repo::read_json(&path)?;

    if let Some(t) = title {
        book.title = t.to_string();
    }
    if let Some(d) = description {
        book.description = d.to_string();
    }
    if let Some(fs) = font_settings {
        book.font_settings = Some(fs.clone());
    }

    repo::write_json(&path, &book)?;

    let dir = book_dir(base_dir, book_id);
    let git_repo = git2::Repository::open(&dir)?;
    repo::commit_all(&git_repo, "Update book")?;

    Ok(())
}

pub fn delete_book(base_dir: &PathBuf, book_id: &str) -> Result<()> {
    let dir = book_dir(base_dir, book_id);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    Ok(())
}

pub fn get_book_at_commit(base_dir: &PathBuf, book_id: &str, commit_hex: &str) -> Result<BookData> {
    let dir = book_dir(base_dir, book_id);
    let git_repo = git2::Repository::open(&dir)?;
    let oid = git2::Oid::from_str(commit_hex)?;
    let book: BookJson = repo::read_json_at_commit(&git_repo, oid, "book.json")?;

    Ok(BookData {
        title: book.title,
        description: book.description,
        font_settings: book.font_settings,
        chapter_order: book.chapter_order,
        created_at: book.created_at,
        updated_at: String::new(),
    })
}

pub fn get_head_oid(base_dir: &PathBuf, book_id: &str) -> Result<String> {
    let dir = book_dir(base_dir, book_id);
    let git_repo = git2::Repository::open(&dir)?;
    let oid = repo::head_oid(&git_repo)?;
    Ok(oid.to_string())
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
