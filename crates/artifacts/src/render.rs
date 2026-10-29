use axum::http::header::{CONTENT_SECURITY_POLICY, CONTENT_TYPE, REFERRER_POLICY, X_CONTENT_TYPE_OPTIONS};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use pulldown_cmark::{CowStr, Event, Options, Parser, Tag};

pub const PAGE_POLICY: &str = "sandbox allow-scripts allow-forms allow-popups; default-src 'none'; script-src 'self' 'unsafe-inline'; style-src 'unsafe-inline'; img-src 'self' data: blob:; font-src https: data:; connect-src 'none'; form-action 'self'; base-uri 'none'; frame-ancestors 'self'";
pub const SHELL_POLICY: &str = "default-src 'none'; script-src 'self'; style-src 'unsafe-inline'; frame-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";
pub const DOCUMENT_POLICY: &str = "default-src 'none'; style-src 'unsafe-inline'; img-src https: data:; base-uri 'none'; frame-ancestors 'none'";
pub const EDITOR_POLICY: &str = "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' https: data: blob:; font-src 'self' https: data:; connect-src 'self'; frame-src 'self' about:; base-uri 'none'; frame-ancestors 'none'";
pub const PDF_VIEWER_POLICY: &str = "default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; worker-src 'self' blob:; style-src 'self'; img-src 'self' blob: data:; media-src blob:; font-src 'self' data:; connect-src 'self' blob: data:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";
pub const FILE_POLICY: &str = "sandbox";

const STYLE: &str = "body{font:1.125rem/1.6 system-ui,sans-serif;max-width:46rem;margin:0 auto;padding:2rem 1rem;color:#1a1a1a;background:#fff}\
h1,h2,h3{line-height:1.25}pre,code{font-family:ui-monospace,monospace;background:#f3f3f3}pre{padding:1rem;overflow:auto}code{padding:.1em .3em}pre code{padding:0}\
table{border-collapse:collapse}td,th{border:1px solid #767676;padding:.4rem .7rem}blockquote{margin-left:0;padding-left:1rem;border-left:4px solid #767676}img{max-width:100%}a{color:#0b5cad}\
@media (prefers-color-scheme:dark){body{color:#eee;background:#161616}pre,code{background:#262626}a{color:#8ab8f0}}";

pub fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

pub fn hardened(mut response: Response, policy: &'static str) -> Response {
    let headers = response.headers_mut();
    headers.insert(CONTENT_SECURITY_POLICY, HeaderValue::from_static(policy));
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(REFERRER_POLICY, HeaderValue::from_static("no-referrer"));

    response
}

pub fn html(status: StatusCode, body: String, policy: &'static str) -> Response {
    let mut response = (status, body).into_response();
    response.headers_mut().insert(CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8"));

    hardened(response, policy)
}

pub fn notice(status: StatusCode, heading: &str, message: &str) -> Response {
    let body = format!(
        "<!doctype html><html lang=\"en-GB\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{heading}</title><style>{STYLE}</style></head><body><main><h1>{heading}</h1><p>{message}</p></main></body></html>",
        heading = escape(heading),
        message = escape(message),
    );

    html(status, body, DOCUMENT_POLICY)
}

fn safe_url(destination: CowStr<'_>) -> CowStr<'_> {
    let compact: String = destination.chars().filter(|character| !character.is_whitespace() && !character.is_control()).collect::<String>().to_lowercase();
    let blocked = ["javascript:", "vbscript:", "data:"].iter().any(|scheme| compact.starts_with(scheme));

    if blocked { CowStr::Borrowed("#") } else { destination }
}

pub fn markdown(title: &str, source: &str) -> Response {
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS | Options::ENABLE_FOOTNOTES;
    let events = Parser::new_ext(source, options).map(|event| match event {
        Event::Html(raw) | Event::InlineHtml(raw) => Event::Text(raw),
        Event::Start(Tag::Link { link_type, dest_url, title, id }) => Event::Start(Tag::Link { link_type, dest_url: safe_url(dest_url), title, id }),
        Event::Start(Tag::Image { link_type, dest_url, title, id }) => Event::Start(Tag::Image { link_type, dest_url: safe_url(dest_url), title, id }),
        other => other,
    });
    let mut rendered = String::new();
    pulldown_cmark::html::push_html(&mut rendered, events);

    let body = format!(
        "<!doctype html><html lang=\"en-GB\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{title}</title><style>{STYLE}</style></head><body><main>{rendered}</main></body></html>",
        title = escape(title),
    );

    html(StatusCode::OK, body, DOCUMENT_POLICY)
}
