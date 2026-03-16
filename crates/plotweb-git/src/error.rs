use thiserror::Error;

#[derive(Debug, Error)]
pub enum GitStoreError {
    #[error("book not found: {0}")]
    BookNotFound(String),

    #[error("chapter not found: {0}")]
    ChapterNotFound(String),

    #[error("note not found: {0}")]
    NoteNotFound(String),

    #[error("circular reference: cannot move note into its own subtree")]
    CircularReference,

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("git error: {0}")]
    Git(#[from] git2::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, GitStoreError>;
