//! Test helpers shared across the `argus-llm` crate's unit tests.
use async_trait::async_trait;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use crate::{CompletionRequest, CompletionResponse, LlmClient, LlmError};

#[derive(Clone)]
pub struct ScriptedMock {
    responses: Arc<StdMutex<Vec<Result<CompletionResponse, LlmError>>>>,
    call_count: Arc<AtomicU32>,
}

impl ScriptedMock {
    pub fn new(responses: Vec<Result<CompletionResponse, LlmError>>) -> Self {
        assert!(!responses.is_empty(), "ScriptedMock needs at least one response");
        Self {
            responses: Arc::new(StdMutex::new(responses)),
            call_count: Arc::new(AtomicU32::new(0)),
        }
    }

    pub fn call_count(&self) -> u32 {
        self.call_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl LlmClient for ScriptedMock {
    fn provider_name(&self) -> &str { "scripted" }
    async fn complete(&self, _request: CompletionRequest, _api_key: &str) -> Result<CompletionResponse, LlmError> {
        let i = self.call_count.fetch_add(1, Ordering::SeqCst) as usize;
        let script = self.responses.lock().unwrap();
        script.get(i).or(script.last()).expect("non-empty script").clone()
    }
}
