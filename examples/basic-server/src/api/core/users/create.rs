use argon2::{Argon2, PasswordHasher, password_hash::{SaltString, rand_core::OsRng}};
use saps::errors::saps::SapsError;
use crate::dal::models::users::tx_definitions::CreateUser;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct NewUser {
    pub username: String,
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserResponse {
    pub id: uuid::Uuid,
    pub username: String,
    pub email: String,
}

/// Creates a new user by hashing the password and inserting via the CreateUser transaction.
pub async fn create_user<X: CreateUser>(new_user: NewUser) -> Result<UserResponse, SapsError> {
    let salt = SaltString::generate(&mut OsRng);
    let password_hash = Argon2::default()
        .hash_password(new_user.password.as_bytes(), &salt)
        .map_err(|e| SapsError::unknown(e.to_string()))?
        .to_string();

    let user = X::create_user(new_user.username, new_user.email, password_hash).await?;
    Ok(UserResponse {
        id: user.id,
        username: user.username,
        email: user.email,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use saps::dal::connections::SqlxPostGresDescriptor;

    #[saps::db_test]
    async fn test_create_user() {
        crate::migrations::run_migrations(pool).await;
        let new_user = NewUser {
            username: "testuser".to_string(),
            email: "test@example.com".to_string(),
            password: "securepassword123".to_string(),
        };
        let user = create_user::<SqlxPostGresDescriptor<TestDbHandle>>(new_user)
            .await.expect("create user");
        assert_eq!(user.username, "testuser");
        assert_eq!(user.email, "test@example.com");
    }

    #[saps::db_test]
    async fn test_create_duplicate_username_fails() {
        crate::migrations::run_migrations(pool).await;
        let new_user = NewUser {
            username: "duplicate".to_string(),
            email: "first@example.com".to_string(),
            password: "password".to_string(),
        };
        create_user::<SqlxPostGresDescriptor<TestDbHandle>>(new_user)
            .await.expect("first create");

        let dup = NewUser {
            username: "duplicate".to_string(),
            email: "second@example.com".to_string(),
            password: "password".to_string(),
        };
        let result = create_user::<SqlxPostGresDescriptor<TestDbHandle>>(dup).await;
        assert!(result.is_err());
    }
}
