use saps::auth::dal::tx_definitions::DeleteAuthSession;
use saps::errors::saps::SapsError;

/// Logs out a user by deleting their auth session.
pub async fn logout<X: DeleteAuthSession>(session_id: uuid::Uuid) -> Result<bool, SapsError> {
    let deleted = X::delete_auth_session(session_id).await?;
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::core::users::create::{NewUser, create_user};
    use crate::roles::Role;
    use saps::auth::dal::model::AuthSession;
    use saps::auth::dal::tx_definitions::{CreateAuthSession, GetAllAuthSessions};
    use saps::dal::connections::{SqlxPostGresDescriptor, AuthPostGresDescriptor};

    #[saps::db_test]
    async fn test_logout_deletes_session() {
        crate::migrations::run_migrations(pool).await;
        // Create user
        let new_user = NewUser {
            username: "logoutuser".to_string(),
            email: "logout@example.com".to_string(),
            password: "password".to_string(),
        };
        create_user::<SqlxPostGresDescriptor<TestDbHandle>>(new_user)
            .await.expect("create user");

        // Create a session
        let session = AuthSession::new(Role::Admin);
        let created = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(session)
            .await.expect("create session");

        let sessions = AuthPostGresDescriptor::<TestDbHandle>::get_all_auth_sessions::<Role>()
            .await.expect("get sessions");
        assert_eq!(sessions.len(), 1);

        // Logout
        let deleted = logout::<AuthPostGresDescriptor<TestDbHandle>>(created.id)
            .await.expect("logout");
        assert!(deleted);

        let sessions = AuthPostGresDescriptor::<TestDbHandle>::get_all_auth_sessions::<Role>()
            .await.expect("get sessions");
        assert_eq!(sessions.len(), 0);
    }

    #[saps::db_test]
    async fn test_logout_nonexistent_returns_false() {
        let fake_id = uuid::Uuid::new_v4();
        let deleted = logout::<AuthPostGresDescriptor<TestDbHandle>>(fake_id)
            .await.expect("logout");
        assert!(!deleted);
    }
}
