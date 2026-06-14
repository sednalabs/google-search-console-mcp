//! CLI and environment-backed configuration.

use std::time::Duration;

use anyhow::{Result, anyhow};
use clap::{Parser, ValueEnum};

pub const DEFAULT_SCOPE: &str = "https://www.googleapis.com/auth/webmasters.readonly";
pub const WRITE_SCOPE: &str = "https://www.googleapis.com/auth/webmasters";
const DEFAULT_API_BASE_URL: &str = "https://www.googleapis.com/webmasters/v3";
const DEFAULT_INSPECTION_BASE_URL: &str = "https://searchconsole.googleapis.com/v1";
const DEFAULT_HTTP_TIMEOUT_MS: u64 = 15_000;
const DEFAULT_MAX_ROW_LIMIT: u32 = 25_000;
const DEFAULT_USER_AGENT: &str = "google-search-console-mcp/0.1.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "snake_case")]
pub enum CapabilityProfile {
    ReadOnly,
    Operator,
}

impl CapabilityProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::Operator => "operator",
        }
    }

    pub fn allows_mutation(self) -> bool {
        matches!(self, Self::Operator)
    }
}

#[derive(Debug, Clone, Parser)]
#[command(
    name = "google-search-console-mcp",
    version,
    about = "Rust stdio MCP server for Google Search Console"
)]
pub struct Cli {
    /// Capability profile. Mutating tools require operator.
    #[arg(
        long,
        env = "GOOGLE_SEARCH_CONSOLE_MCP_PROFILE",
        value_enum,
        default_value_t = CapabilityProfile::ReadOnly
    )]
    pub profile: CapabilityProfile,

    /// OAuth scope used for token acquisition.
    #[arg(long, env = "GOOGLE_SEARCH_CONSOLE_MCP_SCOPE", default_value = DEFAULT_SCOPE)]
    pub scope: String,

    /// Base URL for Webmasters v3 endpoints.
    #[arg(
        long,
        env = "GOOGLE_SEARCH_CONSOLE_MCP_API_BASE_URL",
        default_value = DEFAULT_API_BASE_URL
    )]
    pub api_base_url: String,

    /// Base URL for URL Inspection v1 endpoints.
    #[arg(
        long,
        env = "GOOGLE_SEARCH_CONSOLE_MCP_INSPECTION_BASE_URL",
        default_value = DEFAULT_INSPECTION_BASE_URL
    )]
    pub inspection_base_url: String,

    /// HTTP timeout budget in milliseconds.
    #[arg(
        long,
        env = "GOOGLE_SEARCH_CONSOLE_MCP_HTTP_TIMEOUT_MS",
        default_value_t = DEFAULT_HTTP_TIMEOUT_MS
    )]
    pub http_timeout_ms: u64,

    /// User-Agent string applied to outbound Google API requests.
    #[arg(
        long,
        env = "GOOGLE_SEARCH_CONSOLE_MCP_USER_AGENT",
        default_value = DEFAULT_USER_AGENT
    )]
    pub user_agent: String,

    /// Optional path to OAuth client-secret JSON (`installed` or `web`) for refresh-token auth.
    #[arg(long, env = "GOOGLE_SEARCH_CONSOLE_MCP_OAUTH_CLIENT_SECRET_JSON")]
    pub oauth_client_secret_json: Option<String>,

    /// Optional OAuth refresh token used with `GOOGLE_SEARCH_CONSOLE_MCP_OAUTH_CLIENT_SECRET_JSON`.
    #[arg(long, env = "GOOGLE_SEARCH_CONSOLE_MCP_OAUTH_REFRESH_TOKEN")]
    pub oauth_refresh_token: Option<String>,

    /// Optional path to service-account JSON. Standard GOOGLE_APPLICATION_CREDENTIALS also works.
    #[arg(long, env = "GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON_PATH")]
    pub service_account_json_path: Option<String>,

    /// Optional raw service-account JSON for MCP clients that cannot mount files.
    #[arg(long, env = "GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON")]
    pub service_account_json: Option<String>,

    /// Optional quota/billing project for Google APIs (`x-goog-user-project`).
    #[arg(long, env = "GOOGLE_SEARCH_CONSOLE_MCP_QUOTA_PROJECT")]
    pub quota_project: Option<String>,

    /// Maximum allowed Search Analytics rowLimit.
    #[arg(
        long,
        env = "GOOGLE_SEARCH_CONSOLE_MCP_MAX_ROW_LIMIT",
        default_value_t = DEFAULT_MAX_ROW_LIMIT
    )]
    pub max_row_limit: u32,

    /// Print registered tool names and exit.
    #[arg(long)]
    pub print_tools: bool,

    /// Print full tool schema snapshot JSON and exit.
    #[arg(long)]
    pub print_tool_schema: bool,
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub profile: CapabilityProfile,
    pub scope: String,
    pub api_base_url: String,
    pub inspection_base_url: String,
    pub http_timeout: Duration,
    pub user_agent: String,
    pub oauth_client_secret_json: Option<String>,
    pub oauth_refresh_token: Option<String>,
    pub service_account_json_path: Option<String>,
    pub service_account_json: Option<String>,
    pub quota_project: Option<String>,
    pub max_row_limit: u32,
    pub print_tools: bool,
    pub print_tool_schema: bool,
}

impl Settings {
    pub fn from_cli(cli: Cli) -> Result<Self> {
        if cli.http_timeout_ms == 0 {
            return Err(anyhow!("http timeout must be positive"));
        }
        if cli.max_row_limit == 0 || cli.max_row_limit > DEFAULT_MAX_ROW_LIMIT {
            return Err(anyhow!(
                "max row limit must be between 1 and {DEFAULT_MAX_ROW_LIMIT}"
            ));
        }
        Ok(Self {
            profile: cli.profile,
            scope: cli.scope,
            api_base_url: trim_trailing_slash(cli.api_base_url),
            inspection_base_url: trim_trailing_slash(cli.inspection_base_url),
            http_timeout: Duration::from_millis(cli.http_timeout_ms),
            user_agent: cli.user_agent,
            oauth_client_secret_json: cli.oauth_client_secret_json,
            oauth_refresh_token: cli.oauth_refresh_token,
            service_account_json_path: cli.service_account_json_path,
            service_account_json: cli.service_account_json,
            quota_project: cli.quota_project,
            max_row_limit: cli.max_row_limit,
            print_tools: cli.print_tools,
            print_tool_schema: cli.print_tool_schema,
        })
    }
}

fn trim_trailing_slash(value: String) -> String {
    value.trim_end_matches('/').to_string()
}
