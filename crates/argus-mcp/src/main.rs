//! `argus-mcp` binary entry point. [Refs: 5]
//!
//! Starts the MCP server on stdio. Designed to be launched by an MCP
//! client (Claude Code / Codex / Cursor) like:
//!
//! ```json
//! { "mcpServers": { "argus": { "command": "argus-mcp",
//!                              "args": [],
//!                              "env": { "ARGUS_NIM_KEY": "nvapi-..." } } } }
//! ```
//!
//! The NIM key is read from `ARGUS_NIM_KEY` at every tool call, so the
//! operator can rotate it without restarting the server.

use argus_mcp::ArgusMcp;
use rmcp::transport::io::stdio;
use rmcp::ServiceExt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // OTel init [Refs: 6.3] — opt-in via `ARGUS_OTEL_DISABLED`. The
    // `try_init` is a no-op when OTel is disabled.
    let _otel_guard = argus_otel::init("argus-mcp");
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,argus=debug")),
        )
        .try_init();

    tracing::info!(
        "argus-mcp starting on stdio (ARGUS_NIM_KEY {})",
        if std::env::var("ARGUS_NIM_KEY")
            .ok()
            .filter(|s| !s.is_empty())
            .is_some()
        {
            "set"
        } else {
            "NOT SET — tools will return an error until set"
        }
    );

    let server = ArgusMcp::new().serve(stdio()).await?;
    server.waiting().await?;
    Ok(())
}
