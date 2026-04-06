use saps::axum::{Json, http::StatusCode, response::IntoResponse};
use saps::auth::dal::tx_definitions::DeleteAuthSession;
use saps::auth::token::checks::{CheckUserRole, UserRole};
use saps::auth::token::header_token::HeaderToken;
use saps::config::GetConfigVariable;
use saps::dal::connections::YieldPostGresPool;
use crate::api::core::users::logout::logout;

/// POST /logout — deletes the caller's auth session.
/// The HeaderToken extractor validates the session before this runs.
pub async fn logout_handler<X, C, Y, R, Z>(
    token: HeaderToken<C, Y, R, Z>,
) -> Result<impl IntoResponse, impl IntoResponse>
where
    X: DeleteAuthSession,
    C: GetConfigVariable + Send + Sync,
    Y: CheckUserRole + Send + Sync,
    R: UserRole + Send + Sync,
    Z: YieldPostGresPool + Send + Sync,
{
    let session_id = uuid::Uuid::parse_str(&token.unique_id)
        .map_err(|e| saps::errors::saps::SapsError::unknown(e.to_string()))?;
    match logout::<X>(session_id).await {
        Ok(_) => Ok((StatusCode::OK, Json(serde_json::json!({"message": "logged out"})))),
        Err(e) => Err(e),
    }
}
