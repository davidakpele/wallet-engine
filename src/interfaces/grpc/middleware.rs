// src/interfaces/grpc/middleware.rs
use std::{
    num::NonZeroU32,
    sync::Arc,
};

use governor::{
    clock::DefaultClock,
    middleware::NoOpMiddleware,
    state::{InMemoryState, NotKeyed},
    Quota, RateLimiter,
};
use metrics::{counter, histogram};
use tonic::{Request, Status};

pub type SharedRateLimiter =
    Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock, NoOpMiddleware>>;

/// Build a global rate limiter (requests-per-second across all callers).
/// For per-wallet limiting, key by wallet_id in a DashMap of limiters.
pub fn build_rate_limiter(rps: u32) -> SharedRateLimiter {
    Arc::new(RateLimiter::direct(Quota::per_second(
        NonZeroU32::new(rps).expect("rps must be > 0"),
    )))
}

/// gRPC interceptor: enforces global rate limit and records request metrics.
pub fn rate_limit_interceptor(
    limiter: SharedRateLimiter,
) -> impl Fn(Request<()>) -> Result<Request<()>, Status> + Clone {
    move |req: Request<()>| {
        if limiter.check().is_err() {
            counter!("grpc_rate_limited_total").increment(1);
            return Err(Status::resource_exhausted("Rate limit exceeded"));
        }
        Ok(req)
    }
}

/// Record gRPC metrics after each request.
pub fn record_grpc_metrics(method: &str, status_code: &str, duration: std::time::Duration) {
    counter!("grpc_requests_total", "method" => method.to_string(), "status" => status_code.to_string())
        .increment(1);
    histogram!("grpc_request_duration_seconds", "method" => method.to_string())
        .record(duration.as_secs_f64());
}