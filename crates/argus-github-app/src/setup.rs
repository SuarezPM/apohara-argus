//! GitHub App manifest flow.
//!
//! GitHub's manifest flow lets a third party (us) describe the
//! App we want a user to install, without ever asking the user
//! to click through github.com/settings/apps/new. The user
//! lands on a `github.com/settings/apps/new?manifest=...` URL,
//! confirms the permissions, and GitHub POSTs the created App's
//! credentials to our callback.
//!
//! We don't host the callback in this binary — the manifest is
//! static, and a properly configured operator can host the
//! callback wherever they want (Lambda, Cloudflare Worker, etc.).
//! The App itself only needs the manifest JSON, which we serve
//! at `GET /setup` for one-click "copy this URL, paste it into
//! a browser, click install" workflows.
//!
//! The manifest references the public App name, the
//! permissions, the events we want, and the callback URL.
//! Permissions are intentionally minimal:
//! - `pull_requests: read` — to fetch the diff
//! - `issues: write` — to post the verdict comment
//! - `metadata: read` — required for every App
//!
//! [Refs: argus-silver-roadmap/P.2]

use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;
use serde_json::Value;

use crate::app_state::AppState;

/// The hard-coded manifest we serve at `GET /setup`.
///
/// The fields are baked in at compile time so the manifest
/// cannot drift from the App's actual behavior at runtime. If
/// the operator needs to customize the name or homepage, they
/// fork the App — there is no env-var knob for the manifest
/// fields, by design.
///
/// The one exception is `hook_attributes.url`: the
/// per-deployment webhook endpoint is **not** a build-time
/// concern. It is sourced from the `ARGUS_APP_WEBHOOK_URL`
/// environment variable at request time, and the static
/// `MANIFEST` carries an empty string in that slot. The empty
/// string is overwritten by [`manifest_with_webhook_url`].
///
/// `https://github.com/SuarezPM/apohara-argus` is the
/// canonical repo home. The same URL is used as the App's
/// public homepage, the setup URL, and the post-install
/// redirect.
pub const MANIFEST: &str = r#"{
  "name": "ARGUS",
  "url": "https://github.com/SuarezPM/apohara-argus",
  "hook_attributes": {
    "url": "",
    "active": true
  },
  "redirect_url": "https://github.com/SuarezPM/apohara-argus",
  "callback_urls": [],
  "public": true,
  "default_events": [
    "pull_request"
  ],
  "default_permissions": {
    "pull_requests": "read",
    "issues": "write",
    "metadata": "read"
  }
}"#;

/// Build a manifest JSON string with `hook_attributes.url`
/// set to `<base>/webhook`. The trailing slash on `base` is
/// tolerated (trimmed), so operators can pass
/// `https://example.com` or `https://example.com/` and get
/// the same result.
///
/// This is the *only* mutable field in the manifest at
/// runtime. All other fields stay compile-time-constant so
/// they cannot drift from the App's actual permissions.
pub fn manifest_with_webhook_url(base: &str) -> String {
    let mut v: Value =
        serde_json::from_str(MANIFEST).expect("MANIFEST is valid JSON (enforced by tests)");
    let trimmed = base.trim_end_matches('/');
    v["hook_attributes"]["url"] = Value::String(format!("{}/webhook", trimmed));
    serde_json::to_string(&v).expect("re-serializing a parsed Value is infallible")
}

/// The JSON shape we return at `GET /setup`. The wrapper struct
/// gives us a place to add a one-liner summary without breaking
/// the manifest structure that GitHub expects.
#[derive(Debug, Serialize)]
pub struct SetupResponse {
    pub manifest: Value,
    pub install_url: String,
    pub notes: &'static str,
}

impl SetupResponse {
    pub fn current() -> Self {
        // The webhook URL is **per-deployment**: we read it
        // from the environment at request time. The default
        // is a visible placeholder so a missing env var is
        // obvious in the rendered manifest — GitHub will
        // refuse the install (or silently drop events) if the
        // URL is left as the placeholder.
        let base = std::env::var("ARGUS_APP_WEBHOOK_URL")
            .unwrap_or_else(|_| "https://REPLACE_AT_INSTALL".to_string());
        let manifest_str = manifest_with_webhook_url(&base);
        let manifest: Value = serde_json::from_str(&manifest_str)
            .expect("manifest_with_webhook_url returns valid JSON");
        let install_url = format!(
            "https://github.com/settings/apps/new?manifest={}",
            urlencoding(&manifest_str)
        );
        Self {
            manifest,
            install_url,
            notes: "POST the `manifest` field to https://api.github.com/app-manifests/{code} after GitHub shows you the confirmation page. The webhook URL inside the manifest is read from the ARGUS_APP_WEBHOOK_URL env var at request time. Operators MUST set ARGUS_APP_WEBHOOK_URL to a URL they control BEFORE installing — the placeholder default (https://REPLACE_AT_INSTALL/webhook) will not receive events.",
        }
    }
}

/// Minimal URL-encoding for the `?manifest=...` query string.
/// We avoid pulling in the `url` crate's `form_urlencoded` for
/// one call site. Only encodes the characters GitHub's manifest
/// endpoint actually rejects.
fn urlencoding(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}

