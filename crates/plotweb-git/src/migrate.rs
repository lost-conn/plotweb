use serde::Deserialize;
use sqlx::SqlitePool;
use std::path::PathBuf;

use crate::book::{self, BookJson};
use crate::chapter::ChapterJson;
use crate::note;
use crate::repo;

/// Legacy chapter format with content embedded in JSON (pre-split).
#[derive(Debug, Deserialize)]
struct LegacyChapterJson {
    title: String,
    #[serde(default)]
    content: String,
    created_at: String,
}

/// Migrate existing books and chapters from SQLite to git repos.
/// Idempotent — skips books that already have a git repo.
/// NOTE: This writes directly to the new layout (manuscript/ subdir + .md files).
pub async fn migrate_sqlite_to_git(pool: &SqlitePool, base_dir: &PathBuf) -> anyhow::Result<()> {
    std::fs::create_dir_all(base_dir)?;

    let books = sqlx::query_as::<_, (String, String, String, String, String, Option<String>)>(
        "SELECT id, title, description, created_at, updated_at, font_settings FROM books",
    )
    .fetch_all(pool)
    .await?;

    for (book_id, title, description, created_at, _updated_at, font_settings_str) in &books {
        let ms_dir = book::manuscript_dir(base_dir, book_id);

        // Skip if already migrated (either old or new layout)
        if ms_dir.join(".git").exists() || book::book_dir(base_dir, book_id).join(".git").exists() {
            continue;
        }

        println!("Migrating book to git: {} ({})", title, book_id);

        let chapters_dir = book::chapters_dir(base_dir, book_id);
        std::fs::create_dir_all(&chapters_dir)?;

        let chapters = sqlx::query_as::<_, (String, String, String, i64, String, String)>(
            "SELECT id, title, content, sort_order, created_at, updated_at \
             FROM chapters WHERE book_id = ? ORDER BY sort_order ASC",
        )
        .bind(book_id)
        .fetch_all(pool)
        .await?;

        let mut chapter_order: Vec<String> = Vec::new();
        for (ch_id, ch_title, content, _sort_order, ch_created_at, _ch_updated_at) in &chapters {
            chapter_order.push(ch_id.clone());

            // Write metadata JSON (no content)
            let ch = ChapterJson {
                title: ch_title.clone(),
                created_at: ch_created_at.clone(),
            };
            let json_path = chapters_dir.join(format!("{}.json", ch_id));
            repo::write_json(&json_path, &ch)?;

            // Write content to .md file
            let md_path = chapters_dir.join(format!("{}.md", ch_id));
            std::fs::write(&md_path, content)?;
        }

        let font_settings = font_settings_str
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok());

        let book_json = BookJson {
            title: title.clone(),
            description: description.clone(),
            font_settings,
            chapter_order,
            created_at: created_at.clone(),
        };
        repo::write_json(&ms_dir.join("book.json"), &book_json)?;

        let git_repo = repo::init_repo(&ms_dir)?;
        repo::commit_all(&git_repo, "Migrate from SQLite")?;

        // Initialize notes repo
        let notes_dir = note::notes_repo_dir(base_dir, book_id);
        std::fs::create_dir_all(&notes_dir)?;
        let default_tree = note::NotesTreeJson::default();
        repo::write_json(&notes_dir.join("notes.json"), &default_tree)?;
        let notes_repo = repo::init_repo(&notes_dir)?;
        repo::commit_all(&notes_repo, "Initialize notes")?;
    }

    Ok(())
}

/// Migrate existing books from old layout (single repo at book root) to new
/// layout (manuscript/ + notes/ separate repos with .md content files).
/// Idempotent — skips books already in new layout.
pub fn migrate_to_split_repos(base_dir: &PathBuf) -> anyhow::Result<()> {
    let entries = match std::fs::read_dir(base_dir) {
        Ok(e) => e,
        Err(_) => return Ok(()), // data dir doesn't exist yet
    };

    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }

        let book_id = match dir.file_name().and_then(|n| n.to_str()) {
            Some(name) => name.to_string(),
            None => continue,
        };

        // Skip if already in new layout
        if dir.join("manuscript").join(".git").exists() {
            continue;
        }

        // Only migrate if old layout exists (.git at book root)
        if !dir.join(".git").exists() {
            continue;
        }

        println!("Migrating book to split repos: {}", book_id);

        let ms_dir = dir.join("manuscript");
        let ms_chapters = ms_dir.join("chapters");
        std::fs::create_dir_all(&ms_chapters)?;

        // Move book.json → manuscript/book.json
        if dir.join("book.json").exists() {
            std::fs::rename(dir.join("book.json"), ms_dir.join("book.json"))?;
        }

        // Move .git/ → manuscript/.git/
        if dir.join(".git").exists() {
            std::fs::rename(dir.join(".git"), ms_dir.join(".git"))?;
        }

        // Move chapters/*.json → manuscript/chapters/*.json + split content to .md
        let old_chapters = dir.join("chapters");
        if old_chapters.exists() {
            for ch_entry in std::fs::read_dir(&old_chapters)?.flatten() {
                let ch_path = ch_entry.path();
                if ch_path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }

                let filename = ch_path.file_name().unwrap().to_string_lossy().to_string();
                let chapter_id = filename.trim_end_matches(".json");

                // Read old format (content embedded in JSON)
                if let Ok(legacy) = repo::read_json::<LegacyChapterJson>(&ch_path) {
                    // Write new metadata JSON (no content)
                    let new_ch = ChapterJson {
                        title: legacy.title,
                        created_at: legacy.created_at,
                    };
                    repo::write_json(&ms_chapters.join(&filename), &new_ch)?;

                    // Write content to .md
                    let md_path = ms_chapters.join(format!("{}.md", chapter_id));
                    std::fs::write(&md_path, &legacy.content)?;
                }
            }

            // Remove old chapters dir
            std::fs::remove_dir_all(&old_chapters)?;
        }

        // Commit the restructuring in the manuscript repo
        let ms_repo = git2::Repository::open(&ms_dir)?;
        repo::commit_all(&ms_repo, "Split content to markdown files")?;

        // Set up notes repo
        let notes_dir = dir.join("notes");
        let notes_tmp = dir.join("notes_tmp");
        std::fs::create_dir_all(&notes_tmp)?;

        // Move notes.json to temp
        if dir.join("notes.json").exists() {
            std::fs::rename(dir.join("notes.json"), notes_tmp.join("notes.json"))?;
        } else {
            // Write default tree
            let default_tree = note::NotesTreeJson::default();
            repo::write_json(&notes_tmp.join("notes.json"), &default_tree)?;
        }

        // Move note files from old notes/ subdir to temp (flatten)
        if notes_dir.exists() {
            for note_entry in std::fs::read_dir(&notes_dir)?.flatten() {
                let note_path = note_entry.path();
                if note_path.extension().and_then(|e| e.to_str()) == Some("json") {
                    let fname = note_path.file_name().unwrap().to_string_lossy().to_string();
                    std::fs::rename(&note_path, notes_tmp.join(&fname))?;
                }
            }
            std::fs::remove_dir_all(&notes_dir)?;
        }

        // Rename temp to notes/
        std::fs::rename(&notes_tmp, &notes_dir)?;

        // Initialize notes git repo
        let notes_repo = repo::init_repo(&notes_dir)?;
        repo::commit_all(&notes_repo, "Initialize notes")?;

        println!("  Done migrating {}", book_id);
    }

    Ok(())
}
