//! This module houses the token implementation for JWT
// External crate imports
// use crate::{models::auth_sessions::tx_definitions::PingAuthSession, token::checks::CheckUserRole};
use std::marker::PhantomData;
use axum::{
    extract::FromRequestParts,
    http::{
        header::{AUTHORIZATION, SEC_WEBSOCKET_PROTOCOL},
        request::Parts,
        HeaderMap,
    },
};
use chrono::{DateTime, Utc};
use jsonwebtoken::{
    decode,
    encode,
    Algorithm,
    DecodingKey,
    EncodingKey,
    Header,
    Validation,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::{
    auth::{
        dal::tx_definitions::{PingAuthSession, DeleteAuthSession},
        token::checks::{CheckUserRole, UserRole},
    },
    config::GetConfigVariable,
    constants::AUTH_TOKEN_COOKIE_KEY,
    dal::connections::{AuthPostGresDescriptor, YieldPostGresPool},
    errors::saps::SapsError,
};

/// The auth token extracted from the header for logged in users.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HeaderToken<X: GetConfigVariable, Y: CheckUserRole, R: UserRole, Z: YieldPostGresPool> {
    /// The unique id of the token for the auth session.
    pub unique_id: String,
    /// The time the token will expire.
    pub time_expire: DateTime<Utc>,
    /// The marker for the compiler to extract config variables.
    #[serde(skip)]
    pub var_handle: PhantomData<X>,
    /// The marker for the compiler to perform a user role check.
    #[serde(skip)]
    pub role_handle: PhantomData<Y>,
    /// The marker for the compiler to access DB calls.
    #[serde(skip)]
    pub db_handle: PhantomData<Z>,
    /// The marker for the type of user role being checked.
    #[serde(skip)]
    pub role: PhantomData<R>,
    /// Optional session metadata populated from the DB during extraction.
    #[serde(skip)]
    pub meta: Option<serde_json::Value>,
}

impl<X: GetConfigVariable, Y: CheckUserRole, R: UserRole, Z: YieldPostGresPool> HeaderToken<X, Y, R, Z> {
    /// Creates a new token for a user.
    ///
    /// # Notes
    /// - Default for the department ID for this method is no allocation for a department.
    ///
    /// # Returns
    /// * A new token for the user
    pub fn new<U: UserRole>() -> Result<Self, SapsError> {
        let token_expire_mins = match X::get_config_variable("TOKEN_EXPIRE_MINS".into())?.parse::<i64>() {
            Ok(num) => num,
            Err(error) => return Err(SapsError::unknown(error.to_string()))
        };
        Ok(HeaderToken {
            unique_id: Uuid::new_v4().to_string(),
            time_expire: Utc::now() + chrono::Duration::minutes(token_expire_mins),
            var_handle: PhantomData,
            role_handle: PhantomData,
            db_handle: PhantomData,
            role: PhantomData,
            meta: None,
        })
    }

    /// Checks if the token has expired.
    ///
    /// # Returns
    /// * error if the token has expired
    pub fn check_if_expired(&self) -> Result<(), SapsError> {
        if Utc::now() > self.time_expire {
            Err(SapsError::unauthorized("Token has expired".to_string()))
        } else {
            Ok(())
        }
    }

    /// Returns the session meta or an error if it's not present.
    pub fn get_meta(&self) -> Result<&serde_json::Value, SapsError> {
        self.meta.as_ref().ok_or_else(|| SapsError::bad_request("session meta not present"))
    }

    /// Deletes the auth session associated with this token.
    ///
    /// Parses the `unique_id` as a UUID and calls `AuthPostGresDescriptor::<Z>::delete_auth_session`.
    ///
    /// # Returns
    /// * `Ok(true)` if the session was deleted, `Ok(false)` if it didn't exist.
    pub async fn delete_auth_session(&self) -> Result<bool, SapsError> {
        let session_id = uuid::Uuid::parse_str(&self.unique_id)
            .map_err(|e| SapsError::unknown(e.to_string()))?;
        let deleted = AuthPostGresDescriptor::<Z>::delete_auth_session(session_id).await?;
        Ok(deleted)
    }

