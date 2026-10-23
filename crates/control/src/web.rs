use crate::error::ApiError;
use axum::http::StatusCode;
use std::path::Path;
use tower_http::services::{ServeDir, ServeFile};

const UNKNOWN_ROUTE_MESSAGE: &str = "This control plane does not offer that yet. Update it to the latest version of Mycelium, then try again.";

pub async fn unknown_api() -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, "unknown_route", UNKNOWN_ROUTE_MESSAGE)
}

pub fn single_page_app(web_dir: &Path) -> ServeDir<ServeFile> {
    ServeDir::new(web_dir).fallback(ServeFile::new(web_dir.join("index.html")))
}
