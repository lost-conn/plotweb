use plotweb_common::{Book, Chapter, Note, NoteTree, SharedBook, User};
use rinch_core::Signal;

#[derive(Clone, Copy)]
pub struct AppStore {
    pub current_user: Signal<Option<User>>,
    pub current_route: Signal<Route>,
    pub books: Signal<Vec<Book>>,
    pub shared_books: Signal<Vec<SharedBook>>,
    pub current_book: Signal<Option<Book>>,
    pub chapters: Signal<Vec<Chapter>>,
    pub notes: Signal<Vec<Note>>,
    pub note_tree: Signal<Option<NoteTree>>,
    pub loading: Signal<bool>,
    pub error: Signal<Option<String>>,
    pub dark_mode: Signal<bool>,
    pub sidebar_open: Signal<bool>,
}

impl AppStore {
    pub fn new() -> Self {
        Self {
            current_user: Signal::new(None),
            current_route: Signal::new(Route::Login),
            books: Signal::new(Vec::new()),
            shared_books: Signal::new(Vec::new()),
            current_book: Signal::new(None),
            chapters: Signal::new(Vec::new()),
            notes: Signal::new(Vec::new()),
            note_tree: Signal::new(None),
            loading: Signal::new(true),
            error: Signal::new(None),
            dark_mode: Signal::new(true),
            sidebar_open: Signal::new(true),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Route {
    Login,
    Register,
    /// Request a password-reset link (enter email).
    ForgotPassword,
    /// Redeem a reset token from an emailed link (`/reset-password/{token}`).
    ResetPassword(String),
    Dashboard,
    Book(String),
    Reader(String),
    /// Author "preview as reader" for a book (by book_id), using the improved
    /// paginated reader without feedback/progress persistence.
    ReaderPreview(String),
    ThemePreview,
    /// Phase-0 dev spike: the rinch-editor-view rich-text editor (`/editor-spike`).
    EditorSpike,
}

impl Route {
    pub fn to_path(&self) -> String {
        match self {
            Route::Dashboard => "/".into(),
            Route::Login => "/login".into(),
            Route::Register => "/register".into(),
            Route::ForgotPassword => "/forgot-password".into(),
            Route::ResetPassword(token) => format!("/reset-password/{}", token),
            Route::Book(id) => format!("/book/{}", id),
            Route::Reader(token) => format!("/read/{}", token),
            Route::ReaderPreview(book_id) => format!("/preview/{}", book_id),
            Route::ThemePreview => "/theme".into(),
            Route::EditorSpike => "/editor-spike".into(),
        }
    }

    pub fn from_path(path: &str) -> Self {
        let path = path.trim_end_matches('/');
        match path {
            "" | "/" => Route::Dashboard,
            "/login" => Route::Login,
            "/register" => Route::Register,
            "/forgot-password" => Route::ForgotPassword,
            "/theme" => Route::ThemePreview,
            "/editor-spike" => Route::EditorSpike,
            _ if path.starts_with("/reset-password/") => {
                let token = &path[16..];
                if token.is_empty() {
                    Route::Login
                } else {
                    Route::ResetPassword(token.to_string())
                }
            }
            _ if path.starts_with("/book/") => {
                let id = &path[6..];
                if id.is_empty() {
                    Route::Dashboard
                } else {
                    Route::Book(id.to_string())
                }
            }
            _ if path.starts_with("/read/") => {
                let token = &path[6..];
                if token.is_empty() {
                    Route::Dashboard
                } else {
                    Route::Reader(token.to_string())
                }
            }
            _ if path.starts_with("/preview/") => {
                let id = &path[9..];
                if id.is_empty() {
                    Route::Dashboard
                } else {
                    Route::ReaderPreview(id.to_string())
                }
            }
            _ => Route::Dashboard,
        }
    }
}