    /// Encodes the struct into a token.
    ///
    /// # Returns
    /// encoded token with fields of the current struct
    pub fn encode(self) -> Result<String, SapsError> {
        let key_str = X::get_config_variable("SECRET_KEY".to_string())?;
        let key = EncodingKey::from_secret(key_str.as_ref());
        match encode(&Header::default(), &self, &key) {
            Ok(token) => Ok(token),
            Err(error) => Err(SapsError::unauthorized(error.to_string())),
        }
    }

    /// Decodes the token into a struct.
    ///
    /// # Arguments
    /// * `token` - The token to be decoded.
    ///
    /// # Returns
    /// decoded token with fields of the current struct
    pub fn decode(token: &str) -> Result<Self, SapsError> {
        let key_str = <X>::get_config_variable("SECRET_KEY".to_string())?;
        let key = DecodingKey::from_secret(key_str.as_ref());
        let mut validation = Validation::new(Algorithm::HS256);
        validation.required_spec_claims.remove("exp");

        match decode::<Self>(token, &key, &validation) {
            Ok(token_data) => Ok(token_data.claims),
            Err(error) => Err(SapsError::unauthorized(error.to_string())),
        }
    }

    /// Extracts the auth token from </cookies/> using the AUTH_TOKEN_COOKIE_KEY.
    ///
    /// # Arguments
    /// * `headers` - The header map to extract </cookies/> from
    ///
    /// # Returns
    /// * The token string from </cookies/> or an error if not found
    fn extract_token_from_cookies(headers: &HeaderMap) -> Result<Option<String>, SapsError> {
        let cookie_header = match headers.get(axum::http::header::COOKIE) {
            Some(cookies) => cookies,
            None => return Ok(None),
        };

        let cookies_str = cookie_header
            .to_str()
            .map_err(|_| SapsError::unauthorized("Invalid cookie format".to_string()))?;
        Ok(Self::parse_cookie_value(cookies_str, AUTH_TOKEN_COOKIE_KEY))
    }

    /// Extracts the auth token from the 'token' header (original behavior).
    ///
    /// # Arguments
    /// * `headers` - The header map to extract the token from
    ///
    /// # Returns
    /// * The token string from headers orNone if not found
    fn extract_token_from_header(headers: &HeaderMap) -> Result<Option<String>, SapsError> {
        let raw_data = match headers.get("token") {
            Some(token) => token,
            None => return Ok(None),
        };
        let token = raw_data
            .to_str()
            .map_err(|_| SapsError::unauthorized("token not a valid string".to_string()))
            .map(|s| s.to_string())?;
        Ok(Some(token))
    }

    /// Extracts the bearer token from the header
    ///
    /// # Notes
    /// This is used as the last line of defence so if the bearer token is not here then an error is returned
    ///
    /// # Arguments
    /// - `headers`: The headers of the request
    ///
    /// # Returns
    /// The raw token from the bearer
    pub fn extract_bearer_token(headers: &HeaderMap) -> Result<String, SapsError> {
        // Prefer subprotocol: Sec-WebSocket-Protocol: bearer, <JWT>
        if let Some(raw) = headers.get(SEC_WEBSOCKET_PROTOCOL) {
            let s = raw.to_str().map_err(|_| {
                SapsError::unauthorized("Invalid Sec-WebSocket-Protocol header")
            })?;

            if let Some((p1, p2)) = s.split_once(',')
                && p1.trim().eq_ignore_ascii_case("bearer")
            {
                let jwt = p2.trim();
                if !jwt.is_empty() {
                    return Ok(jwt.to_owned());
                }
            }
        }

        // Fallback: Authorization: Bearer <token> (unchanged from your original)
        let raw = headers
            .get(AUTHORIZATION)
            .ok_or_else(|| SapsError::unauthorized("Missing Authorization header"))?;

        let s = raw
            .to_str()
            .map_err(|_| SapsError::unauthorized("Invalid Authorization header"))?;

        let mut parts = s.split_whitespace();
        let scheme = parts.next().unwrap_or("");
        let token = parts.next().unwrap_or("");

        if !scheme.eq_ignore_ascii_case("bearer") || token.is_empty() {
            return Err(SapsError::unauthorized("Expected 'Bearer <token>'"));
        }
        Ok(token.to_owned())
    }

