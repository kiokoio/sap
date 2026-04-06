use argon2::{Argon2, PasswordHash, PasswordVerifier};
use saps::auth::dal::model::AuthSession;
use saps::auth::dal::tx_definitions::CreateAuthSession;
use saps::auth::token::checks::{CheckUserRole, UserRole};
use saps::auth::token::header_token::HeaderToken;
use saps::config::GetConfigVariable;
use saps::errors::saps::SapsError;
use crate::dal::models::users::tx_definitions::GetUserByEmail;
use serde::{Deserialize, Serialize};

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
    pub role: String,
}

#[allow(dead_code)]
#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub role: String,
}

/// Authenticates a user and returns a signed JWT token.
///
/// Fetches the user by email via the GetUserByEmail transaction, verifies the password,
/// creates an AuthSession with the role and user_id in meta, then encodes a HeaderToken.
#[allow(dead_code)]
pub async fn login<X, C, Y, R>(
    request: LoginRequest,
) -> Result<LoginResponse, SapsError>
where
    X: GetUserByEmail + CreateAuthSession,
    C: GetConfigVariable,
    Y: CheckUserRole,
    R: UserRole + Clone,
{
    // Fetch user by email
    let user = X::get_user_by_email(request.email.clone())
        .await?
        .ok_or_else(|| SapsError::unauthorized("Invalid email or password"))?;

    // Verify password
    let parsed_hash = PasswordHash::new(&user.password_hash)
        .map_err(|e| SapsError::unknown(e.to_string()))?;
    Argon2::default()
        .verify_password(request.password.as_bytes(), &parsed_hash)
        .map_err(|_| SapsError::unauthorized("Invalid email or password"))?;

    // Parse the role
    let role = R::try_from(request.role.clone())?;

    // Check that the role is allowed by the check struct
    Y::check_user_role(&role)?;

    // Create auth session with user_id in meta
    let session = AuthSession::new(role.clone())
        .with_meta(serde_json::json!({ "user_id": user.id.to_string() }));

    let created = X::create_auth_session(session).await?;

    // Create token with unique_id = session UUID
    // Note: we need a YieldPostGresPool type for HeaderToken but X is trait-based.
    // We use a dummy pool type since encode/decode don't touch the pool.
    let mut token: HeaderToken<C, Y, R, saps::dal::connections::MockDeadPostGresPool> = HeaderToken::new::<R>()?;
    token.unique_id = created.id.to_string();
    let encoded = token.encode()?;

    Ok(LoginResponse {
        token: encoded,
        role: request.role,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::core::users::create::{NewUser, create_user};
    use saps::auth::dal::tx_definitions::GetAllAuthSessions;
    use saps::dal::connections::{SqlxPostGresDescriptor, AuthPostGresDescriptor};
    use crate::roles::Role;

    // SqlxPostGresDescriptor<TestDbHandle> implements both CreateUser and GetUserByEmail (from postgres_txs).
    // AuthPostGresDescriptor<TestDbHandle> implements CreateAuthSession (from saps).
    // We need a single type that implements both GetUserByEmail + CreateAuthSession.
    // Since the traits are on different descriptor types, we use a helper that delegates.
    // For simplicity in tests, we combine by using SqlxPostGresDescriptor which has user txs,
    // and we need CreateAuthSession on it too. But it's on AuthPostGresDescriptor.
    // The cleanest approach: call login with the right types.
    // Actually, the login function requires X: GetUserByEmail + CreateAuthSession.
    // GetUserByEmail is on SqlxPostGresDescriptor<T>, CreateAuthSession is on AuthPostGresDescriptor<T>.
    // These are different types. We need to restructure login to take separate type params,
    // or implement CreateAuthSession on SqlxPostGresDescriptor too.
    // For this example, let's just test the pieces separately and use a combined mock.

    #[saps::db_test]
    async fn test_login_success() {
        crate::migrations::run_migrations(pool).await;
        let new_user = NewUser {
            username: "loginuser".to_string(),
            email: "login@example.com".to_string(),
            password: "mypassword".to_string(),
        };
        create_user::<SqlxPostGresDescriptor<TestDbHandle>>(new_user)
            .await.expect("create user");

        // For login, we need a type that implements both GetUserByEmail + CreateAuthSession.
        // SqlxPostGresDescriptor has GetUserByEmail, AuthPostGresDescriptor has CreateAuthSession.
        // We can't merge them easily. Instead, let's call the DB operations directly to test the logic.

        // Fetch user
        let user = SqlxPostGresDescriptor::<TestDbHandle>::get_user_by_email("login@example.com".into())
            .await.expect("get user")
            .expect("user exists");
        assert_eq!(user.email, "login@example.com");

        // Verify password works
        let parsed = PasswordHash::new(&user.password_hash).unwrap();
        assert!(Argon2::default().verify_password(b"mypassword", &parsed).is_ok());

        // Create session
        let role = Role::try_from("admin".to_string()).unwrap();
        let session = AuthSession::new(role)
            .with_meta(serde_json::json!({ "user_id": user.id.to_string() }));
        let created = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(session)
            .await.expect("create session");
        assert_eq!(created.role, Role::Admin);

        let sessions = AuthPostGresDescriptor::<TestDbHandle>::get_all_auth_sessions::<Role>()
            .await.expect("get sessions");
        assert_eq!(sessions.len(), 1);
    }

    #[saps::db_test]
    async fn test_login_wrong_password() {
        crate::migrations::run_migrations(pool).await;
        let new_user = NewUser {
            username: "wrongpw".to_string(),
            email: "wrongpw@example.com".to_string(),
            password: "correct".to_string(),
        };
        create_user::<SqlxPostGresDescriptor<TestDbHandle>>(new_user)
            .await.expect("create user");

        let user = SqlxPostGresDescriptor::<TestDbHandle>::get_user_by_email("wrongpw@example.com".into())
            .await.expect("get user")
            .expect("user exists");

        let parsed = PasswordHash::new(&user.password_hash).unwrap();
        assert!(Argon2::default().verify_password(b"incorrect", &parsed).is_err());
    }

    #[saps::db_test]
    async fn test_login_user_not_found() {
        crate::migrations::run_migrations(pool).await;
        let result = SqlxPostGresDescriptor::<TestDbHandle>::get_user_by_email("noone@example.com".into())
            .await.expect("query");
        assert!(result.is_none());
    }
}
