use saps::axum::{Json, http::StatusCode, response::IntoResponse};
use saps::auth::token::checks::{CheckUserRole, UserRole};
use saps::auth::token::header_token::HeaderToken;
use saps::config::GetConfigVariable;
use saps::dal::connections::{AuthPostGresDescriptor, YieldPostGresPool};
use saps::auth::dal::tx_definitions::PingAuthSession;
use crate::api::core::users::get::get_user;
use crate::dal::models::users::tx_definitions::GetUserById;

/// GET /users/me — returns the authenticated user's profile.
/// The HeaderToken extractor validates the session. We then ping the session
/// to retrieve the meta (which contains user_id) and look up the user.
pub async fn get_user_handler<X, C, Y, R, Z>(
    token: HeaderToken<C, Y, R, Z>,
) -> Result<impl IntoResponse, impl IntoResponse>
where
    X: GetUserById,
    C: GetConfigVariable + Send + Sync,
    Y: CheckUserRole + Send + Sync,
    R: UserRole + Send + Sync,
    Z: YieldPostGresPool + Send + Sync,
{
    // Ping the session to get meta with user_id
    let session = AuthPostGresDescriptor::<Z>::ping_auth_session::<R>(10, &token.unique_id)
        .await
        .map_err(|e| saps::errors::saps::SapsError::unknown(e.to_string()))?
        .ok_or_else(|| saps::errors::saps::SapsError::unauthorized("session not found"))?;

    match get_user::<X>(session.meta).await {
        Ok(user) => Ok((StatusCode::OK, Json(user))),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::core::users::create::{NewUser, create_user};
    use crate::roles::{Role, NoRoleCheck};
    use saps::auth::dal::model::AuthSession;
    use saps::auth::dal::tx_definitions::CreateAuthSession;
    use saps::axum::{Router, body::{self, Body, Bytes}, http::{Request, StatusCode}, routing::get};
    use saps::dal::connections::{AuthPostGresDescriptor, SqlxPostGresDescriptor};
    use saps::errors::saps::SapsError;
    use tower::ServiceExt;

    #[derive(Clone)]
    struct TestConfig;

    impl GetConfigVariable for TestConfig {
        fn get_config_variable(variable: String) -> Result<String, SapsError> {
            match variable.as_str() {
                "SECRET_KEY" => Ok("test_secret".to_string()),
                "TOKEN_EXPIRE_MINS" => Ok("20".to_string()),
                _ => Err(SapsError::unknown(format!("{} not found", variable))),
            }
        }
    }

    async fn send(app: &Router, req: Request<Body>) -> (StatusCode, Bytes) {
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        (status, bytes)
    }

    #[saps::db_test]
    async fn test_get_user_endpoint() {
        crate::migrations::run_migrations(pool).await;

        // Create a user
        let new_user = NewUser {
            username: "getnetuser".to_string(),
            email: "getnet@example.com".to_string(),
            password: "password".to_string(),
        };
        let user = create_user::<SqlxPostGresDescriptor<TestDbHandle>>(new_user)
            .await.expect("create user");

        // Create auth session with user_id in meta
        let session = AuthSession::new(Role::Admin)
            .with_meta(serde_json::json!({ "user_id": user.id.to_string() }));
        let created_session = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(session)
            .await.expect("create session");

        // Build a token whose unique_id matches the session
        type Tk = HeaderToken<TestConfig, NoRoleCheck, Role, TestDbHandle>;
        let mut token: Tk = HeaderToken::new::<Role>().unwrap();
        token.unique_id = created_session.id.to_string();
        let encoded = token.encode().unwrap();

        // Mount the handler
        async fn handler(
            token: HeaderToken<TestConfig, NoRoleCheck, Role, TestDbHandle>,
        ) -> Result<impl IntoResponse, impl IntoResponse> {
            get_user_handler::<SqlxPostGresDescriptor<TestDbHandle>, TestConfig, NoRoleCheck, Role, TestDbHandle>(token).await
        }

        let app = Router::new().route("/users/me", get(handler));

        let req = Request::builder()
            .method("GET")
            .uri("/users/me")
            .header("token", encoded)
            .body(Body::empty())
            .unwrap();

        let (status, body) = send(&app, req).await;
        assert_eq!(status, StatusCode::OK);

        let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(resp["username"], "getnetuser");
        assert_eq!(resp["email"], "getnet@example.com");
    }

    #[saps::db_test]
    async fn test_get_user_endpoint_no_session() {
        crate::migrations::run_migrations(pool).await;

        // Build a token with a non-existent session
        type Tk = HeaderToken<TestConfig, NoRoleCheck, Role, TestDbHandle>;
        let token: Tk = HeaderToken::new::<Role>().unwrap();
        let encoded = token.encode().unwrap();

        async fn handler(
            token: HeaderToken<TestConfig, NoRoleCheck, Role, TestDbHandle>,
        ) -> Result<impl IntoResponse, impl IntoResponse> {
            get_user_handler::<SqlxPostGresDescriptor<TestDbHandle>, TestConfig, NoRoleCheck, Role, TestDbHandle>(token).await
        }

        let app = Router::new().route("/users/me", get(handler));

        let req = Request::builder()
            .method("GET")
            .uri("/users/me")
            .header("token", encoded)
            .body(Body::empty())
            .unwrap();

        let (status, _) = send(&app, req).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
}
