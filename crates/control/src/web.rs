use crate::error::ApiError;
use std::path::Path;
use tower_http::services::{ServeDir, ServeFile};

pub async fn unknown_api() -> ApiError {
    ApiError::not_found("That address")
}

pub fn single_page_app(web_dir: &Path) -> ServeDir<ServeFile> {
    ServeDir::new(web_dir).fallback(ServeFile::new(web_dir.join("index.html")))
}
