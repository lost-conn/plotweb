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

    pool
}

/// Run migration 003 — must be called AFTER data migration to git.
pub async fn run_migration_003(pool: &SqlitePool) {
    let migration_003 = include_str!("../../../migrations/003_git_migration.sql");
    sqlx::raw_sql(migration_003)
        .execute(pool)
        .await
        .expect("failed to run migration 003");
}
