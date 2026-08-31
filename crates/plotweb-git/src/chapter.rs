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
    pub word_count: u64,
    pub created_at: String,
    pub updated_at: String,
}

/// Count words in chapter/note content.
///
/// Content is an opaque string the frontend owns. New content is `DocNode` JSON
/// (the editor's durable save shape); legacy content is chapter Markdown / note
/// HTML. If the content parses as a JSON object, walk it and count words in every
/// `"text"` string field (the DocNode text-node shape); otherwise fall back to the
/// legacy tag-strip logic.
fn count_words(content: &str) -> u64 {
    let trimmed = content.trim_start();
    if trimmed.starts_with('{') {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
            return count_json_text_words(&value);
        }
    }
    count_stripped_words(content)
}

/// Recursively sum whitespace-separated words across every `"text"` string field
/// in a DocNode JSON tree.
fn count_json_text_words(value: &serde_json::Value) -> u64 {
    match value {
        serde_json::Value::Object(map) => {
            let mut count = 0;
            for (key, val) in map {
                if key == "text" {
                    if let Some(s) = val.as_str() {
                        count += s.split_whitespace().count() as u64;
                    }
                } else {
                    count += count_json_text_words(val);
                }
            }
            count
        }
        serde_json::Value::Array(items) => items.iter().map(count_json_text_words).sum(),
        _ => 0,
    }
}

/// Legacy word count: strip HTML/Markdown tags, then count whitespace-separated words.
fn count_stripped_words(content: &str) -> u64 {
    let mut in_tag = false;
    let text: String = content
        .chars()
        .filter(|&c| {
            if c == '<' {
                in_tag = true;
                false
            } else if c == '>' {
                in_tag = false;
                false
            } else {
                !in_tag
            }
        })
        .collect();
    text.split_whitespace().count() as u64
}

/// Validate that an id is a well-formed UUID. All ids in this app are
/// `Uuid::new_v4().to_string()`, so anything else (notably path-traversal
/// sequences like `../../etc/passwd`) must be rejected at the storage boundary
/// before it reaches a filesystem path.
fn valid_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok()
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

/// Sum word counts for all chapters in a book without loading full chapter data.
pub fn book_word_count(base_dir: &PathBuf, book_id: &str) -> u64 {
    let book_path = book::book_json_path(base_dir, book_id);
    let book_json: BookJson = match repo::read_json(&book_path) {
        Ok(b) => b,
        Err(_) => return 0,
    };
    book_json
        .chapter_order
        .iter()
        .map(|cid| count_words(&read_content(base_dir, book_id, cid)))
        .sum()
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
            let word_count = count_words(&content);
            let updated_at = book::file_mtime_str(&chapter_md_path(base_dir, book_id, chapter_id));
            chapters.push(ChapterData {
                id: chapter_id.clone(),
                title: ch.title,
                content,
                sort_order: i as i64,
                word_count,
                created_at: ch.created_at,
                updated_at,
            });
        }
    }

    Ok(chapters)
}

