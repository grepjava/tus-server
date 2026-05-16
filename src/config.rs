use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub db_url: String,
    pub storage_dir: PathBuf,
    pub base_url: String,
    pub bind_addr: String,
    pub max_upload_bytes: i64,
    pub abandoned_after_hours: i64,
    pub cleanup_interval_secs: u64,
    pub upload_expiry_hours: i64,
    /// When set, all requests must carry `Authorization: Bearer <key>`.
    /// Unset (default) disables auth — suitable for local/dev use only.
    pub api_key: Option<String>,
    /// Maximum number of delivery attempts per webhook event (WEBHOOK_MAX_ATTEMPTS).
    pub webhook_max_attempts: u32,
    /// Seconds to wait before each retry attempt (WEBHOOK_RETRY_DELAYS, comma-separated).
    /// The last value is reused when attempts exceed the list length.
    pub webhook_retry_delays_secs: Vec<u64>,
    /// Webhook delivery records older than this many days are pruned by the cleanup worker.
    pub webhook_delivery_retention_days: i64,
    /// Requests per second allowed per client IP (0 = disabled).
    pub rate_limit_rps: u32,
    /// Burst allowance above the steady rate (0 = same as rate_limit_rps).
    pub rate_limit_burst: u32,
    /// Storage backend: "filesystem" (default) or "s3".
    pub storage_backend: String,
    /// S3 bucket name (required when storage_backend = "s3").
    pub s3_bucket: Option<String>,
    /// S3 key prefix (default: "uploads/").
    pub s3_prefix: String,
    /// Local directory for in-progress upload staging (default: {storage_dir}/staging).
    pub s3_staging_dir: Option<std::path::PathBuf>,
    /// Use path-style S3 URLs — required for MinIO and LocalStack (default: false).
    pub s3_force_path_style: bool,
    /// Files smaller than this (bytes) use PutObject; larger files use multipart upload (default: 8 MiB).
    pub s3_multipart_threshold: u64,
    /// Part size for multipart uploads in bytes (default: 8 MiB; minimum 5 MiB).
    pub s3_part_size: usize,
    /// Maximum total bytes across all active uploads (0 = no limit).
    pub quota_max_storage_bytes: i64,
    /// Maximum number of concurrent active uploads (0 = no limit).
    pub quota_max_active_uploads: i64,
    /// Audit log entries older than this many days are pruned (0 = keep forever, default: 90).
    pub audit_log_retention_days: i64,
    /// Optional URL to a Grafana dashboard for this server (shown in the Metrics page).
    pub grafana_url: Option<String>,
    /// Append `Secure` to the session cookie (set true when serving over HTTPS).
    pub cookie_secure: bool,
    /// CIDRs of trusted reverse proxies whose X-Forwarded-For header is honoured.
    /// Empty = trust all (backwards-compatible default).
    pub trusted_proxies: Vec<ipnet::IpNet>,
    /// Maximum failed login attempts from one IP before a lockout (default: 10).
    pub login_max_attempts: u32,
    /// Seconds an IP is locked out after exceeding login_max_attempts (default: 900).
    pub login_lockout_secs: u64,
    /// OIDC issuer URL (e.g. https://accounts.google.com). Unset = OIDC disabled.
    pub oidc_issuer_url: Option<String>,
    /// OIDC client ID.
    pub oidc_client_id: Option<String>,
    /// OIDC client secret (optional for public clients).
    pub oidc_client_secret: Option<String>,
    /// Redirect URI registered with the IdP (e.g. https://tuskar.example.com/api/auth/oidc/callback).
    pub oidc_redirect_uri: Option<String>,
    /// Role assigned to auto-provisioned OIDC users (default: viewer).
    pub oidc_default_role: String,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            db_url: std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "tus.db".to_string()),
            storage_dir: PathBuf::from(
                std::env::var("STORAGE_DIR").unwrap_or_else(|_| "uploads".to_string()),
            ),
            base_url: std::env::var("BASE_URL")
                .unwrap_or_else(|_| "http://localhost:3000".to_string()),
            bind_addr: std::env::var("BIND_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:3000".to_string()),
            max_upload_bytes: std::env::var("MAX_UPLOAD_BYTES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(107_374_182_400),
            abandoned_after_hours: std::env::var("ABANDONED_AFTER_HOURS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(24),
            cleanup_interval_secs: std::env::var("CLEANUP_INTERVAL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3600),
            upload_expiry_hours: std::env::var("UPLOAD_EXPIRY_HOURS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(24),
            api_key: std::env::var("API_KEY").ok().filter(|s| !s.is_empty()),
            webhook_max_attempts: std::env::var("WEBHOOK_MAX_ATTEMPTS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3),
            webhook_retry_delays_secs: std::env::var("WEBHOOK_RETRY_DELAYS")
                .ok()
                .map(|v| v.split(',').filter_map(|s| s.trim().parse().ok()).collect::<Vec<u64>>())
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| vec![1, 4]),
            webhook_delivery_retention_days: std::env::var("WEBHOOK_DELIVERY_RETENTION_DAYS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30),
            rate_limit_rps: std::env::var("RATE_LIMIT_RPS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            rate_limit_burst: std::env::var("RATE_LIMIT_BURST")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            storage_backend: std::env::var("STORAGE_BACKEND")
                .unwrap_or_else(|_| "filesystem".to_string()),
            s3_bucket: std::env::var("S3_BUCKET").ok().filter(|s| !s.is_empty()),
            s3_prefix: std::env::var("S3_PREFIX")
                .unwrap_or_else(|_| "uploads/".to_string()),
            s3_staging_dir: std::env::var("S3_STAGING_DIR")
                .ok()
                .filter(|s| !s.is_empty())
                .map(std::path::PathBuf::from),
            s3_force_path_style: std::env::var("S3_FORCE_PATH_STYLE")
                .map(|v| matches!(v.to_lowercase().as_str(), "true" | "1" | "yes"))
                .unwrap_or(false),
            s3_multipart_threshold: std::env::var("S3_MULTIPART_THRESHOLD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(8 * 1024 * 1024),
            s3_part_size: std::env::var("S3_PART_SIZE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(8 * 1024 * 1024),
            quota_max_storage_bytes: std::env::var("QUOTA_MAX_STORAGE_BYTES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            quota_max_active_uploads: std::env::var("QUOTA_MAX_ACTIVE_UPLOADS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            audit_log_retention_days: std::env::var("AUDIT_LOG_RETENTION_DAYS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(90),
            grafana_url: std::env::var("GRAFANA_URL").ok().filter(|s| !s.is_empty()),
            cookie_secure: std::env::var("COOKIE_SECURE")
                .map(|v| matches!(v.to_lowercase().as_str(), "true" | "1" | "yes"))
                .unwrap_or_else(|_| {
                    // Default to true when BASE_URL is https:// — avoids accidental
                    // insecure-cookie deployments without requiring explicit config.
                    std::env::var("BASE_URL")
                        .map(|u| u.starts_with("https://"))
                        .unwrap_or(false)
                }),
            trusted_proxies: std::env::var("TRUSTED_PROXIES")
                .ok()
                .map(|v| {
                    v.split(',')
                        .filter_map(|s| s.trim().parse::<ipnet::IpNet>().ok())
                        .collect()
                })
                .unwrap_or_default(),
            login_max_attempts: std::env::var("LOGIN_MAX_ATTEMPTS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10),
            login_lockout_secs: std::env::var("LOGIN_LOCKOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(900),
            oidc_issuer_url: std::env::var("OIDC_ISSUER_URL").ok().filter(|s| !s.is_empty()),
            oidc_client_id: std::env::var("OIDC_CLIENT_ID").ok().filter(|s| !s.is_empty()),
            oidc_client_secret: std::env::var("OIDC_CLIENT_SECRET").ok().filter(|s| !s.is_empty()),
            oidc_redirect_uri: std::env::var("OIDC_REDIRECT_URI").ok().filter(|s| !s.is_empty()),
            oidc_default_role: std::env::var("OIDC_DEFAULT_ROLE")
                .unwrap_or_else(|_| "viewer".to_string()),
        })
    }
}
