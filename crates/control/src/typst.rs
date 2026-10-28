use crate::artifacts;
use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use std::path::Path as FilePath;
use std::time::Duration;
use tokio::process::Command;
use uuid::Uuid;

const SOURCE_FILE: &str = "main.typ";
const MOST_COMPILE_TIME: Duration = Duration::from_secs(30);
const MOST_SOURCE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Diagnostic {
    pub line: u32,
    pub column: u32,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub enum Format {
    Svg,
    Pdf,
}

#[derive(Debug)]
pub enum Outcome {
    Compiled(Vec<Vec<u8>>),
    Failed(Vec<Diagnostic>),
}

#[derive(Deserialize)]
pub struct CompileRequest {
    source: String,
}

#[derive(Serialize)]
pub struct Preview {
    compiled: bool,
    pages: Vec<String>,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Serialize)]
pub struct Latest {
    latest_version: i32,
    latest_compiled: bool,
    compiled_version: Option<i32>,
    compiled_at: Option<chrono::DateTime<chrono::Utc>>,
    pages: Vec<String>,
    diagnostics: Vec<Diagnostic>,
}

fn diagnostics_of(output: &str) -> Vec<Diagnostic> {
    output
        .lines()
        .filter_map(|line| {
            let (location, message) = line.split_once(": error: ")?;
            let mut parts = location.rsplitn(3, ':');
            let column = parts.next()?.parse().ok()?;
            let line_number = parts.next()?.parse().ok()?;

            Some(Diagnostic { line: line_number, column, message: message.trim().to_string() })
        })
        .collect()
}

pub async fn compile(binary: &str, scratch: &FilePath, source: &str, format: Format) -> ApiResult<Outcome> {
    if source.len() > MOST_SOURCE_BYTES {
        return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "source_too_large", "The document is larger than 1 MB. Split it up."));
    }

    let directory = scratch.join(Uuid::new_v4().to_string());

    tokio::fs::create_dir_all(&directory).await.map_err(ApiError::internal)?;

    let outcome = compile_in(binary, &directory, source, format).await;

    let _ = tokio::fs::remove_dir_all(&directory).await;

    outcome
}

async fn compile_in(binary: &str, directory: &FilePath, source: &str, format: Format) -> ApiResult<Outcome> {
    tokio::fs::write(directory.join(SOURCE_FILE), source).await.map_err(ApiError::internal)?;

    let output_name = match format {
        Format::Svg => "out-{p}.svg",
        Format::Pdf => "out.pdf",
    };
    let mut command = Command::new(binary);

    command.current_dir(directory).args(["compile", "--diagnostic-format", "short", "--root", ".", SOURCE_FILE, output_name]).kill_on_drop(true);

    let spawned = tokio::time::timeout(MOST_COMPILE_TIME, command.output()).await;
    let Ok(spawned) = spawned else {
        return Err(ApiError::new(StatusCode::GATEWAY_TIMEOUT, "typst_timeout", "The document took longer than 30 seconds to compile. Simplify it."));
    };
    let output = spawned.map_err(|_| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "typst_unavailable", "Typst is not installed on the control plane. Ask the operator to install it or set MYCELIUM_TYPST."))?;

    if !output.status.success() {
        let text = String::from_utf8_lossy(&output.stderr);
        let mut diagnostics = diagnostics_of(&text);

        if diagnostics.is_empty() {
            diagnostics.push(Diagnostic { line: 1, column: 1, message: text.trim().chars().take(500).collect() });
        }

        return Ok(Outcome::Failed(diagnostics));
    }

    let mut pages = Vec::new();

    match format {
        Format::Pdf => pages.push(tokio::fs::read(directory.join("out.pdf")).await.map_err(ApiError::internal)?),
        Format::Svg => {
            let mut number = 1;

            while let Ok(page) = tokio::fs::read(directory.join(format!("out-{number}.svg"))).await {
                pages.push(page);
                number += 1;
            }
        }
    }

    Ok(Outcome::Compiled(pages))
}

fn scratch_of(state: &AppState) -> std::path::PathBuf {
    state.config.data_dir.join("typst")
}

fn preview_of(outcome: Outcome) -> Preview {
    match outcome {
        Outcome::Compiled(pages) => Preview { compiled: true, pages: pages.iter().map(|page| String::from_utf8_lossy(page).into_owned()).collect(), diagnostics: Vec::new() },
        Outcome::Failed(diagnostics) => Preview { compiled: false, pages: Vec::new(), diagnostics },
    }
}

