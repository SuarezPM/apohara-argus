// SPDX-License-Identifier: MIT
//
// Optional DALL-E image generation adapter for the audit-chain dashboard.
// Gated behind the `dalle` Cargo feature (off by default per CLAUDE.md
// "NEVER default-on a sensitive surface"); the public surface is a single
// `generate()` async fn that returns the image URL or an error.
//
// The actual implementation was never landed (only the feature flag and
// the `pub mod` declaration in `lib.rs`); this stub exists so that
// `cargo fmt`, IDEs, and `cargo test --no-run` can resolve the module
// path. Enabling the feature without committing the real implementation
// will fail to compile; this is intentional.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// Configuration for the DALL-E adapter. Loaded from
/// `ARGUS_DALLE_API_KEY` + `ARGUS_DALLE_ENDPOINT` env vars.
#[derive(Clone, Debug)]
pub struct DalleConfig {
    pub api_key: String,
    pub endpoint: String,
    pub model: String,
}

/// A request to generate a single image from a prompt.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DalleRequest {
    pub prompt: String,
    pub size: String,
}

/// The result of a successful image generation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DalleResponse {
    pub url: String,
    pub revised_prompt: String,
}

/// Stub: the real implementation is not yet committed. Returns
/// `Err` unconditionally so a feature-enabled build that somehow
/// reaches this path fails loudly instead of silently doing nothing.
pub async fn generate(_cfg: &DalleConfig, _req: &DalleRequest) -> Result<DalleResponse, String> {
    Err(
        "argus-llm/dalle: stub implementation; real DALL-E adapter not yet committed. \
         Disable the `dalle` feature or open an issue at \
         https://github.com/SuarezPM/apohara-argus/issues"
            .to_string(),
    )
}
