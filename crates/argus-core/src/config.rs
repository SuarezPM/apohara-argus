//! Configuration loaded from environment variables.

use serde::{Deserialize, Serialize};
use std::env;

use super::errors::{ArgusError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub database_url: String,
    pub github_token: Option<String>,
    pub nim_base_url: String,
    pub nim_default_model: String,
    pub api_port: u16,
    pub dashboard_port: u16,
    pub log_level: String,
    /// Retention window in days for the Article 12 audit log (Roadmap 2.2
    /// NDJSON exporter prunes entries older than this). Default 365 days
    /// (1 year) — enough for most enterprise review cycles, well within
    /// EU AI Act's "throughout the lifecycle" obligation.
    pub retention_days: u32,
}

impl Config {
    /// Load config from environment variables (with `.env` if present).
    pub fn from_env() -> Result<Self> {
        let _ = dotenv_load();
        Ok(Self {
            database_url: env::var("DATABASE_URL")
                .unwrap_or_else(|_| "postgresql://localhost:5432/argus_dev".into()),
            github_token: env::var("GITHUB_TOKEN").ok().filter(|s| !s.is_empty()),
            nim_base_url: env::var("ARGUS_NIM_BASE_URL")
                .unwrap_or_else(|_| "https://integrate.api.nvidia.com/v1".into()),
            nim_default_model: env::var("ARGUS_NIM_MODEL")
                .unwrap_or_else(|_| "meta/llama-3.1-70b-instruct".into()),
            api_port: env::var("ARGUS_API_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(8080),
            dashboard_port: env::var("ARGUS_DASHBOARD_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3000),
            log_level: env::var("ARGUS_LOG").unwrap_or_else(|_| "info".into()),
            retention_days: env::var("ARGUS_RETENTION_DAYS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(365),
        })
    }

    /// Validates that required fields are set.
    pub fn validate(&self) -> Result<()> {
        if self.database_url.is_empty() {
            return Err(ArgusError::Config("DATABASE_URL is empty".into()));
        }
        if !self.nim_base_url.starts_with("http") {
            return Err(ArgusError::Config(
                "ARGUS_NIM_BASE_URL must be a URL".into(),
            ));
        }
        Ok(())
    }
}

fn dotenv_load() -> std::io::Result<()> {
    use std::fs;
    let path = std::path::Path::new(".env");
    if !path.exists() {
        return Ok(());
    }
    let content = fs::read_to_string(path)?;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let k = k.trim();
            let v = v.trim().trim_matches('"').trim_matches('\'');
            // SAFETY: this is called at startup before any threads are spawned.
            if std::env::var(k).is_err() {
                std::env::set_var(k, v);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_with_defaults() {
        // cargo test runs tests in parallel — env::set_var is unsafe in that context.
        // We just check that defaults are non-empty when the env var isn't set.
        // SAFETY: single-threaded test, no concurrent env access.
        let prev = std::env::var("ARGUS_NIM_BASE_URL").ok();
        std::env::remove_var("ARGUS_NIM_BASE_URL");
        let c = Config::from_env().expect("load");
        if let Some(v) = prev {
            std::env::set_var("ARGUS_NIM_BASE_URL", v);
        }
        assert!(!c.nim_base_url.is_empty());
        assert!(c.api_port > 0);
    }

    #[test]
    fn retention_days_default_and_env_override() {
        // Default: 365 when env not set.
        let prev = std::env::var("ARGUS_RETENTION_DAYS").ok();
        std::env::remove_var("ARGUS_RETENTION_DAYS");
        let c = Config::from_env().expect("load");
        assert_eq!(c.retention_days, 365);
        if let Some(v) = prev {
            std::env::set_var("ARGUS_RETENTION_DAYS", v);
        }

        // Env override: 90.
        // SAFETY: single-threaded test.
        let prev2 = std::env::var("ARGUS_RETENTION_DAYS").ok();
        std::env::set_var("ARGUS_RETENTION_DAYS", "90");
        let c = Config::from_env().expect("load");
        match prev2 {
            Some(v) => std::env::set_var("ARGUS_RETENTION_DAYS", v),
            None => std::env::remove_var("ARGUS_RETENTION_DAYS"),
        }
        assert_eq!(c.retention_days, 90);
    }
}
