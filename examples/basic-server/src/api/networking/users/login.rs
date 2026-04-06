use saps::axum::{Json, http::StatusCode, response::IntoResponse};
use saps::auth::dal::tx_definitions::CreateAuthSession;
use saps::auth::token::checks::{CheckUserRole, UserRole};
use saps::config::GetConfigVariable;
use crate::api::core::users::login::{LoginRequest, login};
use crate::dal::models::users::tx_definitions::GetUserByEmail;

/// POST /login — authenticates a user and returns a JWT token.
pub async fn login_handler<X, C, Y, R>(
    Json(body): Json<LoginRequest>,
) -> Result<impl IntoResponse, impl IntoResponse>
where
    X: GetUserByEmail + CreateAuthSession,
    C: GetConfigVariable,
    Y: CheckUserRole,
    R: UserRole + Clone,
{
    match login::<X, C, Y, R>(body).await {
        Ok(response) => Ok((StatusCode::OK, Json(response))),
        Err(e) => Err(e),
    }
}

// Note: networking-level login tests require X: GetUserByEmail + CreateAuthSession on a
// single type, but GetUserByEmail is on SqlxPostGresDescriptor and CreateAuthSession is on
// AuthPostGresDescriptor. The login flow is tested at the core level in
// api/core/users/login.rs. To test the full handler, unify the descriptors or implement
// both traits on a single app-level descriptor.
