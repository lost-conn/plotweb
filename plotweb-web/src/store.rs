use plotweb_common::{Book, Chapter, User};
use rinch_core::Signal;

#[derive(Clone, Copy)]
pub struct AppStore {
    pub current_user: Signal<Option<User>>,
    pub current_route: Signal<Route>,
    pub books: Signal<Vec<Book>>,
    pub current_book: Signal<Option<Book>>,
    pub chapters: Signal<Vec<Chapter>>,
    pub loading: Signal<bool>,
    pub error: Signal<Option<String>>,
    pub dark_mode: Signal<bool>,
}

impl AppStore {
    pub fn new() -> Self {
        Self {
            current_user: Signal::new(None),
            current_route: Signal::new(Route::Login),
            books: Signal::new(Vec::new()),
            current_book: Signal::new(None),
            chapters: Signal::new(Vec::new()),
            loading: Signal::new(true),
            error: Signal::new(None),
            dark_mode: Signal::new(true),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Route {
    Login,
    Register,
    Dashboard,
    Book(String),
    Editor(String, String),
}
