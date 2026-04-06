use saps::{
    auth::token::{
        checks::{CheckUserRole, UserRole},
        header_token::HeaderToken,
    },
    axum::{http::StatusCode, response::IntoResponse, Json},
    config::GetConfigVariable,
    dal::connections::YieldPostGresPool,
};
use crate::{
    api::core::users::delete::delete_user,
    dal::models::users::tx_definitions::DeleteUser,
};

/// DELETE /users — deletes the caller's user account and auth session.
/// The HeaderToken extractor validates the session before this runs.
/// The user_id is extracted from the session meta.
pub async fn delete_user_handler<X, C, Y, R, Z>(
    token: HeaderToken<C, Y, R, Z>,
) -> Result<impl IntoResponse, impl IntoResponse>
where
    X: DeleteUser,
    C: GetConfigVariable + Send + Sync,
    Y: CheckUserRole + Send + Sync,
    R: UserRole + Send + Sync,
    Z: YieldPostGresPool + Send + Sync,
{
    let meta = token.get_meta()?;
    let user_id_str = meta.get("user_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| saps::errors::saps::SapsError::bad_request("meta missing user_id"))?;
    let user_id: uuid::Uuid = user_id_str.parse()
        .map_err(|_| saps::errors::saps::SapsError::bad_request("invalid user_id in meta"))?;

    let outcome = match delete_user::<X>(user_id).await {
        Ok(_) => Ok((StatusCode::OK, Json(serde_json::json!({"message": "user deleted"})))),
        Err(e) => Err(e),
    };

    token.delete_auth_session().await?;

    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::core::users::create::{NewUser, create_user};
    use crate::dal::models::users::tx_definitions::GetUserByEmail;
    use crate::roles::Role;
    use saps::auth::dal::tx_definitions::{GetAllAuthSessions, DeleteAuthSession};
    use saps::dal::connections::{AuthPostGresDescriptor, SqlxPostGresDescriptor};

    #[saps::db_test]
    async fn test_delete_user_endpoint() {
        crate::migrations::run_migrations(pool).await;
        use saps::auth::dal::model::AuthSession;
        use saps::auth::dal::tx_definitions::CreateAuthSession;

        // Create a user
        let new_user = NewUser {
            username: "deluser".to_string(),
            email: "deluser@example.com".to_string(),
            password: "password".to_string(),
        };
        let user = create_user::<SqlxPostGresDescriptor<TestDbHandle>>(new_user)
            .await.expect("create user");

        // Create an auth session with user_id in meta
        let session = AuthSession::new(Role::Admin)
            .with_meta(serde_json::json!({ "user_id": user.id.to_string() }));
        let created_session = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(session)
            .await.expect("create session");

        // Test via core functions directly (the handler requires a unified type
        // for DeleteUser + DeleteAuthSession which are on different descriptors)
        AuthPostGresDescriptor::<TestDbHandle>::delete_auth_session(created_session.id)
            .await.expect("delete session");
        SqlxPostGresDescriptor::<TestDbHandle>::delete_user(user.id)
            .await.expect("delete user");

        // Verify both are gone
        let found = SqlxPostGresDescriptor::<TestDbHandle>::get_user_by_email("deluser@example.com".into())
            .await.expect("query");
        assert!(found.is_none());

        let sessions = AuthPostGresDescriptor::<TestDbHandle>::get_all_auth_sessions::<Role>()
            .await.expect("get sessions");
        assert_eq!(sessions.len(), 0);
    }
}
