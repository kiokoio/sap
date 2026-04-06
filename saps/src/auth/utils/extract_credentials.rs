//! Extracts Basic-Auth credentials from an Axum/Hyper request.
//!
//! This module provides functionality to parse and validate Basic Authentication
//! credentials from HTTP requests. It decodes the Base64-encoded credentials
//! and splits them into email and password.
//!
//! # Purpose
//! - Securely extract user credentials for authentication purposes.
//! - Handle common error cases with appropriate HTTP status codes.
//!
//! # Notes
//! - Assumes the Authorization header is in the format: "Basic <base64(email:password)>".
//! - Returns `SapsError::Unauthorized` for invalid or missing credentials.
use axum::http::{Request, header};
use base64::{Engine, engine::general_purpose};
use crate::errors::saps::SapsError;

/// Represents extracted Basic-Auth credentials.
///
/// # Fields
/// - `email`: The user's email address.
/// - `password`: The user's password (plaintext, as sent in the request).
#[derive(Debug)]
pub struct Credentials {
    pub email: String,
    pub password: String,
}

/// Pulls Basic-Auth credentials out of an Axum/Hyper request.
///
/// This function extracts and decodes the Authorization header, expecting
/// Basic Authentication in the format "Basic <base64(email:password)>".
///
/// # Arguments
/// - `req`: The incoming HTTP request.
///
/// # Returns
/// - `Ok(Credentials)`: If credentials are successfully extracted and valid.
/// - `Err(NanoServiceError)`: If the header is missing, invalid, or decoding fails (with `Unauthorized` status).
///
/// # Errors
/// - Returns `NanoServiceError::Unauthorized` for:
///   - Missing Authorization header.
///   - Invalid prefix (not "Basic ").
///   - Base64 decoding errors.
///   - UTF-8 conversion errors.
///   - Malformed credentials (missing colon or parts).
///   - Empty email or password.
pub fn extract_credentials<B>(req: &Request<B>) -> Result<Credentials, SapsError> {
    // ── 1. Grab the header ────────────────────────────────────────────────
    let header_value = req
        .headers()
        .get(header::AUTHORIZATION)
        .ok_or_else(|| SapsError::unauthorized("No credentials provided".to_string()))?;

    let encoded = header_value
        .to_str()
        .map_err(|_| SapsError::unauthorized("Invalid credentials".to_string()))?;

    // ── 2. Expect the “Basic ” prefix ──────────────────────────────────────
    if !encoded.starts_with("Basic ") {
        return Err(SapsError::unauthorized("Invalid credentials".to_string()));
    }

    // ── 3. Base-64 decode the `email:password` blob ────────────────────────
    let base64_credentials = &encoded[6..];
    let decoded = general_purpose::STANDARD
        .decode(base64_credentials)
        .map_err(|e| SapsError::unauthorized(e.to_string()))?;

    let credentials =
        String::from_utf8(decoded).map_err(|e| SapsError::unauthorized(e.to_string()))?;

    // ── 4. Split into email / password ─────────────────────────────────────
    let mut split = credentials.splitn(2, ':');
    match (split.next(), split.next()) {
        (Some(email), Some(password)) => {
            if email.is_empty() || password.is_empty() {
                return Err(SapsError::unauthorized("Invalid credentials".to_string()));
            }
            Ok(Credentials { email: email.to_owned(), password: password.to_owned() })
        },
        _ => Err(SapsError::unauthorized("Invalid credentials".to_string())),
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use axum::http::{Request, header};
    use crate::errors::saps::SapsErrorStatus;

    fn build_request_with_auth(auth_value: &str) -> Request<()> {
        Request::builder().uri("/").header(header::AUTHORIZATION, auth_value).body(()).unwrap()
    }

    #[test]
    fn test_extract_credentials_success() {
        // "test@example.com:password" base64-encoded is "dGVzdEBleGFtcGxlLmNvbTpwYXNzd29yZA=="
        let req = build_request_with_auth("Basic dGVzdEBleGFtcGxlLmNvbTpwYXNzd29yZA==");
        let credentials = extract_credentials(&req).unwrap();
        assert_eq!(credentials.email, "test@example.com");
        assert_eq!(credentials.password, "password");
    }

    #[test]
    fn test_no_authorization_header() {
        let req = Request::builder().uri("/").body(()).unwrap();
        let err = extract_credentials(&req).unwrap_err();
        assert_eq!(err.status, SapsErrorStatus::Unauthorized);
        assert_eq!(err.message, "No credentials provided");
    }

    #[test]
    fn test_invalid_prefix() {
        let req = build_request_with_auth("Bearer some-token");
        let err = extract_credentials(&req).unwrap_err();
        assert_eq!(err.status, SapsErrorStatus::Unauthorized);
        assert_eq!(err.message, "Invalid credentials");
    }

    #[test]
    fn test_invalid_base64() {
        let req = build_request_with_auth("Basic invalid-base64");
        let err = extract_credentials(&req).unwrap_err();
        assert_eq!(err.status, SapsErrorStatus::Unauthorized);
        assert!(err.message.contains("Invalid symbol 45, offset 7."));
    }

    #[test]
    fn test_invalid_utf8() {
        // Invalid UTF-8 base64: "AA==" decodes to [0], which is invalid UTF-8
        let req = build_request_with_auth("Basic AA==");
        let err = extract_credentials(&req).unwrap_err();
        assert_eq!(err.status, SapsErrorStatus::Unauthorized);
        assert!(err.message.contains("Invalid credentials"));
    }

    #[test]
    fn test_missing_colon() {
        // "testemailpassword" base64-encoded is "dGVzdGVtYWlscGFzc3dvcmQ="
        let req = build_request_with_auth("Basic dGVzdGVtYWlscGFzc3dvcmQ=");
        let err = extract_credentials(&req).unwrap_err();
        assert_eq!(err.status, SapsErrorStatus::Unauthorized);
        assert_eq!(err.message, "Invalid credentials");
    }

    #[test]
    fn test_empty_credentials() {
        // "::" base64-encoded is "Ojo="
        let req = build_request_with_auth("Basic Ojo=");
        let err = extract_credentials(&req).unwrap_err();
        assert_eq!(err.status, SapsErrorStatus::Unauthorized);
        assert_eq!(err.message, "Invalid credentials");
    }
}
