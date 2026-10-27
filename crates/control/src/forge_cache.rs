use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};
use uuid::Uuid;

pub const FORGE_CACHE_TTL: Duration = Duration::from_secs(180);
const STALE_LIMIT: Duration = Duration::from_secs(3600);
const MOST_ENTRIES: usize = 512;

pub enum Cached {
    Fresh(Value),
    Stale(Value),
    Miss,
}

struct Entry {
    forge: Uuid,
    value: Value,
    fetched: Instant,
    used: u64,
}

#[derive(Default)]
struct Inner {
    entries: HashMap<String, Entry>,
    epochs: HashMap<Uuid, u64>,
    refreshing: HashSet<String>,
    ticks: u64,
}

pub struct ForgeCache {
    ttl: Duration,
    inner: Mutex<Inner>,
}

impl ForgeCache {
    pub fn new(ttl: Duration) -> ForgeCache {
        ForgeCache { ttl, inner: Mutex::new(Inner::default()) }
    }

    fn guard(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn key(forge: Uuid, url: &str) -> String {
        format!("{forge} GET {url}")
    }

    pub fn lookup(&self, key: &str) -> Cached {
        let mut inner = self.guard();

        inner.ticks += 1;
        let ticks = inner.ticks;
        let Some(entry) = inner.entries.get_mut(key) else { return Cached::Miss };
        let age = entry.fetched.elapsed();

        if age > STALE_LIMIT.max(self.ttl) {
            inner.entries.remove(key);

            return Cached::Miss;
        }

        entry.used = ticks;

        if age < self.ttl { Cached::Fresh(entry.value.clone()) } else { Cached::Stale(entry.value.clone()) }
    }

    pub fn epoch(&self, forge: Uuid) -> u64 {
        self.guard().epochs.get(&forge).copied().unwrap_or(0)
    }

    pub fn store(&self, forge: Uuid, key: String, value: Value, epoch: u64) {
        let mut inner = self.guard();

        if inner.epochs.get(&forge).copied().unwrap_or(0) != epoch {
            return;
        }

        inner.ticks += 1;
        let used = inner.ticks;
        inner.entries.insert(key, Entry { forge, value, fetched: Instant::now(), used });

        while inner.entries.len() > MOST_ENTRIES {
            let Some(oldest) = inner.entries.iter().min_by_key(|(_, entry)| entry.used).map(|(key, _)| key.clone()) else { break };

            inner.entries.remove(&oldest);
        }
    }

    pub fn invalidate(&self, forge: Uuid) {
        let mut inner = self.guard();

        *inner.epochs.entry(forge).or_insert(0) += 1;
        inner.entries.retain(|_, entry| entry.forge != forge);
    }

    pub fn claim_refresh(&self, key: &str) -> bool {
        self.guard().refreshing.insert(key.to_string())
    }

    pub fn finish_refresh(&self, key: &str) {
        self.guard().refreshing.remove(key);
    }
}
