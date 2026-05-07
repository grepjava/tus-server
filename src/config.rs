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
        })
    }
}
