use serde_json::{Value, json};
use std::cmp::Ordering;
use std::future::Future;

pub const MOST_MESSAGES: u64 = 500;
pub const PAGE_SIZE: u64 = 100;
pub const MOST_PAGES: usize = 10;

#[derive(Clone, Copy, PartialEq)]
pub enum Direction {
    Backward,
    Forward,
}

pub struct Gathered {
    pub messages: Vec<Value>,
    pub exhausted: bool,
    pub cursor: Option<String>,
}

pub struct Collected {
    pub messages: Vec<Value>,
    pub coverage: Value,
}

fn compare(left: &str, right: &str) -> Ordering {
    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
}

pub fn total_limit(asked: Option<u64>, default: u64) -> u64 {
    asked.unwrap_or(default).clamp(1, MOST_MESSAGES)
}

pub fn coverage(returned: usize, oldest_id: Option<&str>, newest_id: Option<&str>, found_oldest: bool, found_newest: bool, next: Option<String>) -> Value {
    json!({"returned": returned, "found_oldest": found_oldest, "found_newest": found_newest, "oldest_id": oldest_id, "newest_id": newest_id, "next": next})
}

pub async fn gather<Fetch, Pending, Advance>(total: u64, page_size: u64, start: Option<String>, mut fetch: Fetch, advance: Advance) -> Result<Gathered, String>
where
    Fetch: FnMut(u64, Option<String>) -> Pending,
    Pending: Future<Output = Result<Vec<Value>, String>>,
    Advance: Fn(&[Value], usize) -> Option<String>,
{
    let mut messages: Vec<Value> = vec![];
    let mut cursor = start;
    let mut exhausted = false;

    for _ in 0..MOST_PAGES {
        let size = (total - messages.len() as u64).min(page_size);
        let page = fetch(size, cursor.clone()).await?;
        let short = (page.len() as u64) < size;

        messages.extend(page);

        if short {
            exhausted = true;

            break;
        }

        cursor = advance(&messages, messages.len());

        if messages.len() as u64 >= total {
            break;
        }
    }

    Ok(Gathered { messages, exhausted, cursor })
}

pub async fn collect<Fetch, Pending>(total: u64, direction: Direction, start: Option<String>, id_of: fn(&Value) -> Option<&str>, fetch: Fetch) -> Result<Collected, String>
where
    Fetch: FnMut(u64, Option<String>) -> Pending,
    Pending: Future<Output = Result<Vec<Value>, String>>,
{
    let started_at_newest = direction == Direction::Backward && start.is_none();
    let gathered = gather(total, PAGE_SIZE, start, fetch, |messages, _| edge(messages, direction, id_of)).await?;
    let mut messages = gathered.messages;

    messages.sort_by(|left, right| compare(id_of(left).unwrap_or_default(), id_of(right).unwrap_or_default()));
    messages.dedup_by(|left, right| id_of(left) == id_of(right));

    let oldest_id = messages.first().and_then(id_of).map(str::to_string);
    let newest_id = messages.last().and_then(id_of).map(str::to_string);
    let found_oldest = direction == Direction::Backward && gathered.exhausted;
    let found_newest = started_at_newest || (direction == Direction::Forward && gathered.exhausted);
    let next = match (gathered.exhausted, direction) {
        (true, _) => None,
        (false, Direction::Backward) => oldest_id.clone(),
        (false, Direction::Forward) => newest_id.clone(),
    };
    let coverage = coverage(messages.len(), oldest_id.as_deref(), newest_id.as_deref(), found_oldest, found_newest, next);

    Ok(Collected { messages, coverage })
}

fn edge(messages: &[Value], direction: Direction, id_of: fn(&Value) -> Option<&str>) -> Option<String> {
    let ids = messages.iter().filter_map(id_of);
    let chosen = match direction {
        Direction::Backward => ids.min_by(|left, right| compare(left, right)),
        Direction::Forward => ids.max_by(|left, right| compare(left, right)),
    };

    chosen.map(str::to_string)
}
