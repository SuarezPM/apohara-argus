//! argus-slop — the 4 ARGUS analyzers
//!
//! Each analyzer:
//! 1. Loads a prompt from `argus-core::prompts`
//! 2. Builds a request: (system prompt, diff, optional context)
//! 3. Calls the LLM via the BYOK key
//! 4. Parses the JSON response into a structured finding
//!
//! The 4 analyzers:
//! - `SlopDetector`   — detects AI slop signals
//! - `SecurityReview` — adversarial security review
//! - `ArchitectureFit` — coherence with the repo
//! - `VerdictSynthesizer` — final verdict from the other 3 outputs

pub mod architecture;
pub mod deterministic;
pub mod pipeline;
pub mod security;
pub mod slop_detector;
pub mod verdict;

pub use architecture::{ArchReport, ArchitectureFit};
pub use deterministic::{run_deterministic_rules, Severity, SlopSignal, OVERSIZED_FN_LOC};
pub use pipeline::{AnalysisPipeline, PipelineOutput};
pub use security::{SecurityFinding, SecurityReport, SecurityReview};
pub use slop_detector::{SlopDetector, SlopReport};
pub use verdict::{SynthesizerInput, VerdictSynthesizer};

use argus_core::{PRFinding, Result as ArgusResult};
use argus_llm::LlmClient;
use async_trait::async_trait;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SlopError {
    #[error("LLM error: {0}")]
    Llm(String),
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("Prompt error: {0}")]
    Prompt(String),
}

impl From<argus_llm::LlmError> for SlopError {
    fn from(e: argus_llm::LlmError) -> Self {
        Self::Llm(e.to_string())
    }
}

impl From<argus_core::ArgusError> for SlopError {
    fn from(e: argus_core::ArgusError) -> Self {
        Self::Prompt(e.to_string())
    }
}

/// Common trait for all analyzers.
#[async_trait]
pub trait Analyzer: Send + Sync {
    type Output: serde::Serialize + serde::de::DeserializeOwned + Send + Sync;

    fn name(&self) -> &'static str;
    fn prompt_name(&self) -> &'static str;

    /// Build the user message for the LLM.
    fn build_user_message(&self, diff: &str, context: Option<&str>) -> String;

    /// Parse the LLM response into the structured output.
    fn parse_response(&self, raw: &str) -> Result<Self::Output, SlopError>;

    /// Run the analyzer: load prompt → call LLM → parse response.
    async fn run(
        &self,
        client: &dyn LlmClient,
        diff: &str,
        context: Option<&str>,
        api_key: &str,
    ) -> Result<Self::Output, SlopError> {
        let lib = argus_core::PromptLibrary::load_embedded()?;
        let prompt = lib.get(self.prompt_name()).ok_or_else(|| {
            SlopError::Prompt(format!("prompt '{}' not found", self.prompt_name()))
        })?;
        let user_msg = self.build_user_message(diff, context);
        let resp = client
            .complete_one_shot(
                &prompt.metadata.model,
                &prompt.body,
                &user_msg,
                api_key,
                prompt.metadata.temperature,
                prompt.metadata.max_tokens,
            )
            .await?;
        self.parse_response(&resp.content)
    }
}

/// Helper to extract JSON from an LLM response (LLMs often wrap it in
/// markdown code fences or add prose around it).
pub fn extract_json(raw: &str) -> String {
    // Strip markdown code fences
    let s = raw.trim();
    let s = s.strip_prefix("```json").unwrap_or(s);
    let s = s.strip_prefix("```").unwrap_or(s);
    let s = s.strip_suffix("```").unwrap_or(s);
    s.trim().to_string()
}
