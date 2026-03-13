use sqlx::SqlitePool;
use std::path::PathBuf;

use crate::book::{self, BookJson};
use crate::chapter::ChapterJson;
use crate::repo;

/// Migrate existing books and chapters from SQLite to git repos.
/// Idempotent — skips books that already have a git repo.
pub async fn migrate_sqlite_to_git(pool: &SqlitePool, base_dir: &PathBuf) -> anyhow::Result<()> {
    std::fs::create_dir_all(base_dir)?;

    // Read all books from old schema (which still has description, font_settings, etc.)
    let books = sqlx::query_as::<_, (String, String, String, String, String, Option<String>)>(
        "SELECT id, title, description, created_at, updated_at, font_settings FROM books",
    )
    .fetch_all(pool)
    .await?;

    for (book_id, title, description, created_at, _updated_at, font_settings_str) in &books {
        let dir = book::book_dir(base_dir, book_id);

        // Skip if already migrated
        if dir.join(".git").exists() {
            continue;
        }

        println!("Migrating book to git: {} ({})", title, book_id);

        let chapters_dir = book::chapters_dir(base_dir, book_id);
        std::fs::create_dir_all(&chapters_dir)?;

        // Read chapters for this book
        let chapters = sqlx::query_as::<_, (String, String, String, i64, String, String)>(
            "SELECT id, title, content, sort_order, created_at, updated_at \
             FROM chapters WHERE book_id = ? ORDER BY sort_order ASC",
        )
        .bind(book_id)
        .fetch_all(pool)
        .await?;

        // Build chapter_order and write chapter files
        let mut chapter_order: Vec<String> = Vec::new();
        for (ch_id, ch_title, content, _sort_order, ch_created_at, _ch_updated_at) in &chapters {
            chapter_order.push(ch_id.clone());

            let ch = ChapterJson {
                title: ch_title.clone(),
                content: content.clone(),
                created_at: ch_created_at.clone(),
            };
            let path = chapters_dir.join(format!("{}.json", ch_id));
            repo::write_json(&path, &ch)?;
        }

        // Parse font_settings
        let font_settings = font_settings_str
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok());

        // Write book.json
        let book_json = BookJson {
            title: title.clone(),
            description: description.clone(),
            font_settings,
            chapter_order,
            created_at: created_at.clone(),
        };
        repo::write_json(&dir.join("book.json"), &book_json)?;

        // Init git repo and commit
        let git_repo = repo::init_repo(&dir)?;
        repo::commit_all(&git_repo, "Migrate from SQLite")?;
    }

    Ok(())
}
