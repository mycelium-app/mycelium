use crate::error::ApiError;
use axum::http::{HeaderValue, StatusCode, header::RETRY_AFTER};
use axum::response::{IntoResponse, Response};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const PRUNE_ABOVE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Limited {
    StartDevice,
    PollDeviceToken,
}

impl Limited {
    fn capacity(self) -> f64 {
        match self {
            Limited::StartDevice => 10.0,
            Limited::PollDeviceToken => 30.0,
        }
    }

    fn refill_per_second(self) -> f64 {
        match self {
            Limited::StartDevice => 1.0 / 6.0,
            Limited::PollDeviceToken => 1.0,
        }
    }
}

struct Bucket {
    tokens: f64,
    updated: Instant,
}

#[derive(Default)]
pub struct RateLimiter {
    buckets: Mutex<HashMap<(Limited, IpAddr), Bucket>>,
}

pub struct TooManyRequests {
    wait: Duration,
}

impl RateLimiter {
    pub fn take(&self, what: Limited, address: IpAddr) -> Result<(), TooManyRequests> {
        let Ok(mut buckets) = self.buckets.lock() else {
            return Ok(());
        };
        let now = Instant::now();

        if buckets.len() > PRUNE_ABOVE {
            buckets.retain(|(what, _), bucket| bucket.tokens + now.duration_since(bucket.updated).as_secs_f64() * what.refill_per_second() < what.capacity());
        }

        let bucket = buckets.entry((what, address)).or_insert(Bucket { tokens: what.capacity(), updated: now });
        bucket.tokens = (bucket.tokens + now.duration_since(bucket.updated).as_secs_f64() * what.refill_per_second()).min(what.capacity());
        bucket.updated = now;

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;

            return Ok(());
        }

        Err(TooManyRequests { wait: Duration::from_secs_f64((1.0 - bucket.tokens) / what.refill_per_second()) })
    }
}

impl IntoResponse for TooManyRequests {
    fn into_response(self) -> Response {
        let mut response = ApiError::new(StatusCode::TOO_MANY_REQUESTS, "rate_limited", "You have made too many requests. Wait a minute, then try again.").into_response();

        if let Ok(seconds) = HeaderValue::from_str(&self.wait.as_secs().max(1).to_string()) {
            response.headers_mut().insert(RETRY_AFTER, seconds);
        }

        response
    }
}
