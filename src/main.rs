use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use rmcp::serve_server;
use rmcp::transport::stdio;
use tracing_subscriber::EnvFilter;

use google_search_console_mcp::config::{Cli, Settings};
use google_search_console_mcp::gsc_client::SearchConsoleClient;
use google_search_console_mcp::server::SearchConsoleMcp;

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
    let client = Arc::new(SearchConsoleClient::from_settings(&settings).await?);
    let server = SearchConsoleMcp::new(client, settings.profile);

    if settings.print_tools {
        println!("{}", serde_json::to_string_pretty(&server.tool_names())?);
        return Ok(());
    }

    if settings.print_tool_schema {
        println!(
            "{}",
            serde_json::to_string_pretty(&server.tool_schema_snapshot())?
        );
        return Ok(());
    }

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
