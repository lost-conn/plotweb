CREATE TABLE IF NOT EXISTS beta_reader_links (
    id TEXT PRIMARY KEY,
    book_id TEXT NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    token TEXT UNIQUE NOT NULL,
    reader_name TEXT NOT NULL,
    max_chapter_index INTEGER,
    active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS beta_reader_feedback (
    id TEXT PRIMARY KEY,
    link_id TEXT NOT NULL REFERENCES beta_reader_links(id) ON DELETE CASCADE,
    chapter_id TEXT NOT NULL,
    selected_text TEXT NOT NULL DEFAULT '',
    context_block TEXT NOT NULL DEFAULT '',
    comment TEXT NOT NULL,
    resolved INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS beta_reader_replies (
    id TEXT PRIMARY KEY,
    feedback_id TEXT NOT NULL REFERENCES beta_reader_feedback(id) ON DELETE CASCADE,
    author_type TEXT NOT NULL,
    author_name TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
