-- Slim down books table: content now lives in git repos.
-- Keep id, user_id, title, created_at as an ownership index.

CREATE TABLE IF NOT EXISTS books_v2 (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    title TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

INSERT OR IGNORE INTO books_v2 (id, user_id, title, created_at)
    SELECT id, user_id, title, created_at FROM books;

DROP TABLE IF EXISTS chapters;
DROP TABLE IF EXISTS books;
ALTER TABLE books_v2 RENAME TO books;
