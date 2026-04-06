use saps::sqlx::{Pool, Postgres, Executor};
use saps::auth::dal::model::AuthSession;

/// Runs all database migrations: the app-specific embedded migrations
/// and the saps auth schema (auth_sessions table, indexes, ping function).
pub async fn run_migrations(pool: &Pool<Postgres>) {
    // Run app-specific migrations (e.g. users table)
    let mut migrations = sqlx::migrate!("./migrations");
    migrations.ignore_missing = true;
    migrations.run(pool).await.expect("failed to run app migrations");

    // Run saps auth schema migration
    let auth_sql = AuthSession::<crate::roles::Role>::generate_migration_sql();
    pool.execute(auth_sql).await.expect("failed to run saps auth migrations");
}
