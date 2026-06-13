//! Integration tests for the argus-github-app HTTP server.
//!
//! These tests use a mock GitHub API server (a tiny axum router
//! bound to a random local port) and drive the App's webhook
//! endpoint via `tower::ServiceExt::oneshot` — no real network,
//! no real GitHub, no real NIM. The four tests cover:
//!
//! 1. `webhook_with_valid_signature_succeeds` — happy path
//! 2. `webhook_with_invalid_signature_returns_401`
//! 3. `webhook_with_oversized_payload_returns_413`
//! 4. `webhook_with_unknown_event_is_ignored`
//!
//! To inject the mock GitHub URL into the handler, the handler
//! reads `ARGUS_GITHUB_API_BASE_URL` from the env (test-only
//! override; production hard-codes `https://api.github.com`).
//!
//! [Refs: argus-silver-roadmap/P.2]

use std::sync::Arc;

use argus_github_app::{
    app_state::{AppConfig, AppState},
    signature::sign,
    webhook::webhook_handler,
};
use axum::{
    body::Body,
    extract::DefaultBodyLimit,
    http::{Request, StatusCode},
    response::Response,
    routing::{get, post},
    Router,
};
use http_body_util::BodyExt;
use tokio::sync::Mutex;
use tower::util::ServiceExt;

const WEBHOOK_SECRET: &[u8] = b"test-webhook-secret-123";
const INSTALL_TOKEN: &str = "test-install-token";

// The webhook handler reads `ARGUS_APP_INSTALL_TOKEN` +
// `ARGUS_GITHUB_API_BASE_URL` from the env at request time
// (test-only override path). `cargo test` runs tests in
// parallel within a single process, so env-var access must
// be serialized. The Mutex below guards the
// install-token + base-URL section of every test that needs
// them; tests that do not (test 2, test 3) skip the lock.
static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

// =====================================================================
// Mock GitHub API
// =====================================================================

struct MockGitHub {
    addr: std::net::SocketAddr,
    state: Arc<MockState>,
}

#[derive(Default)]
struct MockState {
    diff: Mutex<Option<String>>,
    comments: Mutex<Vec<(u32, String)>>,
    labels: Mutex<Vec<(u32, Vec<String>)>>,
    call_count: Mutex<u32>,
}

