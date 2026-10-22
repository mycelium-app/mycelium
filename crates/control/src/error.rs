use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mycelium_proto::api::ErrorBody;

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    error: &'static str,
    message: String,
}

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub fn new(status: StatusCode, error: &'static str, message: impl Into<String>) -> ApiError {
        ApiError { status, error, message: message.into() }
    }

    pub fn unauthorised() -> ApiError {
        ApiError::new(StatusCode::UNAUTHORIZED, "unauthorised", "Sign in to continue.")
    }

    pub fn bad_request(error: &'static str, message: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::BAD_REQUEST, error, message)
    }

    pub fn not_found(what: &str) -> ApiError {
        ApiError::new(StatusCode::NOT_FOUND, "not_found", format!("{what} was not found. Check the address and try again."))
    }

    pub fn conflict(error: &'static str, message: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::CONFLICT, error, message)
    }

    pub fn internal(cause: impl std::fmt::Display) -> ApiError {
        tracing::error!("{cause}");

        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", "Something went wrong on the server. Try again in a moment.")
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(cause: sqlx::Error) -> ApiError {
        ApiError::internal(cause)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(ErrorBody { error: self.error.to_string(), message: self.message })).into_response()
    }
}
