use std::{
    net::{IpAddr, SocketAddr},
    num::NonZeroU32,
    sync::Arc,
};

use axum::{
    extract::{ConnectInfo, Request, State},
    http::{HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use governor::{DefaultKeyedRateLimiter, Quota, RateLimiter};

use crate::app_state::AppState;

pub type IpRateLimiter = DefaultKeyedRateLimiter<IpAddr>;

pub fn build_limiter(per_second: NonZeroU32, burst: NonZeroU32) -> IpRateLimiter {
    RateLimiter::keyed(Quota::per_second(per_second).allow_burst(burst))
}

pub async fn rate_limit_middleware(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    if let Some(limiter) = &state.rate_limiter {
        let ip = crate::trusted_proxy::extract_client_ip(
            req.headers(),
            addr.ip(),
            &state.config.trusted_proxies,
        );
        if limiter.check_key(&ip).is_err() {
            let mut resp = (StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded").into_response();
            resp.headers_mut()
                .insert(axum::http::header::RETRY_AFTER, HeaderValue::from_static("1"));
            return resp;
        }
    }
    next.run(req).await
}


pub fn from_config(rps: u32, burst: u32) -> Option<Arc<IpRateLimiter>> {
    let per_second = NonZeroU32::new(rps)?;
    let burst_size = NonZeroU32::new(burst).unwrap_or(per_second);
    Some(Arc::new(build_limiter(per_second, burst_size)))
}