impl MockGitHub {
    async fn start() -> Self {
        let state = Arc::new(MockState::default());
        *state.diff.lock().await = Some(
            "diff --git a/src/lib.rs b/src/lib.rs\n+pub fn new_function() {}\n".to_string(),
        );

        let app = Router::new()
            .route(
                "/repos/:owner/:repo/pulls/:number",
                get({
                    let state = state.clone();
                    move |path: axum::extract::Path<(String, String, u32)>,
                          headers: axum::http::HeaderMap| {
                        let state = state.clone();
                        async move {
                            let (owner, repo, number) = path.0;
                            let accept = headers
                                .get("accept")
                                .and_then(|v| v.to_str().ok())
                                .unwrap_or("");
                            let diff = state.diff.lock().await.clone().unwrap_or_default();
                            if accept.contains("vnd.github.v3.diff") {
                                axum::response::Response::builder()
                                    .status(200)
                                    .header("content-type", "application/vnd.github.v3.diff")
                                    .body(Body::from(diff))
                                    .unwrap()
                            } else {
                                let body = serde_json::json!({
                                    "number": number,
                                    "title": "test",
                                    "state": "open",
                                    "head_sha": "abc",
                                    "base_sha": "def",
                                    "html_url": format!(
                                        "https://github.com/{}/{}/pull/{}", owner, repo, number
                                    ),
                                    "additions": 1,
                                    "deletions": 0,
                                    "changed_files": 1,
                                });
                                axum::response::Response::builder()
                                    .status(200)
                                    .header("content-type", "application/json")
                                    .body(Body::from(body.to_string()))
                                    .unwrap()
                            }
                        }
                    }
                }),
            )
            .route(
                "/repos/:owner/:repo/issues/:number/comments",
                post({
                    let state = state.clone();
                    move |path: axum::extract::Path<(String, String, u32)>,
                          body: axum::body::Bytes| {
                        let state = state.clone();
                        async move {
                            let (_, _, number) = path.0;
                            let parsed: serde_json::Value =
                                serde_json::from_slice(&body).unwrap_or_default();
                            let text = parsed["body"].as_str().unwrap_or("").to_string();
                            state.comments.lock().await.push((number, text));
                            let mut count = state.call_count.lock().await;
                            *count += 1;
                            let resp = serde_json::json!({ "id": *count });
                            axum::response::Response::builder()
                                .status(201)
                                .header("content-type", "application/json")
                                .body(Body::from(resp.to_string()))
                                .unwrap()
                        }
                    }
                }),
            )
            .route(
                "/repos/:owner/:repo/issues/:number/labels",
                axum::routing::put({
                    let state = state.clone();
                    move |path: axum::extract::Path<(String, String, u32)>,
                          body: axum::body::Bytes| {
                        let state = state.clone();
                        async move {
                            let (_, _, number) = path.0;
                            let parsed: serde_json::Value =
                                serde_json::from_slice(&body).unwrap_or_default();
                            let labels: Vec<String> = parsed["labels"]
                                .as_array()
                                .map(|a| {
                                    a.iter()
                                        .filter_map(|v| v.as_str().map(String::from))
                                        .collect()
                                })
                                .unwrap_or_default();
                            state.labels.lock().await.push((number, labels));
                            axum::response::Response::builder()
                                .status(200)
                                .header("content-type", "application/json")
                                .body(Body::from("[]"))
                                .unwrap()
                        }
                    }
                }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.ok();
        });
        Self { addr, state }
    }

    fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }
}

// =====================================================================
// Test harness
// =====================================================================

fn make_app() -> Router {
    let config = AppConfig {
        webhook_secret: String::from_utf8(WEBHOOK_SECRET.to_vec()).unwrap(),
        label_pass: "argus/approved".into(),
        label_warn: "argus/needs-review".into(),
        label_fail: "argus/halted".into(),
        allowed_repos: vec![],
        event_allowlist: vec!["pull_request".into()],
    };
    let state = AppState::new(config);
    // Mirror the body-limit raise in main.rs so test 3 can
    // hit the Cordon's 10 MiB cap rather than axum's 2 MiB
    // default.
    Router::new()
        .route(
            "/webhook",
            post(webhook_handler).layer(DefaultBodyLimit::max(11 * 1024 * 1024)),
        )
        .with_state(state)
}

fn sample_pr_event(action: &str) -> serde_json::Value {
    serde_json::json!({
        "action": action,
        "number": 42,
        "pull_request": {
            "number": 42,
            "head": { "sha": "abc123", "ref": "feature-branch" },
            "base": { "sha": "def456", "ref": "main" },
            "html_url": "https://github.com/octocat/hello-world/pull/42"
        },
        "repository": {
            "full_name": "octocat/hello-world",
            "html_url": "https://github.com/octocat/hello-world"
        },
        "installation": { "id": 12345 }
    })
}

async fn body_string(resp: Response) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

// =====================================================================
// Test 1: valid signature -> success
// =====================================================================