pub async fn preview(State(state): State<AppState>, _user: AuthUser, Json(request): Json<CompileRequest>) -> ApiResult<Json<Preview>> {
    let outcome = compile(&state.config.typst_binary, &scratch_of(&state), &request.source, Format::Svg).await?;

    Ok(Json(preview_of(outcome)))
}

async fn last_compiled(state: &AppState, id: Uuid, format: Format, newest: i32) -> ApiResult<Option<(i32, chrono::DateTime<chrono::Utc>, Outcome, Vec<Diagnostic>)>> {
    let mut diagnostics = Vec::new();

    for version in (1..=newest).rev() {
        let Some((source, created_at)) = artifacts::text_of_version(state, id, version).await? else {
            continue;
        };
        let outcome = compile(&state.config.typst_binary, &scratch_of(state), &source, format).await?;

        match outcome {
            Outcome::Compiled(_) => return Ok(Some((version, created_at, outcome, diagnostics))),
            Outcome::Failed(found) if version == newest => diagnostics = found,
            Outcome::Failed(_) => {}
        }
    }

    Ok(None)
}

pub async fn latest(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Latest>> {
    let row = artifacts::find_typst(&state, id, user.id).await?;
    let newest = row.latest_version;

    match last_compiled(&state, id, Format::Svg, newest).await? {
        Some((version, created_at, outcome, diagnostics)) => {
            let shown = preview_of(outcome);

            Ok(Json(Latest { latest_version: newest, latest_compiled: version == newest, compiled_version: Some(version), compiled_at: Some(created_at), pages: shown.pages, diagnostics }))
        }
        None => Ok(Json(Latest { latest_version: newest, latest_compiled: false, compiled_version: None, compiled_at: None, pages: Vec::new(), diagnostics: Vec::new() })),
    }
}

pub async fn pdf(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Response> {
    let row = artifacts::find_typst(&state, id, user.id).await?;
    let Some((_, _, Outcome::Compiled(mut pages), _)) = last_compiled(&state, id, Format::Pdf, row.latest_version).await? else {
        return Err(ApiError::bad_request("typst_never_compiled", "No version of this document compiles yet. Fix the source, then download the PDF."));
    };
    let bytes = pages.pop().unwrap_or_default();
    let name = format!("{}.pdf", artifacts::file_stem(&row.title));

    Ok(([(header::CONTENT_TYPE, "application/pdf".to_string()), (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\"")), (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string())], bytes).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn fake_typst(directory: &FilePath) -> String {
        let path = directory.join("typst");

        std::fs::write(
            &path,
            "#!/bin/sh\nif grep -q BROKEN main.typ; then echo 'main.typ:16:24: error: unclosed delimiter' >&2; exit 1; fi\nprintf '<svg>1</svg>' > out-1.svg\nprintf '<svg>2</svg>' > out-2.svg\nprintf '%%PDF' > out.pdf\n",
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();

        path.to_string_lossy().into_owned()
    }

    #[tokio::test]
    async fn a_compiling_document_returns_every_page() {
        let directory = tempfile::tempdir().unwrap();
        let binary = fake_typst(directory.path());
        let outcome = compile(&binary, directory.path(), "= Title", Format::Svg).await.unwrap();

        match outcome {
            Outcome::Compiled(pages) => assert_eq!(pages, vec![b"<svg>1</svg>".to_vec(), b"<svg>2</svg>".to_vec()]),
            Outcome::Failed(diagnostics) => panic!("{diagnostics:?}"),
        }
    }

    #[tokio::test]
    async fn a_document_that_does_not_compile_returns_the_line_and_column() {
        let directory = tempfile::tempdir().unwrap();
        let binary = fake_typst(directory.path());
        let outcome = compile(&binary, directory.path(), "BROKEN [24 min,", Format::Svg).await.unwrap();

        match outcome {
            Outcome::Failed(diagnostics) => assert_eq!(diagnostics, vec![Diagnostic { line: 16, column: 24, message: "unclosed delimiter".into() }]),
            Outcome::Compiled(_) => panic!("compiled"),
        }
    }

    #[tokio::test]
    async fn a_missing_compiler_says_how_to_install_it() {
        let directory = tempfile::tempdir().unwrap();
        let error = compile("/nonexistent/typst", directory.path(), "= Title", Format::Pdf).await.unwrap_err();

        assert!(error.message().contains("MYCELIUM_TYPST"));
    }
}
