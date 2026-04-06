//! Defines a generic error.
//!
//! # Notes
//! This is currently a placeholder but happy to talk about the error handling more
use axum::{
    Json,
    http::StatusCode as AxumStatusCode,
    response::{IntoResponse, Response as AxumResponse},
};
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error as ThisError;

/// The response error status for usually a HTTP request.
#[derive(ThisError, Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SapsErrorStatus {
    #[error("Requested resource was not found")]
    NotFound,
    #[error("You are forbidden to access requested resource.")]
    Forbidden,
    #[error("Unknown Internal Error")]
    Unknown,
    #[error("Bad Request")]
    BadRequest,
    #[error("Conflict")]
    Conflict,
    #[error("Unauthorized")]
    Unauthorized,
}

impl SapsErrorStatus {
    /// Constructs an error status from a numeric code.
    ///
    /// # Arguments
    /// * `code` - The numeric code representing the error status.
    ///
    /// # Returns
    /// * `NanoServiceErrorStatus` - The corresponding error status.
    pub fn from_code(code: u16) -> SapsErrorStatus {
        match code {
            404 => SapsErrorStatus::NotFound,
            403 => SapsErrorStatus::Forbidden,
            400 => SapsErrorStatus::BadRequest,
            409 => SapsErrorStatus::Conflict,
            401 => SapsErrorStatus::Unauthorized,
            _ => SapsErrorStatus::Unknown,
        }
    }

    /// Constructs a numeric code from an `Self`.
    ///
    /// # Returns
    /// * `code` - The corresponding error code.
    pub fn to_code(&self) -> u32 {
        match self {
            SapsErrorStatus::NotFound => 404,
            SapsErrorStatus::Forbidden => 403,
            SapsErrorStatus::BadRequest => 400,
            SapsErrorStatus::Conflict => 409,
            SapsErrorStatus::Unauthorized => 401,
            SapsErrorStatus::Unknown => 500,
        }
    }
}

pub type SapsResult<T> = Result<T, SapsError>;

/// The custom error that Actix web automatically converts to a HTTP response.
///
/// # Fields
/// * `message` - The message of the error.
/// * `status` - The status of the error.
#[derive(Serialize, Deserialize, Debug, ThisError, Clone, PartialEq)]
pub struct SapsError {
    pub message: String,
    pub status: SapsErrorStatus,
}

impl SapsError {
    /// Constructs a new error.
    ///
    /// # Arguments
    /// * `message` - The message of the error.
    /// * `status` - The status of the error.
    ///
    /// # Returns
    /// * `CustomError` - The new error.
    pub fn new(message: impl Into<String>, status: SapsErrorStatus) -> SapsError {
        SapsError { message: message.into(), status }
    }

    pub fn not_found(message: impl Into<String>) -> SapsError {
        SapsError { message: message.into(), status: SapsErrorStatus::NotFound }
    }

    pub fn forbidden(message: impl Into<String>) -> SapsError {
        SapsError { message: message.into(), status: SapsErrorStatus::Forbidden }
    }

    pub fn unknown(message: impl Into<String>) -> SapsError {
        SapsError { message: message.into(), status: SapsErrorStatus::Unknown }
    }

    pub fn bad_request(message: impl Into<String>) -> SapsError {
        SapsError { message: message.into(), status: SapsErrorStatus::BadRequest }
    }

    pub fn conflict(message: impl Into<String>) -> SapsError {
        SapsError { message: message.into(), status: SapsErrorStatus::Conflict }
    }

    pub fn unauthorized(message: impl Into<String>) -> SapsError {
        SapsError { message: message.into(), status: SapsErrorStatus::Unauthorized }
    }
}

impl fmt::Display for SapsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl IntoResponse for SapsError {
    fn into_response(self) -> AxumResponse {
        let status_code = match self.status {
            SapsErrorStatus::NotFound => AxumStatusCode::NOT_FOUND,
            SapsErrorStatus::Forbidden => AxumStatusCode::FORBIDDEN,
            SapsErrorStatus::Unknown => AxumStatusCode::INTERNAL_SERVER_ERROR,
            SapsErrorStatus::BadRequest => AxumStatusCode::BAD_REQUEST,
            SapsErrorStatus::Conflict => AxumStatusCode::CONFLICT,
            SapsErrorStatus::Unauthorized => AxumStatusCode::UNAUTHORIZED,
        };

        (status_code, Json(self.message)).into_response()
    }
}

impl From<sqlx::Error> for SapsError {
    fn from(error: sqlx::Error) -> Self {
        match error {
            sqlx::Error::RowNotFound => {
                SapsError::not_found("Resource not found".to_string())
            },
            sqlx::Error::Database(db_err) if db_err.code().as_deref() == Some("23505") => {
                SapsError::conflict("Duplicate entry".to_string())
            },
            sqlx::Error::Database(db_err) if db_err.code().as_deref() == Some("23503") => {
                SapsError::bad_request("Foreign key constraint violation".to_string())
            },
            _ => SapsError::unknown(format!("Database error: {}", error)),
        }
    }
}

impl From<SapsError> for u32 {
    fn from(value: SapsError) -> Self {
        let outcome = match value.status {
            SapsErrorStatus::NotFound => 404,
            SapsErrorStatus::Forbidden => 403,
            SapsErrorStatus::Unknown => 500,
            SapsErrorStatus::BadRequest => 400,
            SapsErrorStatus::Conflict => 409,
            SapsErrorStatus::Unauthorized => 401,
        };
        outcome as u32
    }
}

impl From<rmp_serde::decode::Error> for SapsError {
    fn from(error: rmp_serde::decode::Error) -> Self {
        // Decoding failures indicate invalid client input.
        SapsError::bad_request(format!("MessagePack decode error: {}", error))
    }
}

impl From<rmp_serde::encode::Error> for SapsError {
    fn from(error: rmp_serde::encode::Error) -> Self {
        // Encoding failures typically indicate an internal server issue.
        SapsError::unknown(format!("MessagePack encode error: {}", error))
    }
}

impl From<std::io::Error> for SapsError {
    fn from(value: std::io::Error) -> Self {
        SapsError::unknown(value.to_string())
    }
}
