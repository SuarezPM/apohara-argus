//! End-to-end test of the analysis pipeline with the real NVIDIA NIM BYOK key.

use argus_llm::{LlmClient, NimClient};
use argus_slop::pipeline::AnalysisPipeline;

const FAKE_SECRET_DIFF: &str = r#"
diff --git a/src/config.py b/src/config.py
@@ -1,3 +1,5 @@
+# AWS credentials
+AWS_ACCESS_KEY = "AKIAIOSFODNN7EXAMPLE"
+AWS_SECRET_KEY = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"
"#;

fn load_key() -> Option<String> {
    std::env::var("ARGUS_NIM_KEY")
        .ok()
        .filter(|s| !s.is_empty())
}

#[tokio::test]
#[ignore = "requires ARGUS_NIM_KEY and internet"]
async fn raw_security_call_works() {
    let Some(key) = load_key() else {
        return;
    };
    let client = NimClient::new();
    let lib = apohara_argus_core::PromptLibrary::load_embedded().expect("load");
    let prompt = lib.get("redteam-security").expect("prompt");
    let diff = FAKE_SECRET_DIFF;
    let resp = client
        .complete_one_shot(
            &prompt.metadata.model,
            &prompt.body,
            &format!("Review this diff:\n```diff\n{}\n```", diff),
            &key,
            prompt.metadata.temperature,
            prompt.metadata.max_tokens,
        )
        .await
        .expect("llm call");
    eprintln!(
        "\n=== RAW SECURITY RESPONSE (first 600 chars) ===\n{}\n=== END ===\n",
        &resp.content[..resp.content.len().min(600)]
    );
}

#[tokio::test]
#[ignore = "requires ARGUS_NIM_KEY and internet"]
async fn raw_slop_call_works() {
    let Some(key) = load_key() else {
        return;
    };
    let client = NimClient::new();
    let lib = apohara_argus_core::PromptLibrary::load_embedded().expect("load");
    let prompt = lib.get("slop-detector").expect("prompt");
    let diff = FAKE_SECRET_DIFF;
    let resp = client
        .complete_one_shot(
            &prompt.metadata.model,
            &prompt.body,
            &format!("Analyze this diff:\n```diff\n{}\n```", diff),
            &key,
            prompt.metadata.temperature,
            prompt.metadata.max_tokens,
        )
        .await
        .expect("llm call");
    eprintln!(
        "\n=== RAW SLOP RESPONSE (first 600 chars) ===\n{}\n=== END ===\n",
        &resp.content[..resp.content.len().min(600)]
    );
}

#[tokio::test]
#[ignore = "requires ARGUS_NIM_KEY and internet"]
async fn pipeline_runs_end_to_end() {
    let Some(key) = load_key() else {
        return;
    };
    let client = NimClient::new();
    let pipeline = AnalysisPipeline::new();
    let out = pipeline
        .run(&client, "owner/repo#1", FAKE_SECRET_DIFF, None, &key)
        .await;
    eprintln!("\n=== Pipeline output ===\n{:#?}\n", out);
}
