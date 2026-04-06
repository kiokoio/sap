use saps::axum::{Json, http::StatusCode, response::IntoResponse};
use crate::api::core::users::create::{NewUser, create_user};
use crate::dal::models::users::tx_definitions::CreateUser;

/// POST /users — creates a new user.
pub async fn create_user_handler<X: CreateUser>(
    Json(body): Json<NewUser>,
) -> Result<impl IntoResponse, impl IntoResponse> {
    match create_user::<X>(body).await {
        Ok(user) => Ok((StatusCode::CREATED, Json(user))),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use saps::axum::{Router, body::{self, Body, Bytes}, http::{Request, StatusCode}, routing::post};
    use saps::dal::connections::SqlxPostGresDescriptor;
    use tower::ServiceExt;

    async fn send(app: &Router, req: Request<Body>) -> (StatusCode, Bytes) {
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        (status, bytes)
    }

    #[saps::db_test]
    async fn test_create_user_endpoint() {
        crate::migrations::run_migrations(pool).await;
        let app = Router::new()
            .route("/users", post(create_user_handler::<SqlxPostGresDescriptor<TestDbHandle>>));

        let req = Request::builder()
            .method("POST")
            .uri("/users")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"username":"httpuser","email":"http@example.com","password":"pass123"}"#))
            .unwrap();

        let (status, body) = send(&app, req).await;
        assert_eq!(status, StatusCode::CREATED);

        let user: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(user["username"], "httpuser");
        assert_eq!(user["email"], "http@example.com");
    }

    #[saps::db_test]
    async fn test_create_duplicate_user_endpoint() {
        crate::migrations::run_migrations(pool).await;
        let app = Router::new()
            .route("/users", post(create_user_handler::<SqlxPostGresDescriptor<TestDbHandle>>));

        let body_json = r#"{"username":"dup","email":"dup@example.com","password":"pass"}"#;

        let req = Request::builder()
            .method("POST")
            .uri("/users")
            .header("content-type", "application/json")
            .body(Body::from(body_json))
            .unwrap();
        let (status, _) = send(&app, req).await;
        assert_eq!(status, StatusCode::CREATED);

        let req = Request::builder()
            .method("POST")
            .uri("/users")
            .header("content-type", "application/json")
            .body(Body::from(body_json))
            .unwrap();
        let (status, _) = send(&app, req).await;
        assert_eq!(status, StatusCode::CONFLICT);
    }
}