#[tokio::test]
async fn webhook_with_valid_signature_succeeds() {
    let _guard = ENV_LOCK.lock().await;
    let gh = MockGitHub::start().await;
    std::env::set_var("ARGUS_APP_INSTALL_TOKEN", INSTALL_TOKEN);
    std::env::set_var("ARGUS_GITHUB_API_BASE_URL", &gh.base_url());

    let app = make_app();
    let body = sample_pr_event("opened").to_string();
    let sig = sign(WEBHOOK_SECRET, body.as_bytes());

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhook")
                .header("x-hub-signature-256", sig)
                .header("x-github-event", "pull_request")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "valid signature should be accepted"
    );

    // The handler returns 200 immediately and spawns the review
    // in a tokio task. Wait for the task to finish.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let comments = gh.state.comments.lock().await.clone();
    let labels = gh.state.labels.lock().await.clone();
    assert_eq!(comments.len(), 1, "should have posted exactly one comment");
    assert_eq!(comments[0].0, 42, "comment is on PR #42");
    assert!(
        comments[0].1.contains("ARGUS deterministic review"),
        "comment body should contain the verdict header: got {}",
        comments[0].1
    );
    assert!(
        comments[0].1.contains("octocat/hello-world#42"),
        "comment should reference the PR"
    );

    assert_eq!(labels.len(), 1, "should have set exactly one label");
    assert_eq!(labels[0].0, 42);
    assert_eq!(labels[0].1, vec!["argus/approved"]);

    std::env::remove_var("ARGUS_APP_INSTALL_TOKEN");
    std::env::remove_var("ARGUS_GITHUB_API_BASE_URL");
}

// =====================================================================
// Test 2: invalid signature -> 401
// =====================================================================

#[tokio::test]
async fn webhook_with_invalid_signature_returns_401() {
    let gh = MockGitHub::start().await;
    let app = make_app();

    let body = sample_pr_event("opened").to_string();
    let wrong_sig = sign(b"wrong-secret", body.as_bytes());

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhook")
                .header("x-hub-signature-256", wrong_sig)
                .header("x-github-event", "pull_request")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "wrong HMAC must be rejected"
    );
    let body = body_string(resp).await;
    assert!(body.contains("invalid signature"));

    assert_eq!(gh.state.comments.lock().await.len(), 0);
    assert_eq!(gh.state.labels.lock().await.len(), 0);
}

// =====================================================================
// Test 3: oversized payload -> 413
// =====================================================================

#[tokio::test]
async fn webhook_with_oversized_payload_returns_413() {
    let gh = MockGitHub::start().await;
    let app = make_app();

    let big = vec![b'a'; argus_github_app::cordon::MAX_PAYLOAD_BYTES + 1];
    let sig = sign(WEBHOOK_SECRET, &big);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhook")
                .header("x-hub-signature-256", sig)
                .header("x-github-event", "pull_request")
                .header("content-type", "application/json")
                .body(Body::from(big))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::PAYLOAD_TOO_LARGE,
        "oversized payload must be rejected with 413"
    );
    let body = body_string(resp).await;
    assert!(body.contains("too large"), "body explains the rejection: {}", body);

    assert_eq!(gh.state.comments.lock().await.len(), 0);
    assert_eq!(gh.state.labels.lock().await.len(), 0);
}

// =====================================================================
// Test 4: unknown event -> 200, no action
// =====================================================================

#[tokio::test]
async fn webhook_with_unknown_event_is_ignored() {
    let _guard = ENV_LOCK.lock().await;
    let gh = MockGitHub::start().await;
    let app = make_app();

    std::env::set_var("ARGUS_APP_INSTALL_TOKEN", INSTALL_TOKEN);
    std::env::set_var("ARGUS_GITHUB_API_BASE_URL", &gh.base_url());

    // A `pull_request` event (in the allowlist) but with an
    // action we don't act on (`closed`). Per the GitHub
    // webhook spec, we must return 2xx for these — otherwise
    // GitHub retries forever and the App's install becomes
    // a noise source in the user's repo.
    let body = sample_pr_event("closed").to_string();
    let sig = sign(WEBHOOK_SECRET, body.as_bytes());

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhook")
                .header("x-hub-signature-256", sig)
                .header("x-github-event", "pull_request")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("ignored"), "body explains the no-op: {}", body);

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(
        gh.state.comments.lock().await.len(),
        0,
        "closed action must not post a comment"
    );
    assert_eq!(
        gh.state.labels.lock().await.len(),
        0,
        "closed action must not set a label"
    );

    std::env::remove_var("ARGUS_APP_INSTALL_TOKEN");
    std::env::remove_var("ARGUS_GITHUB_API_BASE_URL");
}
