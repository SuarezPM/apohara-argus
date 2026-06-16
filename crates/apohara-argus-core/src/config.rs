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
        // Acquire the ENV_LOCK to serialize with the other
        // env-touching tests in this module (added later). Without
        // the lock, a concurrent test setting ARGUS_RETENTION_DAYS
        // to a non-365 value would race with this test's
        // `remove_var` + `from_env` sequence and produce a flaky
        // failure.
        let _guard = ENV_LOCK.lock().unwrap();
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

    /// Serializes the env-touching tests in this module. The
    /// existing 2 tests above touch `ARGUS_NIM_BASE_URL` and
    /// `ARGUS_RETENTION_DAYS`; the new tests below touch many
    /// more env vars. Without a lock, two tests setting the
    /// same var in parallel would race. We use `std::sync::Mutex`
    /// (not `tokio::sync::Mutex`) because the critical section
    /// is pure env-var I/O with no async work.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn validate_succeeds_with_valid_config() {
        let c = Config {
            database_url: "postgresql://localhost/db".into(),
            github_token: None,
            nim_base_url: "https://example.com/v1".into(),
            nim_default_model: "m".into(),
            api_port: 8080,
            dashboard_port: 3000,
            log_level: "info".into(),
            retention_days: 365,
        };
        assert!(c.validate().is_ok());
    }

    #[test]
    fn validate_errors_on_empty_database_url() {
        let c = Config {
            database_url: String::new(),
            github_token: None,
            nim_base_url: "https://example.com/v1".into(),
            nim_default_model: "m".into(),
            api_port: 8080,
            dashboard_port: 3000,
            log_level: "info".into(),
            retention_days: 365,
        };
        let res = c.validate();
        assert!(matches!(res, Err(ArgusError::Config(ref m)) if m.contains("DATABASE_URL")));
    }

    #[test]
    fn validate_errors_on_invalid_nim_base_url() {
        let c = Config {
            database_url: "postgresql://localhost/db".into(),
            github_token: None,
            nim_base_url: "not-a-url".into(),
            nim_default_model: "m".into(),
            api_port: 8080,
            dashboard_port: 3000,
            log_level: "info".into(),
            retention_days: 365,
        };
        let res = c.validate();
        assert!(matches!(res, Err(ArgusError::Config(ref m)) if m.contains("ARGUS_NIM_BASE_URL")));
    }

    #[test]
    fn from_env_with_all_vars_set() {
        let _guard = ENV_LOCK.lock().unwrap();
        // Save and clear all relevant env vars, then set them to
        // known values, then verify Config::from_env picks them up.
        let keys = [
            "DATABASE_URL",
            "GITHUB_TOKEN",
            "ARGUS_NIM_BASE_URL",
            "ARGUS_NIM_MODEL",
            "ARGUS_API_PORT",
            "ARGUS_DASHBOARD_PORT",
            "ARGUS_LOG",
            "ARGUS_RETENTION_DAYS",
        ];
        let prev: Vec<_> = keys.iter().map(|k| (k, std::env::var(k).ok())).collect();
        for k in &keys {
            std::env::remove_var(k);
        }
        std::env::set_var("DATABASE_URL", "postgresql://test/db");
        std::env::set_var("GITHUB_TOKEN", "ghp_test");
        std::env::set_var("ARGUS_NIM_BASE_URL", "https://test.example/v1");
        std::env::set_var("ARGUS_NIM_MODEL", "test/model");
        std::env::set_var("ARGUS_API_PORT", "9999");
        std::env::set_var("ARGUS_DASHBOARD_PORT", "4000");
        std::env::set_var("ARGUS_LOG", "debug");
        std::env::set_var("ARGUS_RETENTION_DAYS", "30");
        let c = Config::from_env().expect("load");
        assert_eq!(c.database_url, "postgresql://test/db");
        assert_eq!(c.github_token.as_deref(), Some("ghp_test"));
        assert_eq!(c.nim_base_url, "https://test.example/v1");
        assert_eq!(c.nim_default_model, "test/model");
        assert_eq!(c.api_port, 9999);
        assert_eq!(c.dashboard_port, 4000);
        assert_eq!(c.log_level, "debug");
        assert_eq!(c.retention_days, 30);
        // Restore.
        for (k, v) in &prev {
            match v {
                Some(val) => std::env::set_var(k, val),
                None => std::env::remove_var(k),
            }
        }
    }

    #[test]
    fn from_env_with_invalid_port_falls_back_to_default() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev = std::env::var("ARGUS_API_PORT").ok();
        // A non-numeric port must fall back to the default (8080),
        // not crash or return an error. The .ok().and_then(parse)
        // chain silently swallows parse failures.
        std::env::set_var("ARGUS_API_PORT", "not-a-number");
        let c = Config::from_env().expect("load");
        assert_eq!(c.api_port, 8080);
        match prev {
            Some(v) => std::env::set_var("ARGUS_API_PORT", v),
            None => std::env::remove_var("ARGUS_API_PORT"),
        }
    }

    #[test]
    fn from_env_with_invalid_retention_falls_back_to_default() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev = std::env::var("ARGUS_RETENTION_DAYS").ok();
        std::env::set_var("ARGUS_RETENTION_DAYS", "not-a-number");
        let c = Config::from_env().expect("load");
        assert_eq!(c.retention_days, 365);
        match prev {
            Some(v) => std::env::set_var("ARGUS_RETENTION_DAYS", v),
            None => std::env::remove_var("ARGUS_RETENTION_DAYS"),
        }
    }

    #[test]
    fn from_env_with_empty_github_token_is_none() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev = std::env::var("GITHUB_TOKEN").ok();
        // The filter(|s| !s.is_empty()) means an empty token
        // string is treated as "not set" (None), not as Some("").
        std::env::set_var("GITHUB_TOKEN", "");
        let c = Config::from_env().expect("load");
        assert!(c.github_token.is_none());
        match prev {
            Some(v) => std::env::set_var("GITHUB_TOKEN", v),
            None => std::env::remove_var("GITHUB_TOKEN"),
        }
    }

    #[test]
    fn from_env_database_url_default() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev = std::env::var("DATABASE_URL").ok();
        std::env::remove_var("DATABASE_URL");
        let c = Config::from_env().expect("load");
        // Default points at localhost Postgres for dev.
        assert_eq!(c.database_url, "postgresql://localhost:5432/argus_dev");
        if let Some(v) = prev {
            std::env::set_var("DATABASE_URL", v);
        }
    }

    #[test]
    fn dotenv_load_returns_ok_when_no_env_file() {
        // If .env doesn't exist in the current dir, dotenv_load
        // must return Ok(()) silently — the function is best-effort
        // and must not block production startup.
        // SAFETY: the test runs single-threaded; the CWD is the
        // workspace root under cargo test, and we don't create a
        // .env there. If one happens to exist, the test still
        // returns Ok (the function never errors on missing files).
        let res = dotenv_load();
        assert!(res.is_ok());
    }

    #[test]
    fn dotenv_load_parses_key_value_lines() {
        // Create a temp .env file, call dotenv_load() from that
        // directory, and verify the env vars were set. We use
        // set_current_dir to avoid polluting the workspace root.
        // SAFETY: this test holds ENV_LOCK and runs in isolation;
        // we restore the CWD before releasing the lock.
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join("argus-config-dotenv-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(".env"),
            "# This is a comment\n\
             ARGUS_TEST_DOTENV_KEY1=value1\n\
             ARGUS_TEST_DOTENV_KEY2=\"quoted value\"\n\
             ARGUS_TEST_DOTENV_KEY3='single quoted'\n\
             \n\
             # Another comment\n\
             ARGUS_TEST_DOTENV_KEY4=trailing=with=equals\n",
        )
        .unwrap();
        let prev_keys = [
            "ARGUS_TEST_DOTENV_KEY1",
            "ARGUS_TEST_DOTENV_KEY2",
            "ARGUS_TEST_DOTENV_KEY3",
            "ARGUS_TEST_DOTENV_KEY4",
        ];
        let prev: Vec<_> = prev_keys
            .iter()
            .map(|k| (k, std::env::var(k).ok()))
            .collect();
        for k in &prev_keys {
            std::env::remove_var(k);
        }
        let prev_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&dir).unwrap();
        let res = dotenv_load();
        std::env::set_current_dir(&prev_cwd).unwrap();
        assert!(res.is_ok());
        assert_eq!(
            std::env::var("ARGUS_TEST_DOTENV_KEY1").as_deref(),
            Ok("value1")
        );
        assert_eq!(
            std::env::var("ARGUS_TEST_DOTENV_KEY2").as_deref(),
            Ok("quoted value")
        );
        assert_eq!(
            std::env::var("ARGUS_TEST_DOTENV_KEY3").as_deref(),
            Ok("single quoted")
        );
        // The parser splits on the FIRST '='; the rest is part of
        // the value (including additional '=' chars).
        assert_eq!(
            std::env::var("ARGUS_TEST_DOTENV_KEY4").as_deref(),
            Ok("trailing=with=equals")
        );
        // Restore.
        for (k, v) in &prev {
            match v {
                Some(val) => std::env::set_var(k, val),
                None => std::env::remove_var(k),
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