/// `GET /setup` — return the GitHub App manifest and a
/// pre-built install URL the user can click.
///
/// The route is unauthenticated: the manifest is not a secret.
/// Anyone can see what permissions the App requests.
pub async fn setup_handler() -> impl IntoResponse {
    Json(SetupResponse::current())
}

/// `GET /version` — service + version + git SHA. Used by the
/// marketplace listing ("v0.1.0 — published from abc1234") and
/// by the App's own health probes.
pub async fn version_handler() -> impl IntoResponse {
    Json(crate::app_state::VersionResponse::current())
}

/// `GET /health` — liveness probe.
///
/// The shape is intentionally boring: `{"ok": true, "service":
/// ..., "version": ...}`. Fly's `[[services.http_checks]]` block
/// matches on HTTP 200 only, so we don't need to return anything
/// richer.
pub async fn health_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(crate::app_state::HealthResponse::current()),
    )
}

/// `GET /` — landing page with a one-paragraph pitch and the
/// install button. Returned as plain text so it renders in
/// `curl` output without a browser.
pub async fn index_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        [("content-type", "text/plain; charset=utf-8")],
        INDEX_BODY.to_string(),
    )
}

const INDEX_BODY: &str = "ARGUS — AI slop defense for code review

POST /webhook   receives GitHub pull_request events
GET  /health    liveness probe
GET  /setup     GitHub App manifest (visit GET /setup/install to install)
GET  /version   service + version info

Visit /setup in a browser to see the manifest JSON and the
pre-built install URL. See https://github.com/SuarezPM/apohara-argus
for full documentation.
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_is_valid_json() {
        let _: Value = serde_json::from_str(MANIFEST).expect("MANIFEST must be valid JSON");
    }

    #[test]
    fn manifest_has_minimal_permissions() {
        let v: Value = serde_json::from_str(MANIFEST).unwrap();
        let perms = v["default_permissions"]
            .as_object()
            .expect("permissions object");
        // We deliberately do NOT request `contents: write` or
        // any write scope beyond `issues`.
        assert!(perms.contains_key("pull_requests"));
        assert!(perms.contains_key("issues"));
        assert!(perms.contains_key("metadata"));
        for (k, v) in perms {
            let scope = v.as_str().unwrap_or("");
            if k == "pull_requests" || k == "metadata" {
                assert_eq!(scope, "read", "{} should be read-only", k);
            } else if k == "issues" {
                assert_eq!(scope, "write", "{} is write (we post comments)", k);
            } else {
                panic!("unexpected permission key: {}", k);
            }
        }
    }

    #[test]
    fn install_url_includes_manifest_query() {
        let resp = SetupResponse::current();
        assert!(resp
            .install_url
            .starts_with("https://github.com/settings/apps/new?manifest="));
        // The encoded manifest should be non-trivial in length.
        assert!(resp.install_url.len() > 200);
    }

    #[test]
    fn urlencoding_handles_braces_and_quotes() {
        let encoded = urlencoding("{}");
        assert_eq!(encoded, "%7B%7D");
        let encoded = urlencoding("\"");
        assert_eq!(encoded, "%22");
    }

    #[test]
    fn manifest_with_webhook_url_sets_url() {
        let out = manifest_with_webhook_url("https://example.com");
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            v["hook_attributes"]["url"]
                .as_str()
                .expect("hook url is a string"),
            "https://example.com/webhook"
        );
    }

    #[test]
    fn manifest_with_webhook_url_trims_trailing_slash() {
        let out = manifest_with_webhook_url("https://example.com/");
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            v["hook_attributes"]["url"]
                .as_str()
                .expect("hook url is a string"),
            "https://example.com/webhook"
        );
    }

    #[test]
    fn manifest_with_webhook_url_preserves_other_fields() {
        // The webhook URL is the ONLY field that changes —
        // permissions, events, and the public URL must stay
        // byte-for-byte identical to the static MANIFEST.
        let out = manifest_with_webhook_url("https://example.com");
        let original: Value = serde_json::from_str(MANIFEST).unwrap();
        let mutated: Value = serde_json::from_str(&out).unwrap();
        for key in [
            "name",
            "url",
            "redirect_url",
            "callback_urls",
            "public",
            "default_events",
            "default_permissions",
        ] {
            assert_eq!(
                original[key], mutated[key],
                "field `{}` must not change when only the webhook URL is rewritten",
                key
            );
        }
    }

    #[test]
    fn env_var_overrides_webhook_url() {
        // This is the *only* env-mutating test in the file;
        // a single Mutex serializes it against any future
        // env-mutating test (poisoned lock is treated as
        // released so a panicking sibling does not block
        // this one forever).
        use std::sync::Mutex;
        static ENV_LOCK: Mutex<()> = Mutex::new(());
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let prev = std::env::var("ARGUS_APP_WEBHOOK_URL").ok();
        std::env::set_var("ARGUS_APP_WEBHOOK_URL", "https://env-test.example.com");
        let resp = SetupResponse::current();
        // Always restore — the env var is process-global.
        match prev {
            Some(v) => std::env::set_var("ARGUS_APP_WEBHOOK_URL", v),
            None => std::env::remove_var("ARGUS_APP_WEBHOOK_URL"),
        }
        assert_eq!(
            resp.manifest["hook_attributes"]["url"]
                .as_str()
                .expect("hook url is a string"),
            "https://env-test.example.com/webhook"
        );
    }
}
