use saps::dal::connections::SqlxPostGresDescriptor;
use saps::db_transaction;
use super::tx_definitions::{CreateUser, GetUserByEmail, GetUserById, DeleteUser, User};

#[db_transaction(SqlxPostGresDescriptor, CreateUser)]
async fn create_user(username: String, email: String, password_hash: String) -> User {
    let pool = T::yield_pool();
    let row = saps::sqlx::query_as::<_, (uuid::Uuid, String, String, String)>(
        r#"
        INSERT INTO users (username, email, password_hash)
        VALUES ($1, $2, $3)
        RETURNING id, username, email, password_hash
        "#,
    )
        .bind(&username)
        .bind(&email)
        .bind(&password_hash)
        .fetch_one(pool)
        .await?;
    Ok(User {
        id: row.0,
        username: row.1,
        email: row.2,
        password_hash: row.3,
    })
}

#[db_transaction(SqlxPostGresDescriptor, GetUserByEmail)]
async fn get_user_by_email(email: String) -> Option<User> {
    let pool = T::yield_pool();
    let row = saps::sqlx::query_as::<_, (uuid::Uuid, String, String, String)>(
        "SELECT id, username, email, password_hash FROM users WHERE email = $1",
    )
        .bind(&email)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| User {
        id: r.0,
        username: r.1,
        email: r.2,
        password_hash: r.3,
    }))
}

#[db_transaction(SqlxPostGresDescriptor, GetUserById)]
async fn get_user_by_id(user_id: uuid::Uuid) -> Option<User> {
    let pool = T::yield_pool();
    let row = saps::sqlx::query_as::<_, (uuid::Uuid, String, String, String)>(
        "SELECT id, username, email, password_hash FROM users WHERE id = $1",
    )
        .bind(user_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| User {
        id: r.0,
        username: r.1,
        email: r.2,
        password_hash: r.3,
    }))
}

#[db_transaction(SqlxPostGresDescriptor, DeleteUser)]
async fn delete_user(user_id: uuid::Uuid) -> bool {
    let pool = T::yield_pool();
    let result = saps::sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}
