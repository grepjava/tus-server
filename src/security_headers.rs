use axum::{
    body::Body,
    extract::{Request, State},
    http::{Response, HeaderValue},
    middleware::Next,
};

use crate::app_state::AppState;

/// Builds a Content-Security-Policy that allows loading Grafana in an iframe
/// when GRAFANA_URL is configured, and blocks all framing otherwise.
fn build_csp(grafana_url: Option<&str>) -> String {
    let frame_src = match grafana_url {
        Some(url) => format!("frame-src 'self' {url}; "),
        None => String::new(),
    };
    // SvelteKit emits an inline bootstrap <script> block that cannot be hashed
    // at the middleware layer without re-parsing the HTML, so 'unsafe-inline'
    // is required for script-src. External script injection is still blocked by
    // 'self', which prevents the most common XSS escalation path.
    format!(
        "default-src 'self'; \
         script-src 'self' 'unsafe-inline'; \
         style-src 'self' 'unsafe-inline'; \
         img-src 'self' data: blob:; \
         connect-src 'self'; \
         font-src 'self'; \
         {frame_src}\
         frame-ancestors 'none'; \
         object-src 'none'; \
         base-uri 'self'"
    )
}

pub async fn security_headers_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response<Body> {
    let mut response = next.run(request).await;
    let h = response.headers_mut();

    h.insert("x-frame-options",        HeaderValue::from_static("DENY"));
    h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    h.insert("referrer-policy",        HeaderValue::from_static("strict-origin-when-cross-origin"));

    let csp = build_csp(state.config.grafana_url.as_deref());
    if let Ok(v) = HeaderValue::from_str(&csp) {
        h.insert("content-security-policy", v);
    }

    if state.config.cookie_secure {
        h.insert(
            "strict-transport-security",
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        );
    }

    response
}
