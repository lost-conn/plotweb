//! One-time idempotent import of metadata from SQLite into rhypedb.
//!
//! Runs at startup. For each of the five metadata tables it copies any row whose
//! UUID isn't already present in rhypedb, so it's safe to run repeatedly and a
//! no-op once everything is migrated (and on fresh installs, where the SQLite
//! tables exist but are empty). SQLite is kept as the import source; no route
//! reads it anymore.

use sqlx::SqlitePool;

use crate::rhype::{quote, Fields, RhypeStore};

/// True if a row of `type_name` with this uuid already exists in rhypedb.
async fn present(rhype: &RhypeStore, type_name: &str, uuid: &str) -> bool {
    rhype
        .exists(format!("{type_name}.filter(.uuid == {}).limit(1)", quote(uuid)))
        .await
        .unwrap_or(false)
}

pub async fn migrate_sqlite_to_rhype(pool: &SqlitePool, rhype: &RhypeStore) {
    let mut totals = (0u32, 0u32, 0u32, 0u32, 0u32);

    // users
    let users = sqlx::query_as::<_, (String, String, String, String, String)>(
        "SELECT id, username, email, password_hash, created_at FROM users",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    for (id, username, email, password_hash, created_at) in users {
        if present(rhype, "User", &id).await {
            continue;
        }
        let fields = Fields::new()
            .str("uuid", &id)
            .str("username", &username)
            .str("email", &email)
            .str("password_hash", &password_hash)
            .str("created_at", &created_at)
            .render();
        if let Err(e) = rhype.create(format!("User.create({fields})")).await {
            eprintln!("rhype import: user {id} failed: {e}");
        } else {
            totals.0 += 1;
        }
    }

    // books
    let books = sqlx::query_as::<_, (String, String, String, String)>(
        "SELECT id, user_id, title, created_at FROM books",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    for (id, user_id, title, created_at) in books {
        if present(rhype, "Book", &id).await {
            continue;
        }
        let fields = Fields::new()
            .str("uuid", &id)
            .str("user_id", &user_id)
            .str("title", &title)
            .str("created_at", &created_at)
            .render();
        if let Err(e) = rhype.create(format!("Book.create({fields})")).await {
            eprintln!("rhype import: book {id} failed: {e}");
        } else {
            totals.1 += 1;
        }
    }

    // beta_reader_links
    let links = sqlx::query_as::<_, (
        String,
        String,
        String,
        String,
        Option<i64>,
        i64,
        String,
        Option<String>,
        Option<String>,
    )>(
        "SELECT id, book_id, token, reader_name, max_chapter_index, active, created_at, pinned_commit, user_id FROM beta_reader_links",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    for (id, book_id, token, reader_name, max_chapter_index, active, created_at, pinned_commit, user_id) in
        links
    {
        if present(rhype, "BetaLink", &id).await {
            continue;
        }
        let fields = Fields::new()
            .str("uuid", &id)
            .str("book_id", &book_id)
            .str("token", &token)
            .str("reader_name", &reader_name)
            .opt_int("max_chapter_index", max_chapter_index)
            .bool("active", active != 0)
            .opt_str("pinned_commit", pinned_commit.as_deref())
            .opt_str("user_id", user_id.as_deref())
            .str("created_at", &created_at)
            .render();
        if let Err(e) = rhype.create(format!("BetaLink.create({fields})")).await {
            eprintln!("rhype import: beta link {id} failed: {e}");
        } else {
            totals.2 += 1;
        }
    }

    // beta_reader_feedback
    let feedback = sqlx::query_as::<_, (String, String, String, String, String, String, i64, String)>(
        "SELECT id, link_id, chapter_id, selected_text, context_block, comment, resolved, created_at FROM beta_reader_feedback",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    for (id, link_id, chapter_id, selected_text, context_block, comment, resolved, created_at) in feedback {
        if present(rhype, "BetaFeedback", &id).await {
            continue;
        }
        let fields = Fields::new()
            .str("uuid", &id)
            .str("link_id", &link_id)
            .str("chapter_id", &chapter_id)
            .str("selected_text", &selected_text)
            .str("context_block", &context_block)
            .str("comment", &comment)
            .bool("resolved", resolved != 0)
            .str("created_at", &created_at)
            .render();
        if let Err(e) = rhype.create(format!("BetaFeedback.create({fields})")).await {
            eprintln!("rhype import: feedback {id} failed: {e}");
        } else {
            totals.3 += 1;
        }
    }

    // beta_reader_replies
    let replies = sqlx::query_as::<_, (String, String, String, String, String, String)>(
        "SELECT id, feedback_id, author_type, author_name, content, created_at FROM beta_reader_replies",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    for (id, feedback_id, author_type, author_name, content, created_at) in replies {
        if present(rhype, "BetaReply", &id).await {
            continue;
        }
        let fields = Fields::new()
            .str("uuid", &id)
            .str("feedback_id", &feedback_id)
            .str("author_type", &author_type)
            .str("author_name", &author_name)
            .str("content", &content)
            .str("created_at", &created_at)
            .render();
        if let Err(e) = rhype.create(format!("BetaReply.create({fields})")).await {
            eprintln!("rhype import: reply {id} failed: {e}");
        } else {
            totals.4 += 1;
        }
    }

    let (u, b, l, f, r) = totals;
    if u + b + l + f + r > 0 {
        println!(
            "rhype import: migrated {u} users, {b} books, {l} beta links, {f} feedback, {r} replies from SQLite"
        );
    }
}
