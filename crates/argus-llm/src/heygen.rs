// SPDX-License-Identifier: MIT
//
// Optional HeyGen avatar-video generation adapter. Gated behind the
// `heygen` Cargo feature (off by default per CLAUDE.md "NEVER default-on
// a sensitive surface"). The real implementation was never landed (only
// the feature flag and the `pub mod` declaration in `lib.rs`); this
// stub exists so `cargo fmt`, IDEs, and `cargo test --no-run` can
// resolve the module path. Enabling the feature without committing
// the real implementation will fail to compile; this is intentional.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// Configuration for the HeyGen adapter. Loaded from
/// `ARGUS_HEYGEN_API_KEY` + `ARGUS_HEYGEN_ENDPOINT` env vars.
#[derive(Clone, Debug)]
pub struct HeygenConfig {
    pub api_key: String,
    pub endpoint: String,
}

/// A request to generate an avatar video from a text script.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HeygenRequest {
    pub script: String,
    pub avatar_id: String,
    pub voice_id: String,
}

/// The result of a successful video generation job.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HeygenResponse {
    pub job_id: String,
    pub video_url: String,
    pub status: String,
}

/// Stub: the real implementation is not yet committed. Returns
/// `Err` unconditionally so a feature-enabled build that somehow
/// reaches this path fails loudly instead of silently doing nothing.
pub async fn generate_video(
    _cfg: &HeygenConfig,
    _req: &HeygenRequest,
) -> Result<HeygenResponse, String> {
    Err(
        "argus-llm/heygen: stub implementation; real HeyGen adapter not yet committed. \
         Disable the `heygen` feature or open an issue at \
         https://github.com/SuarezPM/apohara-argus/issues"
            .to_string(),
    )
}
