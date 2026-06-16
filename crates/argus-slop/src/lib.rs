//! argus-slop — the 4 ARGUS analyzers
//!
//! Each analyzer:
//! 1. Loads a prompt from `apohara-argus-core::prompts`
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

impl From<apohara_argus_core::ArgusError> for SlopError {
    fn from(e: apohara_argus_core::ArgusError) -> Self {
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
        let lib = apohara_argus_core::PromptLibrary::load_embedded()?;
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

#[cfg(test)]
mod tests {
    //! Unit tests for the shared slop crate surface: the `extract_json`
    //! helper (used by all 4 analyzers) + the `SlopError` Display impls.
    use super::*;

    #[test]
    fn extract_json_handles_bare_json() {
        // The simplest case: the LLM returns clean JSON.
        let raw = r#"{"a":1,"b":"x"}"#;
        assert_eq!(extract_json(raw), raw);
    }

    #[test]
    fn extract_json_strips_json_fence() {
        // The most common case: ```json ... ```
        let raw = "```json\n{\"a\":1}\n```";
        assert_eq!(extract_json(raw), "{\"a\":1}");
    }

    #[test]
    fn extract_json_strips_plain_fence() {
        // Some models emit ``` without the json tag.
        let raw = "```\n{\"a\":1}\n```";
        assert_eq!(extract_json(raw), "{\"a\":1}");
    }

    #[test]
    fn extract_json_strips_surrounding_whitespace() {
        // The `trim()` on entry handles leading/trailing whitespace.
        let raw = "   \n{\"a\":1}\n   ";
        assert_eq!(extract_json(raw), "{\"a\":1}");
    }

    #[test]
    fn extract_json_handles_prose_around_json() {
        // LLMs sometimes return: "Here is the result: {...} Hope this helps!"
        // The current implementation only strips fences, not prose.
        // We document the behavior: prose stays, fence gets stripped.
        let raw = "Here is the result:\n{\"a\":1}\nDone.";
        assert_eq!(extract_json(raw), "Here is the result:\n{\"a\":1}\nDone.");
    }

    #[test]
    fn extract_json_handles_unicode() {
        // The analyzer prompts can produce non-ASCII (e.g. Spanish
        // code comments). The extractor must not corrupt them.
        let raw = r#"{"msg":"hola — ñoño"}"#;
        assert_eq!(extract_json(raw), raw);
    }

    #[test]
    fn slop_error_llm_display() {
        // The Display impl is what shows up in the audit chain +
        // the CLI. The format must be stable for log parsers.
        let e = SlopError::Llm("rate limited".to_string());
        assert_eq!(e.to_string(), "LLM error: rate limited");
    }

    #[test]
    fn slop_error_parse_display() {
        let e = SlopError::Parse("unexpected token at line 5".to_string());
        assert_eq!(e.to_string(), "Parse error: unexpected token at line 5");
    }

    #[test]
    fn slop_error_prompt_display() {
        let e = SlopError::Prompt("prompt 'foo' not found".to_string());
        assert_eq!(e.to_string(), "Prompt error: prompt 'foo' not found");
    }

    #[test]
    fn slop_error_debug_includes_variant() {
        // The Debug impl is used in test failures and panic
        // messages; the variant name must be present so we can
        // distinguish Parse vs Llm vs Prompt in the logs.
        let llm = format!("{:?}", SlopError::Llm("x".to_string()));
        let parse = format!("{:?}", SlopError::Parse("x".to_string()));
        let prompt = format!("{:?}", SlopError::Prompt("x".to_string()));
        assert!(llm.contains("Llm"));
        assert!(parse.contains("Parse"));
        assert!(prompt.contains("Prompt"));
    }

    #[test]
    fn re_exports_are_accessible_from_crate_root() {
        // The 4 analyzers + their reports must be importable as
        // `argus_slop::Analyzer` etc. (the `pub use` in lib.rs).
        // We just check the type names resolve.
        let _: fn() -> ArchitectureFit = ArchitectureFit::new;
        let _: fn() -> SecurityReview = SecurityReview::new;
        let _: fn() -> SlopDetector = SlopDetector::new;
        let _: fn() -> VerdictSynthesizer = VerdictSynthesizer::new;
    }
}
