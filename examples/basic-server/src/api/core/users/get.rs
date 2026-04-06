use saps::errors::saps::SapsError;
use crate::api::core::users::create::UserResponse;
use crate::dal::models::users::tx_definitions::GetUserById;

/// Gets a user profile by extracting the user_id from the session meta.
///
/// # Arguments
/// * `meta` - The session meta JSON containing a `user_id` field
pub async fn get_user<X: GetUserById>(
    meta: Option<serde_json::Value>,
) -> Result<UserResponse, SapsError> {
    // Extract user_id from session meta
    let meta = meta.ok_or_else(|| SapsError::bad_request("session has no meta"))?;
    let user_id_str = meta.get("user_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| SapsError::bad_request("meta missing user_id"))?;
    let user_id: uuid::Uuid = user_id_str.parse()
        .map_err(|_| SapsError::bad_request("invalid user_id in meta"))?;

    // Fetch user by ID
    let user = X::get_user_by_id(user_id)
        .await?
        .ok_or_else(|| SapsError::not_found("user not found"))?;

    Ok(UserResponse {
        id: user.id,
        username: user.username,
        email: user.email,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::core::users::create::{NewUser, create_user};
    use crate::roles::Role;
    use saps::auth::dal::model::AuthSession;
    use saps::auth::dal::tx_definitions::CreateAuthSession;
    use saps::dal::connections::{AuthPostGresDescriptor, SqlxPostGresDescriptor};

    #[saps::db_test]
    async fn test_get_user_from_session_meta() {
        crate::migrations::run_migrations(pool).await;

        // Create a user
        let new_user = NewUser {
            username: "getuser".to_string(),
            email: "get@example.com".to_string(),
            password: "password".to_string(),
        };
        let created = create_user::<SqlxPostGresDescriptor<TestDbHandle>>(new_user)
            .await.expect("create user");

        // Create a session with user_id in meta
        let session = AuthSession::new(Role::Admin)
            .with_meta(serde_json::json!({ "user_id": created.id.to_string() }));
        let created_session = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(session)
            .await.expect("create session");

        // Get user via session meta
        let user = get_user::<SqlxPostGresDescriptor<TestDbHandle>>(created_session.meta)
            .await.expect("get user");
        assert_eq!(user.id, created.id);
        assert_eq!(user.username, "getuser");
        assert_eq!(user.email, "get@example.com");
    }

    #[saps::db_test]
    async fn test_get_user_no_meta_returns_error() {
        crate::migrations::run_migrations(pool).await;

        let result = get_user::<SqlxPostGresDescriptor<TestDbHandle>>(None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("no meta"));
    }

    #[saps::db_test]
    async fn test_get_user_not_found_returns_error() {
        crate::migrations::run_migrations(pool).await;

        let fake_meta = Some(serde_json::json!({ "user_id": uuid::Uuid::new_v4().to_string() }));
        let result = get_user::<SqlxPostGresDescriptor<TestDbHandle>>(fake_meta).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("not found"));
    }
}
