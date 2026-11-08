use serde_json::Value;
use std::future::Future;

pub const MOST_PAGES: usize = 200;

pub struct Page {
    pub rows: Vec<Value>,
    pub next: Option<String>,
}

pub async fn all_rows<E, Fetch, Pending>(first: Page, mut fetch: Fetch, limit: impl FnOnce(String) -> E) -> Result<(Vec<Value>, usize), E>
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

        let page = fetch(address).await?;

        rows.extend(page.rows);
        next = page.next;
        pages += 1;
    }

    Ok((rows, pages))
}
