use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

pub async fn init_db() -> SqlitePool {
    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:plotweb.db".into());

    let options = SqliteConnectOptions::from_str(&db_url)
        .expect("invalid DATABASE_URL")
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .expect("failed to connect to database");

    // Run migrations
    let migration_sql = include_str!("../../../migrations/001_initial.sql");
    sqlx::raw_sql(migration_sql)
        .execute(&pool)
        .await
        .expect("failed to run migrations");

    let migration_002 = include_str!("../../../migrations/002_book_font_settings.sql");
    sqlx::raw_sql(migration_002).execute(&pool).await.ok(); // .ok() = idempotent

    let migration_004 = include_str!("../../../migrations/004_beta_readers.sql");
    sqlx::raw_sql(migration_004).execute(&pool).await.ok();

    let migration_005 = include_str!("../../../migrations/005_beta_pinned_commit.sql");
    sqlx::raw_sql(migration_005).execute(&pool).await.ok();

    pool
}

/// Run migration 003 — must be called AFTER data migration to git.
/// Only runs if the old `books` schema still has columns that were removed
/// (e.g. `description`). Skips if already migrated to avoid DROP CASCADE
/// destroying beta_reader_links and related tables.
pub async fn run_migration_003(pool: &SqlitePool) {
    // Check if the old schema still has the `description` column
    let has_description = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM pragma_table_info('books') WHERE name = 'description'"
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    if has_description > 0 {
        let migration_003 = include_str!("../../../migrations/003_git_migration.sql");
        sqlx::raw_sql(migration_003)
            .execute(pool)
            .await
            .expect("failed to run migration 003");
    }
}
