use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use mcp_toolkit_scratchpad::{
    DuckDbEngine, ScratchpadSessionConfig, ScratchpadSessionManager, SharedScratchpadEngine,
};
use rmcp::serve_server;
use rmcp::transport::stdio;
use tracing_subscriber::EnvFilter;

use google_search_console_mcp::config::{Cli, Settings};
use google_search_console_mcp::gsc_client::SearchConsoleClient;
use google_search_console_mcp::server::SearchConsoleMcp;
use google_search_console_mcp::tools::{registered_tool_names, registered_tool_schema_snapshot};

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("google-search-console-mcp failed to start: {err}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    init_tracing();

    let settings = Settings::from_cli(Cli::parse())?;
    if settings.print_tools {
        println!(
            "{}",
            serde_json::to_string_pretty(&registered_tool_names(settings.profile))?
        );
        return Ok(());
    }

    if settings.print_tool_schema {
        println!(
            "{}",
            serde_json::to_string_pretty(&registered_tool_schema_snapshot(settings.profile))?
        );
        return Ok(());
    }

    let client = Arc::new(SearchConsoleClient::from_settings(&settings).await?);
    let scratchpad_engine: SharedScratchpadEngine = Arc::new(DuckDbEngine::new()?);
    let scratchpad_config = ScratchpadSessionConfig::new(
        settings.scratchpad_session_ttl,
        settings.scratchpad_max_sessions,
        settings.scratchpad_max_tables_per_session,
        settings.scratchpad_max_rows_per_session,
        settings.scratchpad_max_memory_mb,
    )
    .with_root_dir(settings.scratchpad_root_dir.clone())
    .with_query_timeout(settings.scratchpad_query_timeout)
    .with_max_sql_bytes(settings.scratchpad_max_sql_bytes);
    let scratchpad_sessions = Arc::new(ScratchpadSessionManager::new(
        scratchpad_engine,
        scratchpad_config,
    )?);
    let server = SearchConsoleMcp::new(client, settings.profile, scratchpad_sessions);

    mcp_toolkit_observability::emit_event(
        mcp_toolkit_observability::Level::INFO,
        "gsc_mcp.startup",
        &mcp_toolkit_observability::EventContext::new(),
        &[
            mcp_toolkit_observability::safe_text("transport", "stdio"),
            mcp_toolkit_observability::safe_text("profile", settings.profile.as_str()),
        ],
    );

    let service = serve_server(server, stdio()).await?;
    service.waiting().await?;
    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_writer(std::io::stderr)
        .try_init();
}
