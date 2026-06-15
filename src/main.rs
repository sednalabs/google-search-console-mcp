use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use rmcp::serve_server;
use rmcp::transport::stdio;
use tracing_subscriber::EnvFilter;

use google_search_console_mcp::auth_ux::run_auth_command;
use google_search_console_mcp::config::{Cli, Settings};
use google_search_console_mcp::config::{CliCommand, WRITE_SCOPE, scope_allows_mutation};
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
    if let Some(command) = settings.command.clone() {
        match command {
            CliCommand::Serve => {}
            CliCommand::Auth(auth) => {
                run_auth_command(&settings, &auth.command).await?;
                return Ok(());
            }
        }
    }

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

    if settings.profile.allows_mutation() && !scope_allows_mutation(&settings.scope) {
        eprintln!(
            "warning: operator profile is enabled but the configured scope does not include the write scope; run `google-search-console-mcp auth login --write-scope` and set GOOGLE_SEARCH_CONSOLE_MCP_SCOPE={WRITE_SCOPE} before using mutation tools"
        );
    }

    let client = Arc::new(SearchConsoleClient::from_settings(&settings).await?);
    let server = SearchConsoleMcp::new(client, settings.profile);

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
