pub mod book;
pub mod chapter;
pub mod error;
pub mod migrate;
pub mod note;
pub mod repo;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use error::Result;
use plotweb_common::{ImportChapter, UpdateBookRequest, UpdateChapterRequest};
use tokio::sync::Mutex;

pub use book::BookData;
pub use chapter::ChapterData;
pub use note::NoteData;

pub struct BookStore {
    base_dir: PathBuf,
    locks: std::sync::Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl BookStore {
    pub fn new(base_dir: PathBuf) -> Self {
        std::fs::create_dir_all(&base_dir).ok();
        Self {
            base_dir,
            locks: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Get or create a lock by key.
    fn lock_for(&self, key: &str) -> Arc<Mutex<()>> {
        let mut locks = self.locks.lock().unwrap();
        locks
            .entry(key.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// Lock for manuscript (book metadata + chapters) operations.
    fn manuscript_lock(&self, book_id: &str) -> Arc<Mutex<()>> {
        self.lock_for(&format!("m:{}", book_id))
    }

    /// Lock for notes operations.
    fn notes_lock(&self, book_id: &str) -> Arc<Mutex<()>> {
        self.lock_for(&format!("n:{}", book_id))
    }

    // ── Books ──

    pub async fn create_book(
        &self,
        book_id: &str,
        title: &str,
        description: &str,
        created_at: &str,
    ) -> Result<()> {
        let lock = self.manuscript_lock(book_id);
        let _guard = lock.lock().await;
        let base = self.base_dir.clone();
        let book_id = book_id.to_string();
        let title = title.to_string();
        let desc = description.to_string();
        let created = created_at.to_string();
        tokio::task::spawn_blocking(move || {
            book::create_book(&base, &book_id, &title, &desc, &created)
        })
        .await
        .unwrap()
    }

    pub async fn book_word_count(&self, book_id: &str) -> u64 {
        let base = self.base_dir.clone();
        let book_id = book_id.to_string();
        tokio::task::spawn_blocking(move || chapter::book_word_count(&base, &book_id))
            .await
            .unwrap_or(0)
    }

    pub async fn get_book(&self, book_id: &str) -> Result<BookData> {
        let base = self.base_dir.clone();
        let book_id = book_id.to_string();
        tokio::task::spawn_blocking(move || book::get_book(&base, &book_id))
            .await
            .unwrap()
    }

    pub async fn update_book(&self, book_id: &str, update: &UpdateBookRequest) -> Result<()> {
        let lock = self.manuscript_lock(book_id);
        let _guard = lock.lock().await;
        let base = self.base_dir.clone();
        let book_id = book_id.to_string();
        let title = update.title.clone();
        let description = update.description.clone();
        let font_settings = update.font_settings.clone();
        tokio::task::spawn_blocking(move || {
            book::update_book(
                &base,
                &book_id,
                title.as_deref(),
                description.as_deref(),
                font_settings.as_ref(),
            )
        })
        .await
        .unwrap()
    }

    pub async fn delete_book(&self, book_id: &str) -> Result<()> {
        let lock = self.manuscript_lock(book_id);
        let _guard = lock.lock().await;
        let base = self.base_dir.clone();
        let book_id = book_id.to_string();
        tokio::task::spawn_blocking(move || book::delete_book(&base, &book_id))
            .await
            .unwrap()
    }

    // ── Chapters ──

    pub async fn list_chapters(&self, book_id: &str) -> Result<Vec<ChapterData>> {
        let base = self.base_dir.clone();
        let book_id = book_id.to_string();
        tokio::task::spawn_blocking(move || chapter::list_chapters(&base, &book_id))
            .await
            .unwrap()
    }

    pub async fn get_chapter(&self, book_id: &str, chapter_id: &str) -> Result<ChapterData> {
        let base = self.base_dir.clone();
        let book_id = book_id.to_string();
        let chapter_id = chapter_id.to_string();
        tokio::task::spawn_blocking(move || chapter::get_chapter(&base, &book_id, &chapter_id))
            .await
            .unwrap()
    }

    pub async fn create_chapter(
        &self,
        book_id: &str,
        chapter_id: &str,
        title: &str,
        created_at: &str,
    ) -> Result<ChapterData> {
        let lock = self.manuscript_lock(book_id);
        let _guard = lock.lock().await;
        let base = self.base_dir.clone();
        let book_id = book_id.to_string();
        let chapter_id = chapter_id.to_string();
        let title = title.to_string();
        let created = created_at.to_string();
        tokio::task::spawn_blocking(move || {
            chapter::create_chapter(&base, &book_id, &chapter_id, &title, &created)
        })
        .await
        .unwrap()
    }

    pub async fn update_chapter(
        &self,
        book_id: &str,
        chapter_id: &str,
        update: &UpdateChapterRequest,
    ) -> Result<()> {
        let lock = self.manuscript_lock(book_id);
        let _guard = lock.lock().await;
        let base = self.base_dir.clone();
        let book_id = book_id.to_string();
        let chapter_id = chapter_id.to_string();
        let title = update.title.clone();
        let content = update.content.clone();
        tokio::task::spawn_blocking(move || {
            chapter::update_chapter(&base, &book_id, &chapter_id, title.as_deref(), content.as_deref())
        })
        .await
        .unwrap()
    }

    pub async fn delete_chapter(&self, book_id: &str, chapter_id: &str) -> Result<()> {
        let lock = self.manuscript_lock(book_id);
        let _guard = lock.lock().await;
        let base = self.base_dir.clone();
        let book_id = book_id.to_string();
        let chapter_id = chapter_id.to_string();
        tokio::task::spawn_blocking(move || chapter::delete_chapter(&base, &book_id, &chapter_id))
            .await
            .unwrap()
    }

    // ── Historical reads (no lock needed) ──

    pub async fn get_book_at_commit(&self, book_id: &str, commit_hex: &str) -> Result<BookData> {
        let base = self.base_dir.clone();
        let book_id = book_id.to_string();
        let commit_hex = commit_hex.to_string();
        tokio::task::spawn_blocking(move || book::get_book_at_commit(&base, &book_id, &commit_hex))
            .await
            .unwrap()
    }

    pub async fn get_head_oid(&self, book_id: &str) -> Result<String> {
        let base = self.base_dir.clone();
        let book_id = book_id.to_string();
        tokio::task::spawn_blocking(move || book::get_head_oid(&base, &book_id))
            .await
            .unwrap()
    }

    pub async fn get_chapter_at_commit(
        &self,
        book_id: &str,
        chapter_id: &str,
        commit_hex: &str,
    ) -> Result<ChapterData> {
        let base = self.base_dir.clone();
        let book_id = book_id.to_string();
        let chapter_id = chapter_id.to_string();
        let commit_hex = commit_hex.to_string();
        tokio::task::spawn_blocking(move || {
            chapter::get_chapter_at_commit(&base, &book_id, &chapter_id, &commit_hex)
        })
        .await
        .unwrap()
    }

    pub async fn list_chapters_at_commit(
        &self,
        book_id: &str,
        commit_hex: &str,
    ) -> Result<Vec<ChapterData>> {
        let base = self.base_dir.clone();
        let book_id = book_id.to_string();
        let commit_hex = commit_hex.to_string();
        tokio::task::spawn_blocking(move || {
            chapter::list_chapters_at_commit(&base, &book_id, &commit_hex)
        })
        .await
        .unwrap()
    }

    // ── History ──

    pub async fn list_commits(
        &self,
        book_id: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<plotweb_common::CommitInfo>> {
        let base = self.base_dir.clone();
        let book_id = book_id.to_string();
        tokio::task::spawn_blocking(move || {
            let ms_dir = book::manuscript_dir(&base, &book_id);
            let git_repo = git2::Repository::open(&ms_dir)?;
            let raw = repo::list_commits(&git_repo, limit, offset)?;
            Ok(raw
                .into_iter()
                .map(|(oid, message, timestamp)| {
                    let dt = chrono::DateTime::from_timestamp(timestamp, 0)
                        .unwrap_or_default();
                    plotweb_common::CommitInfo {
                        oid,
                        message,
                        created_at: dt.format("%Y-%m-%d %H:%M:%S").to_string(),
                    }
                })
                .collect())
        })
        .await
        .unwrap()
    }

    pub async fn restore_to_commit(&self, book_id: &str, commit_hex: &str) -> Result<()> {
        let lock = self.manuscript_lock(book_id);
        let _guard = lock.lock().await;
        let base = self.base_dir.clone();
        let book_id = book_id.to_string();
        let commit_hex = commit_hex.to_string();
        tokio::task::spawn_blocking(move || {
            let ms_dir = book::manuscript_dir(&base, &book_id);
            let git_repo = git2::Repository::open(&ms_dir)?;
            let oid = git2::Oid::from_str(&commit_hex)?;
            repo::restore_to_commit(&git_repo, oid)
        })
        .await
        .unwrap()
    }

    pub async fn diff_commit(
        &self,
        book_id: &str,
        commit_hex: &str,
    ) -> Result<plotweb_common::CommitDiff> {
        let base = self.base_dir.clone();
        let book_id = book_id.to_string();
        let commit_hex = commit_hex.to_string();
        tokio::task::spawn_blocking(move || {
            let ms_dir = book::manuscript_dir(&base, &book_id);
            let git_repo = git2::Repository::open(&ms_dir)?;
            let oid = git2::Oid::from_str(&commit_hex)?;

            let raw = repo::diff_commit(&git_repo, oid)?;

            // Read book.json at this commit to resolve chapter titles
            let book_json: Option<book::BookJson> =
                repo::read_json_at_commit(&git_repo, oid, "book.json").ok();

            let changed_chapters = raw
                .into_iter()
                .map(|(chapter_id, change_type, hunks)| {
                    // Try to get chapter title from the JSON at this commit
                    let chapter_title = book_json
                        .as_ref()
                        .and_then(|_| {
                            let ch_path = format!("chapters/{}.json", chapter_id);
                            repo::read_json_at_commit::<serde_json::Value>(&git_repo, oid, &ch_path)
                                .ok()
                                .and_then(|v| v.get("title")?.as_str().map(|s| s.to_string()))
                        })
                        .unwrap_or_else(|| chapter_id.clone());

                    plotweb_common::ChapterDiff {
                        chapter_id,
                        chapter_title,
                        change_type,
                        hunks: hunks
                            .into_iter()
                            .map(|(lines,)| plotweb_common::DiffHunk {
                                lines: lines
                                    .into_iter()
                                    .map(|(origin, content)| plotweb_common::DiffLine {
                                        origin: origin.to_string(),
                                        content,
                                    })
                                    .collect(),
                            })
                            .collect(),
                    }
                })
                .collect();

            Ok(plotweb_common::CommitDiff { changed_chapters })
        })
        .await
        .unwrap()
    }

    /// Import multiple chapters at once (bulk create with content).
    pub async fn import_chapters(
        &self,
        book_id: &str,
        chapters: &[ImportChapter],
    ) -> Result<Vec<ChapterData>> {
        let lock = self.manuscript_lock(book_id);
        let _guard = lock.lock().await;
        let base = self.base_dir.clone();
        let book_id = book_id.to_string();
        let chapters = chapters.to_vec();
        tokio::task::spawn_blocking(move || {
            chapter::import_chapters(&base, &book_id, &chapters)
        })
        .await
        .unwrap()
    }

    pub async fn reorder_chapters(&self, book_id: &str, chapter_ids: &[String]) -> Result<()> {
        let lock = self.manuscript_lock(book_id);
        let _guard = lock.lock().await;
        let base = self.base_dir.clone();
        let book_id = book_id.to_string();
        let ids = chapter_ids.to_vec();
        tokio::task::spawn_blocking(move || chapter::reorder_chapters(&base, &book_id, &ids))
            .await
            .unwrap()
    }

    // ── Notes ──

    pub async fn list_notes(
        &self,
        book_id: &str,
    ) -> Result<(Vec<note::NoteData>, note::NotesTreeJson)> {
        let base = self.base_dir.clone();
        let book_id = book_id.to_string();
        tokio::task::spawn_blocking(move || note::list_notes(&base, &book_id))
            .await
            .unwrap()
    }

    pub async fn get_note(&self, book_id: &str, note_id: &str) -> Result<note::NoteData> {
        let base = self.base_dir.clone();
        let book_id = book_id.to_string();
        let note_id = note_id.to_string();
        tokio::task::spawn_blocking(move || note::get_note(&base, &book_id, &note_id))
            .await
            .unwrap()
    }

    pub async fn create_note(
        &self,
        book_id: &str,
        note_id: &str,
        title: &str,
        parent_id: Option<&str>,
        color: Option<&str>,
        created_at: &str,
    ) -> Result<note::NoteData> {
        let lock = self.notes_lock(book_id);
        let _guard = lock.lock().await;
        let base = self.base_dir.clone();
        let book_id = book_id.to_string();
        let note_id = note_id.to_string();
        let title = title.to_string();
        let parent_id = parent_id.map(|s| s.to_string());
        let color = color.map(|s| s.to_string());
        let created = created_at.to_string();
        tokio::task::spawn_blocking(move || {
            note::create_note(
                &base,
                &book_id,
                &note_id,
                &title,
                parent_id.as_deref(),
                color.as_deref(),
                &created,
            )
        })
        .await
        .unwrap()
    }

    pub async fn update_note(
        &self,
        book_id: &str,
        note_id: &str,
        title: Option<&str>,
        content: Option<&str>,
        color: Option<Option<&str>>,
    ) -> Result<()> {
        let lock = self.notes_lock(book_id);
        let _guard = lock.lock().await;
        let base = self.base_dir.clone();
        let book_id = book_id.to_string();
        let note_id = note_id.to_string();
        let title = title.map(|s| s.to_string());
        let content = content.map(|s| s.to_string());
        let color = color.map(|o| o.map(|s| s.to_string()));
        tokio::task::spawn_blocking(move || {
            note::update_note(
                &base,
                &book_id,
                &note_id,
                title.as_deref(),
                content.as_deref(),
                color.as_ref().map(|o| o.as_deref()),
            )
        })
        .await
        .unwrap()
    }

    pub async fn delete_note(&self, book_id: &str, note_id: &str) -> Result<()> {
        let lock = self.notes_lock(book_id);
        let _guard = lock.lock().await;
        let base = self.base_dir.clone();
        let book_id = book_id.to_string();
        let note_id = note_id.to_string();
        tokio::task::spawn_blocking(move || note::delete_note(&base, &book_id, &note_id))
            .await
            .unwrap()
    }

    pub async fn move_note(
        &self,
        book_id: &str,
        note_id: &str,
        new_parent_id: Option<&str>,
        index: usize,
    ) -> Result<()> {
        let lock = self.notes_lock(book_id);
        let _guard = lock.lock().await;
        let base = self.base_dir.clone();
        let book_id = book_id.to_string();
        let note_id = note_id.to_string();
        let new_parent_id = new_parent_id.map(|s| s.to_string());
        let index = index;
        tokio::task::spawn_blocking(move || {
            note::move_note(&base, &book_id, &note_id, new_parent_id.as_deref(), index)
        })
        .await
        .unwrap()
    }

    pub async fn update_note_tree(
        &self,
        book_id: &str,
        tree: &note::NotesTreeJson,
    ) -> Result<()> {
        let lock = self.notes_lock(book_id);
        let _guard = lock.lock().await;
        let base = self.base_dir.clone();
        let book_id = book_id.to_string();
        let tree = tree.clone();
        tokio::task::spawn_blocking(move || note::update_note_tree(&base, &book_id, &tree))
            .await
            .unwrap()
    }
}
