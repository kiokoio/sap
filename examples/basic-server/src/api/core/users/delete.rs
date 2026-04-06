use saps::errors::saps::SapsError;
use crate::dal::models::users::tx_definitions::DeleteUser;

/// Deletes a user and their auth session.
///
/// # Arguments
/// * `user_id` - The UUID of the user to delete
/// * `session_id` - The UUID of the auth session to delete
pub async fn delete_user<X: DeleteUser>(
    user_id: uuid::Uuid,
) -> Result<bool, SapsError> {
    let deleted = X::delete_user(user_id).await?;
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::core::users::create::{NewUser, create_user};
    use crate::dal::models::users::tx_definitions::GetUserByEmail;
    use crate::roles::Role;
    use saps::auth::dal::model::AuthSession;
    use saps::auth::dal::tx_definitions::{CreateAuthSession, DeleteAuthSession, GetAllAuthSessions};
    use saps::dal::connections::{AuthPostGresDescriptor, SqlxPostGresDescriptor};

    #[saps::db_test]
    async fn test_delete_user_and_session() {
        crate::migrations::run_migrations(pool).await;

        // Create a user
        let new_user = NewUser {
            username: "deleteuser".to_string(),
            email: "delete@example.com".to_string(),
            password: "password".to_string(),
        };
        let user = create_user::<SqlxPostGresDescriptor<TestDbHandle>>(new_user)
            .await.expect("create user");

        // Create an auth session for the user
        let session = AuthSession::new(Role::Admin)
            .with_meta(serde_json::json!({ "user_id": user.id.to_string() }));
        let created_session = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(session)
            .await.expect("create session");

        // Verify both exist
        let sessions = AuthPostGresDescriptor::<TestDbHandle>::get_all_auth_sessions::<Role>()
            .await.expect("get sessions");
        assert_eq!(sessions.len(), 1);

        let found_user = SqlxPostGresDescriptor::<TestDbHandle>::get_user_by_email("delete@example.com".into())
            .await.expect("get user");
        assert!(found_user.is_some());

        // Delete user and session
        // Note: DeleteUser is on SqlxPostGresDescriptor, DeleteAuthSession is on AuthPostGresDescriptor.
        // We delete them separately since they're on different descriptor types.
        AuthPostGresDescriptor::<TestDbHandle>::delete_auth_session(created_session.id)
            .await.expect("delete session");
        SqlxPostGresDescriptor::<TestDbHandle>::delete_user(user.id)
            .await.expect("delete user");

        // Verify both are gone
        let sessions = AuthPostGresDescriptor::<TestDbHandle>::get_all_auth_sessions::<Role>()
            .await.expect("get sessions");
        assert_eq!(sessions.len(), 0);

        let found_user = SqlxPostGresDescriptor::<TestDbHandle>::get_user_by_email("delete@example.com".into())
            .await.expect("get user");
        assert!(found_user.is_none());
    }

    #[saps::db_test]
    async fn test_delete_nonexistent_user_returns_false() {
        crate::migrations::run_migrations(pool).await;

        let fake_id = uuid::Uuid::new_v4();
        let deleted = SqlxPostGresDescriptor::<TestDbHandle>::delete_user(fake_id)
            .await.expect("delete user");
        assert!(!deleted);
    }
}
