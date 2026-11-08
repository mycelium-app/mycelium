use serde_json::Value;
use std::collections::HashSet;
use std::future::Future;

pub const MOST_PAGES: usize = 200;

pub struct Page {
    pub rows: Vec<Value>,
    pub next: Option<String>,
}

pub async fn all_rows<E, Fetch, Pending>(first: Page, asked: &str, mut fetch: Fetch, limit: impl FnOnce(String) -> E) -> Result<(Vec<Value>, usize), E>
where
    Fetch: FnMut(String) -> Pending,
    Pending: Future<Output = Result<Page, E>>,
{
    let mut rows = first.rows;
    let mut next = first.next;
    let mut pages = 1;

    while let Some(address) = next.take() {
        if pages == MOST_PAGES {
            return Err(limit(format!("Stopped after {MOST_PAGES} pages ({} rows). Narrow the dates and ask again.", rows.len())));
        }

        let (path, own) = address.split_once('?').unwrap_or((&address, ""));
        let query = {
            let present: HashSet<String> = url::form_urlencoded::parse(own.as_bytes()).map(|(key, _)| key.into_owned()).collect();
            let mut merged = url::form_urlencoded::Serializer::new(own.to_string());

            for (key, value) in url::form_urlencoded::parse(asked.as_bytes()).filter(|(key, _)| !present.contains(key.as_ref())) {
                merged.append_pair(&key, &value);
            }

            merged.finish()
        };
        let page = fetch(if query.is_empty() { path.to_string() } else { format!("{path}?{query}") }).await?;

        rows.extend(page.rows);
        next = page.next;
        pages += 1;
    }

    Ok((rows, pages))
}