    /// Parses cookie value from cookie string - helper method for cookie extraction.
    ///
    /// # Arguments
    /// * `cookies` - The </cookies/> in string format from the header
    /// * `target_name` - The name of the cookie variable to extract
    ///
    /// # Returns
    /// * The value of the cookie if present
    fn parse_cookie_value(cookies: &str, target_name: &str) -> Option<String> {
        cookies
            .split(';')
            .filter_map(|cookie| {
                let cookie = cookie.trim();
                cookie.split_once('=')
            })
            .find(|(name, _)| name.trim() == target_name)
            .map(|(_, value)| value.trim().to_string())
    }
}

impl<S, X, Y, R, Z> FromRequestParts<S> for HeaderToken<X, Y, R, Z>
where
    S: Send + Sync,                     // router state type
    X: GetConfigVariable + Send + Sync, // config provider strategy
    Y: CheckUserRole + Send + Sync,     // role‑check strategy
    Z: YieldPostGresPool + Send + Sync,
    R: UserRole + Send + Sync
{
    type Rejection = SapsError;

    /// This function fires before the API request function is loaded.
    ///
    /// # Arguments
    /// * `parts` - The request parts for axum.
    ///
    /// # Returns
    /// * Token or Nanoservice Error
    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // extract the token from the header
        let headers = &parts.headers;

        // extract from </cookies/> and attempt to get token from header and lastly the bearer as a fallback
        let raw_token = match Self::extract_token_from_cookies(headers)? {
            Some(token) => token,
            None => match Self::extract_token_from_header(headers)? {
                Some(token) => token,
                None => Self::extract_bearer_token(headers)?,
            },
        };

        // decode the token and perform role and device checks
        let token = Self::decode(&raw_token)?;

        // per‑request checks
        let session = match AuthPostGresDescriptor::<Z>::ping_auth_session::<R>(
            10, &token.unique_id
        ).await? {
            Some(session) => session,
            None => return Err(SapsError::unauthorized("session not present"))
        };
        Y::check_user_role(&session.role)?;
        let mut token = token;
        token.meta = session.meta;
        // TODO => implement expire logic
        // if let Err(e) = token.check_if_expired() {
        //     return err(e);
        // }
        Ok(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::dal::tx_definitions::CreateAuthSession;
    use crate::{
        auth::dal::{
            model::AuthSession,
        },
        dal::connections::MockDeadPostGresPool,
        errors::saps::SapsErrorStatus,
    };
    use crate::auth::token::checks::{
        AdminRoleCheck, CustomerRoleCheck, ExactAdminRoleCheck, NoRoleCheck,
        SuperAdminRoleCheck,
    };
    use axum::{
        Json, Router,
        body::{self, Body, Bytes},
        http::{HeaderValue, Request, StatusCode},
        response::IntoResponse,
        routing::get,
    };
    use serde_json::json;
    use tower::ServiceExt;

    // -- Test role enum --
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
    enum TestRole {
        SuperAdmin,
        Admin,
        Customer,
    }

    impl std::fmt::Display for TestRole {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                TestRole::SuperAdmin => write!(f, "superadmin"),
                TestRole::Admin => write!(f, "admin"),
                TestRole::Customer => write!(f, "customer"),
            }
        }
    }

    impl TryFrom<String> for TestRole {
        type Error = SapsError;
        fn try_from(value: String) -> Result<Self, Self::Error> {
            match value.to_lowercase().as_str() {
                "superadmin" => Ok(TestRole::SuperAdmin),
                "admin" => Ok(TestRole::Admin),
                "customer" => Ok(TestRole::Customer),
                _ => Err(SapsError::bad_request(format!("Unknown role: {}", value))),
            }
        }
    }

    impl UserRole for TestRole {}

    // -- Fake config that returns hardcoded values --
    #[derive(Clone)]
    struct FakeConfig;

    impl GetConfigVariable for FakeConfig {
        fn get_config_variable(variable: String) -> Result<String, SapsError> {
            match variable.as_str() {
                "SECRET_KEY" => Ok("test_secret".to_string()),
                "TOKEN_EXPIRE_MINS" => Ok("20".to_string()),
                _ => Err(SapsError::unknown(format!("key: {} was not found", variable))),
            }
        }
    }

    // -- Type aliases for HeaderToken variants --
    type TkNo = HeaderToken<FakeConfig, NoRoleCheck, TestRole, MockDeadPostGresPool>;

    // -- Helper to construct a token --
    fn construct_token() -> TkNo {
        HeaderToken::<FakeConfig, NoRoleCheck, TestRole, MockDeadPostGresPool>::new::<TestRole>().unwrap()
    }

    // -- Handlers --
    async fn pass_handle(tok: TkNo) -> impl IntoResponse {
        Json(json!({ "unique_id": tok.unique_id }))
    }

    // -- Helper to send a request and collect (status, body) --
    async fn send(app: &Router, req: Request<Body>) -> (StatusCode, Bytes) {
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let body = body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        (status, body)
    }

    // ===== Unit tests =====

    #[test]
    fn test_encode_decode() {
        let jwt = construct_token();
        let encoded = jwt.encode().unwrap();
        let decoded = TkNo::decode(&encoded).unwrap();
        assert!(!decoded.unique_id.is_empty());
    }

    #[test]
    fn test_decode_preserves_unique_id() {
        let jwt = construct_token();
        let original_id = jwt.unique_id.clone();
        let raw = jwt.encode().unwrap();
        let decoded = TkNo::decode(&raw).unwrap();
        assert_eq!(decoded.unique_id, original_id);
    }

    #[test]
    fn test_check_if_expired_not_expired() {
        let jwt = construct_token();
        assert!(jwt.check_if_expired().is_ok());
    }

    #[test]
    fn test_check_if_expired_is_expired() {
        let mut jwt = construct_token();
        jwt.time_expire = Utc::now() - chrono::Duration::minutes(1);
        let err = jwt.check_if_expired().unwrap_err();
        assert_eq!(err.status, SapsErrorStatus::Unauthorized);
    }

    // ===== Bearer / header extraction tests =====

    #[test]
    fn test_extract_bearer_token_from_authorization() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer abc123"));
        let token = TkNo::extract_bearer_token(&headers).expect("token");
        assert_eq!(token, "abc123");
    }

    #[test]
    fn test_extract_bearer_case_insensitive() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("bearer   T0KEN"));
        let token = TkNo::extract_bearer_token(&headers).expect("token");
        assert_eq!(token, "T0KEN");
    }

    #[test]
    fn test_missing_authorization_header() {
        let headers = HeaderMap::new();
        let err = TkNo::extract_bearer_token(&headers).expect_err("expected error");
        assert_eq!(err.status, SapsErrorStatus::Unauthorized);
        assert!(err.message.contains("Missing Authorization header"));
    }

    #[test]
    fn test_wrong_scheme_is_unauthorized() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Basic Zm9vOmJhcg=="));
        let err = TkNo::extract_bearer_token(&headers).expect_err("expected error");
        assert_eq!(err.status, SapsErrorStatus::Unauthorized);
        assert!(err.message.contains("Expected 'Bearer <token>'"));
    }

    #[test]
    fn test_bearer_without_token_is_unauthorized() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer"));
        let err = TkNo::extract_bearer_token(&headers).expect_err("expected error");
        assert_eq!(err.status, SapsErrorStatus::Unauthorized);
    }

    #[test]
    fn ws_subprotocol_bearer_simple() {
        let mut headers = HeaderMap::new();
        headers.insert(SEC_WEBSOCKET_PROTOCOL, HeaderValue::from_static("bearer, jwt123"));
        let token = TkNo::extract_bearer_token(&headers).expect("token");
        assert_eq!(token, "jwt123");
    }

    #[test]
    fn ws_subprotocol_bearer_case_insensitive_and_spaces() {
        let mut headers = HeaderMap::new();
        headers.insert(SEC_WEBSOCKET_PROTOCOL, HeaderValue::from_static("BeArEr,    42-XYZ "));
        let token = TkNo::extract_bearer_token(&headers).expect("token");
        assert_eq!(token, "42-XYZ");
    }

    #[test]
    fn ws_subprotocol_non_bearer_falls_back_to_authorization() {
        let mut headers = HeaderMap::new();
        headers.insert(
            SEC_WEBSOCKET_PROTOCOL,
            HeaderValue::from_static("somethingelse, token-ignored"),
        );
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer real-token"));
        let token = TkNo::extract_bearer_token(&headers).expect("token");
        assert_eq!(token, "real-token");
    }

    // ===== Extractor-through-router tests =====

    #[tokio::test]
    async fn test_fail_no_token() {
        let app = Router::new().route("/", get(pass_handle));
        let req = Request::builder()
            .uri("/")
            .body(Body::empty())
            .unwrap();
        let (status, body) = send(&app, req).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body, Bytes::from_static(b"\"Missing Authorization header\""));
    }

    // ===== Handler + extractor tests via #[db_test] =====
    // The extractor calls AuthPostGresDescriptor::<Z>::ping_auth_session, so Z must be
    // TestDbHandle (provided by #[db_test]) for a working pool. Since TestDbHandle is
    // only available inside the #[db_test] block, we use a macro to define the handler,
    // token type, and test body together.

    macro_rules! db_handler_test {
        ($test_name:ident, $check:ty, $role:expr, $expected_status:expr) => {
            #[saps::db_test]
            async fn $test_name() {
                type Tk = HeaderToken<FakeConfig, $check, TestRole, TestDbHandle>;

                async fn handler(tok: Tk) -> impl IntoResponse {
                    Json(json!({ "unique_id": tok.unique_id }))
                }

                let token: Tk = HeaderToken::new::<TestRole>().unwrap();
                let mut session = AuthSession::new($role);
                session.id = uuid::Uuid::parse_str(&token.unique_id).unwrap();
                AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(session)
                    .await
                    .expect("failed to create session");

                let app = Router::new().route("/", get(handler));
                let req = Request::builder()
                    .uri("/")
                    .header("token", token.encode().unwrap())
                    .body(Body::empty())
                    .unwrap();
                let (status, _body) = send(&app, req).await;
                assert_eq!(status, $expected_status);
            }
        };
    }

    db_handler_test!(test_pass_no_role_check, NoRoleCheck, TestRole::Admin, StatusCode::OK);
    db_handler_test!(test_pass_admin_check, AdminRoleCheck, TestRole::Admin, StatusCode::OK);
    db_handler_test!(test_pass_customer_check, CustomerRoleCheck, TestRole::Admin, StatusCode::OK);
    db_handler_test!(test_fail_super_admin_check, SuperAdminRoleCheck, TestRole::Admin, StatusCode::UNAUTHORIZED);
    db_handler_test!(test_pass_exact_admin_check, ExactAdminRoleCheck, TestRole::Admin, StatusCode::OK);

    // ===== Named handler tests using TkAdm/TkSup/TkCus/TkExa =====
    // These use the db_handler_test macro with explicit handler functions and the
    // MockDeadPostGresPool-based type aliases for encode/decode, while the actual
    // extractor runs against TestDbHandle via #[db_test].

    #[saps::db_test]
    async fn test_admin_handle_passes_for_admin_role() {
        type Tk = HeaderToken<FakeConfig, AdminRoleCheck, TestRole, TestDbHandle>;
        async fn handler(tok: Tk) -> impl IntoResponse {
            Json(json!({ "unique_id": tok.unique_id }))
        }

        let token: Tk = HeaderToken::new::<TestRole>().unwrap();
        let mut session = AuthSession::new(TestRole::Admin);
        session.id = uuid::Uuid::parse_str(&token.unique_id).unwrap();
        AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(session)
            .await.expect("failed to create session");

        let app = Router::new().route("/", get(handler));
        let req = Request::builder()
            .uri("/")
            .header("token", token.encode().unwrap())
            .body(Body::empty())
            .unwrap();
        assert_eq!(send(&app, req).await.0, StatusCode::OK);
    }

    #[saps::db_test]
    async fn test_admin_handle_rejects_customer_role() {
        type Tk = HeaderToken<FakeConfig, AdminRoleCheck, TestRole, TestDbHandle>;
        async fn handler(tok: Tk) -> impl IntoResponse {
            Json(json!({ "unique_id": tok.unique_id }))
        }

        let token: Tk = HeaderToken::new::<TestRole>().unwrap();
        let mut session = AuthSession::new(TestRole::Customer);
        session.id = uuid::Uuid::parse_str(&token.unique_id).unwrap();
        AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(session)
            .await.expect("failed to create session");

        let app = Router::new().route("/", get(handler));
        let req = Request::builder()
            .uri("/")
            .header("token", token.encode().unwrap())
            .body(Body::empty())
            .unwrap();
        let (status, body) = send(&app, req).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body, Bytes::from_static(b"\"Role does not have sufficient permissions\""));
    }

    #[saps::db_test]
    async fn test_super_admin_handle_passes_for_super_admin_role() {
        type Tk = HeaderToken<FakeConfig, SuperAdminRoleCheck, TestRole, TestDbHandle>;
        async fn handler(tok: Tk) -> impl IntoResponse {
            Json(json!({ "unique_id": tok.unique_id }))
        }

        let token: Tk = HeaderToken::new::<TestRole>().unwrap();
        let mut session = AuthSession::new(TestRole::SuperAdmin);
        session.id = uuid::Uuid::parse_str(&token.unique_id).unwrap();
        AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(session)
            .await.expect("failed to create session");

        let app = Router::new().route("/", get(handler));
        let req = Request::builder()
            .uri("/")
            .header("token", token.encode().unwrap())
            .body(Body::empty())
            .unwrap();
        assert_eq!(send(&app, req).await.0, StatusCode::OK);
    }

    #[saps::db_test]
    async fn test_super_admin_handle_rejects_admin_role() {
        type Tk = HeaderToken<FakeConfig, SuperAdminRoleCheck, TestRole, TestDbHandle>;
        async fn handler(tok: Tk) -> impl IntoResponse {
            Json(json!({ "unique_id": tok.unique_id }))
        }

        let token: Tk = HeaderToken::new::<TestRole>().unwrap();
        let mut session = AuthSession::new(TestRole::Admin);
        session.id = uuid::Uuid::parse_str(&token.unique_id).unwrap();
        AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(session)
            .await.expect("failed to create session");

        let app = Router::new().route("/", get(handler));
        let req = Request::builder()
            .uri("/")
            .header("token", token.encode().unwrap())
            .body(Body::empty())
            .unwrap();
        let (status, body) = send(&app, req).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body, Bytes::from_static(b"\"Role does not have sufficient permissions\""));
    }

    #[saps::db_test]
    async fn test_customer_handle_passes_for_all_roles() {
        type Tk = HeaderToken<FakeConfig, CustomerRoleCheck, TestRole, TestDbHandle>;
        async fn handler(tok: Tk) -> impl IntoResponse {
            Json(json!({ "unique_id": tok.unique_id }))
        }

        for role in [TestRole::SuperAdmin, TestRole::Admin, TestRole::Customer] {
            let token: Tk = HeaderToken::new::<TestRole>().unwrap();
            let mut session = AuthSession::new(role);
            session.id = uuid::Uuid::parse_str(&token.unique_id).unwrap();
            AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(session)
                .await.expect("failed to create session");

            let app = Router::new().route("/", get(handler));
            let req = Request::builder()
                .uri("/")
                .header("token", token.encode().unwrap())
                .body(Body::empty())
                .unwrap();
            assert_eq!(send(&app, req).await.0, StatusCode::OK);
        }
    }

    #[saps::db_test]
    async fn test_exact_admin_handle_rejects_super_admin() {
        type Tk = HeaderToken<FakeConfig, ExactAdminRoleCheck, TestRole, TestDbHandle>;
        async fn handler(tok: Tk) -> impl IntoResponse {
            Json(json!({ "unique_id": tok.unique_id }))
        }

        let token: Tk = HeaderToken::new::<TestRole>().unwrap();
        let mut session = AuthSession::new(TestRole::SuperAdmin);
        session.id = uuid::Uuid::parse_str(&token.unique_id).unwrap();
        AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(session)
            .await.expect("failed to create session");

        let app = Router::new().route("/", get(handler));
        let req = Request::builder()
            .uri("/")
            .header("token", token.encode().unwrap())
            .body(Body::empty())
            .unwrap();
        let (status, body) = send(&app, req).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body, Bytes::from_static(b"\"Role does not have sufficient permissions\""));
    }

    #[saps::db_test]
    async fn test_exact_admin_handle_rejects_customer() {
        type Tk = HeaderToken<FakeConfig, ExactAdminRoleCheck, TestRole, TestDbHandle>;
        async fn handler(tok: Tk) -> impl IntoResponse {
            Json(json!({ "unique_id": tok.unique_id }))
        }

        let token: Tk = HeaderToken::new::<TestRole>().unwrap();
        let mut session = AuthSession::new(TestRole::Customer);
        session.id = uuid::Uuid::parse_str(&token.unique_id).unwrap();
        AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(session)
            .await.expect("failed to create session");

        let app = Router::new().route("/", get(handler));
        let req = Request::builder()
            .uri("/")
            .header("token", token.encode().unwrap())
            .body(Body::empty())
            .unwrap();
        let (status, body) = send(&app, req).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body, Bytes::from_static(b"\"Role does not have sufficient permissions\""));
    }
}
