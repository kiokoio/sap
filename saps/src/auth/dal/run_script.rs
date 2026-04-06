//! Defines the functionality around running an SQL script against a database.
use sqlx::{Executor, Pool, Postgres};
use crate::errors::saps::SapsError;

/// Loads a SQL script from a path relative to the workspace root and executes it
/// against the provided PostgreSQL connection pool.
///
/// # Arguments
/// * `pool` - The PostgreSQL connection pool to execute the script against
/// * `relative_path` - Path to the SQL file relative to the workspace `Cargo.toml`
///
/// # Example
/// ```ignore
/// run_sql_script(pool, "scripts/seed.sql").await?;
/// ```
pub async fn run_sql_script(pool: &Pool<Postgres>, relative_path: &str) -> Result<(), SapsError> {
    let workspace_root = std::env::var("CARGO_MANIFEST_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().expect("failed to get current directory"));

    let full_path = workspace_root.join(relative_path);
    let sql = std::fs::read_to_string(&full_path)
        .map_err(|e| SapsError::unknown(format!("failed to read SQL file {}: {}", full_path.display(), e)))?;

    pool.execute(sql.as_str())
        .await
        .map_err(|e| SapsError::unknown(format!("failed to execute SQL script {}: {}", full_path.display(), e)))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::Row;

    #[saps::db_test]
    async fn test_run_sql_script_creates_table_and_inserts() {
        run_sql_script(pool, "tests/fixtures/test_create_table.sql")
            .await
            .expect("run script");

        let rows = sqlx::query("SELECT name FROM test_run_script ORDER BY id")
            .fetch_all(pool)
            .await
            .expect("query test table");

        assert_eq!(rows.len(), 2);
        let first: String = rows[0].get("name");
        let second: String = rows[1].get("name");
        assert_eq!(first, "hello");
        assert_eq!(second, "world");
    }

    #[saps::db_test]
    async fn test_run_sql_script_file_not_found() {
        let result = run_sql_script(pool, "tests/fixtures/nonexistent.sql").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("failed to read SQL file"));
    }

    #[saps::db_test]
    async fn test_run_sql_script_invalid_sql() {
        // Write a temp file with invalid SQL
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let path = std::path::Path::new(&dir).join("tests/fixtures/bad.sql");
        std::fs::write(&path, "NOT VALID SQL AT ALL;").expect("write bad sql");

        let result = run_sql_script(pool, "tests/fixtures/bad.sql").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("failed to execute SQL script"));

        // Clean up
        let _ = std::fs::remove_file(&path);
    }
}