pub fn get_chapter(base_dir: &PathBuf, book_id: &str, chapter_id: &str) -> Result<ChapterData> {
    if !valid_id(chapter_id) {
        return Err(GitStoreError::ChapterNotFound(chapter_id.to_string()));
    }
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
    let word_count = count_words(&content);
    let updated_at = book::file_mtime_str(&chapter_md_path(base_dir, book_id, chapter_id));

    Ok(ChapterData {
        id: chapter_id.to_string(),
        title: ch.title,
        content,
        sort_order,
        word_count,
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
    repo::write_text(&md_path, "")?;

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
        word_count: 0,
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
    if !valid_id(chapter_id) {
        return Err(GitStoreError::ChapterNotFound(chapter_id.to_string()));
    }
    let json_path = chapter_json_path(base_dir, book_id, chapter_id);
    if !json_path.exists() {
        return Err(GitStoreError::ChapterNotFound(chapter_id.to_string()));
    }

    let mut ch: ChapterJson = repo::read_json(&json_path)?;

    // Track exactly which files change so we can stage only those (and so
    // consecutive autosaves of the same chapter coalesce into one commit).
    let mut changed_paths: Vec<String> = Vec::new();
    if let Some(t) = title {
        ch.title = t.to_string();
        repo::write_json(&json_path, &ch)?;
        changed_paths.push(format!("chapters/{}.json", chapter_id));
    }
    if let Some(c) = content {
        let md_path = chapter_md_path(base_dir, book_id, chapter_id);
        repo::write_text(&md_path, c)?;
        changed_paths.push(format!("chapters/{}.md", chapter_id));
    }

    if changed_paths.is_empty() {
        return Ok(());
    }

    let ms_dir = book::manuscript_dir(base_dir, book_id);
    let git_repo = git2::Repository::open(&ms_dir)?;
    repo::commit_paths(&git_repo, &changed_paths, &format!("Update chapter: {}", ch.title))?;

    Ok(())
}

pub fn delete_chapter(base_dir: &PathBuf, book_id: &str, chapter_id: &str) -> Result<()> {
    if !valid_id(chapter_id) {
        return Err(GitStoreError::ChapterNotFound(chapter_id.to_string()));
    }

    // Update book.json FIRST so it never references files that are already
    // deleted (if a crash happens mid-delete).
    let book_path = book::book_json_path(base_dir, book_id);
    if book_path.exists() {
        let mut book_json: BookJson = repo::read_json(&book_path)?;
        book_json.chapter_order.retain(|id| id != chapter_id);
        repo::write_json(&book_path, &book_json)?;
    }

    let json_path = chapter_json_path(base_dir, book_id, chapter_id);
    if json_path.exists() {
        std::fs::remove_file(&json_path)?;
    }
    let md_path = chapter_md_path(base_dir, book_id, chapter_id);
    if md_path.exists() {
        std::fs::remove_file(&md_path)?;
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
        repo::write_text(&md_path, &ch.content)?;

        book_json.chapter_order.push(id.clone());

        result.push(ChapterData {
            id,
            title: ch.title.clone(),
            content: ch.content.clone(),
            sort_order,
            word_count: count_words(&ch.content),
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
    if !valid_id(chapter_id) {
        return Err(GitStoreError::ChapterNotFound(chapter_id.to_string()));
    }
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
    let word_count = count_words(&content);

    Ok(ChapterData {
        id: chapter_id.to_string(),
        title: ch.title,
        content,
        sort_order,
        word_count,
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
            let word_count = count_words(&content);
            chapters.push(ChapterData {
                id: chapter_id.clone(),
                title: ch.title,
                content,
                sort_order: i as i64,
                word_count,
                created_at: ch.created_at,
                updated_at: String::new(),
            });
        }
    }

    Ok(chapters)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book;
    use crate::repo;
    use crate::repo::AMEND_WINDOW_SECS;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Unique scratch dir under the system temp dir (no tempfile dep available).
    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "plotweb-git-test-{}-{}",
                std::process::id(),
                n
            ));
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
        fn path(&self) -> &PathBuf {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Count commits reachable from HEAD of a book's manuscript repo.
    fn commit_count(base_dir: &PathBuf, book_id: &str) -> usize {
        let ms_dir = book::manuscript_dir(base_dir, book_id);
        let git_repo = git2::Repository::open(&ms_dir).unwrap();
        // Large limit — these test repos have only a handful of commits.
        repo::list_commits(&git_repo, 10_000, 0).unwrap().len()
    }

    fn head_tree_id(base_dir: &PathBuf, book_id: &str) -> git2::Oid {
        let ms_dir = book::manuscript_dir(base_dir, book_id);
        let git_repo = git2::Repository::open(&ms_dir).unwrap();
        let oid = repo::head_oid(&git_repo).unwrap();
        git_repo.find_commit(oid).unwrap().tree_id()
    }

    #[test]
    fn count_words_handles_docnode_json_and_legacy() {
        // DocNode JSON: only `"text"` fields count; type/attr/mark keys are ignored.
        let docnode = r#"{
            "type": "doc",
            "content": [
                {"type": "heading", "attrs": {"level": 1}, "content": [
                    {"type": "text", "text": "Chapter One"}
                ]},
                {"type": "paragraph", "content": [
                    {"type": "text", "text": "The lantern ", "marks": [{"type": "bold"}]},
                    {"type": "text", "text": "guttered against the fog."}
                ]}
            ]
        }"#;
        // "Chapter One" (2) + "The lantern " (2) + "guttered against the fog." (4) = 8
        assert_eq!(count_words(docnode), 8);

        // Legacy Markdown (chapters): tag-strip path.
        assert_eq!(count_words("# Title\n\nOne two three four."), 6);

        // Legacy HTML (notes): tags stripped, text words counted.
        assert_eq!(count_words("<p>hello <strong>brave</strong> world</p>"), 3);

        // Not JSON (leading brace matters): falls back to the legacy path.
        assert_eq!(count_words("plain prose here"), 3);

        // A string that starts with '{' but isn't valid JSON also falls back.
        assert_eq!(count_words("{not json} but four words"), 5);

        assert_eq!(count_words(""), 0);
    }

    #[test]
    fn update_chapter_skips_noop_commit_but_records_real_change() {
        let tmp = TempDir::new();
        let base = tmp.path().clone();

        let book_id = uuid::Uuid::new_v4().to_string();
        book::create_book(&base, &book_id, "Test Book", "desc", "2026-01-01 00:00:00").unwrap();

        let chapter_id = uuid::Uuid::new_v4().to_string();
        create_chapter(&base, &book_id, &chapter_id, "Chapter One", "2026-01-01 00:00:00").unwrap();

        // Baseline: the "Add chapter" commit wrote an empty .md.
        let count_before = commit_count(&base, &book_id);
        let tree_before = head_tree_id(&base, &book_id);

        // 1) Re-save with IDENTICAL content ("" matches what create_chapter wrote)
        //    → staged tree equals HEAD tree → no commit should be recorded.
        update_chapter(&base, &book_id, &chapter_id, None, Some("")).unwrap();
        assert_eq!(
            commit_count(&base, &book_id),
            count_before,
            "no-op re-save must not create a ghost commit"
        );
        assert_eq!(
            head_tree_id(&base, &book_id),
            tree_before,
            "HEAD must be unchanged after a no-op save"
        );

        // 2) Save with DIFFERENT content → a real commit must be recorded.
        //    The prior commit ("Add chapter") is a multi-file change, so
        //    commit_paths appends rather than amends → exactly +1 commit.
        update_chapter(&base, &book_id, &chapter_id, None, Some("Some new prose.")).unwrap();
        assert_eq!(
            commit_count(&base, &book_id),
            count_before + 1,
            "a genuine content change must add exactly one commit"
        );
        assert_ne!(
            head_tree_id(&base, &book_id),
            tree_before,
            "HEAD tree must differ once the new content is committed"
        );
    }

    /// Backdate HEAD so the next save sees it as an older commit, without waiting.
    fn backdate_head(base_dir: &PathBuf, book_id: &str, seconds: i64) {
        let ms_dir = book::manuscript_dir(base_dir, book_id);
        let git_repo = git2::Repository::open(&ms_dir).unwrap();
        let head = git_repo.head().unwrap().peel_to_commit().unwrap();
        let when = git2::Time::new(head.time().seconds() - seconds, 0);
        let sig = git2::Signature::new("PlotWeb", "plotweb@localhost", &when).unwrap();
        head.amend(Some("HEAD"), Some(&sig), Some(&sig), None, None, None)
            .unwrap();
    }

    /// Autosaves seconds apart coalesce — that is what amending is for — but the
    /// coalescing must be bounded, or a long session on one chapter leaves a single
    /// commit whose content is overwritten each time. Every intermediate version would
    /// be destroyed, and a bad final write (a stale device replacing newer text) would
    /// take the good text with it. History is the safety net cutover depends on.
    #[test]
    fn amending_is_bounded_so_a_session_leaves_recoverable_versions() {
        let dir = TempDir::new();
        let base = dir.path().clone();
        let book_id = uuid::Uuid::new_v4().to_string();
        let book_id = book_id.as_str();
        book::create_book(&base, book_id, "Amend Window", "desc", "2026-01-01 00:00:00").unwrap();
        let chapter_id = uuid::Uuid::new_v4().to_string();
        let chapter_id = chapter_id.as_str();
        create_chapter(&base, book_id, chapter_id, "One", "2026-01-01 00:00:00").unwrap();

        // First content save: the previous commit ("Add chapter") is a multi-file
        // change, so this appends.
        update_chapter(&base, book_id, chapter_id, None, Some("first pass")).unwrap();
        let after_first = commit_count(&base, book_id);

        // A save moments later continues the same commit.
        update_chapter(&base, book_id, chapter_id, None, Some("first pass, tweaked")).unwrap();
        assert_eq!(
            commit_count(&base, book_id),
            after_first,
            "consecutive autosaves of one chapter still coalesce"
        );

        // The same save, once the window has passed, is a new version instead.
        backdate_head(&base, book_id, AMEND_WINDOW_SECS + 60);
        update_chapter(&base, book_id, chapter_id, None, Some("second pass")).unwrap();
        assert_eq!(
            commit_count(&base, book_id),
            after_first + 1,
            "past the window a save must start a new commit, so the earlier text stays \
             recoverable"
        );

        // And the earlier text really is still there, at the commit before HEAD.
        let ms_dir = book::manuscript_dir(&base, book_id);
        let git_repo = git2::Repository::open(&ms_dir).unwrap();
        let head = git_repo.head().unwrap().peel_to_commit().unwrap();
        let previous = head.parent(0).unwrap();
        let earlier =
            get_chapter_at_commit(&base, book_id, chapter_id, &previous.id().to_string()).unwrap();
        assert!(
            earlier.content.contains("first pass, tweaked"),
            "the superseded version must be readable at its own commit: {}",
            earlier.content
        );
    }
}
