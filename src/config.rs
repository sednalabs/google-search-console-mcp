//! CLI and environment-backed configuration.

use std::path::PathBuf;
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
const DEFAULT_SCRATCHPAD_SESSION_TTL_SECS: u64 = 900;
const DEFAULT_SCRATCHPAD_MAX_SESSIONS: usize = 64;
const DEFAULT_SCRATCHPAD_MAX_TABLES_PER_SESSION: usize = 32;
const DEFAULT_SCRATCHPAD_MAX_ROWS_PER_SESSION: usize = 1_000_000;
const DEFAULT_SCRATCHPAD_MAX_MEMORY_MB: usize = 256;
const DEFAULT_SCRATCHPAD_QUERY_TIMEOUT_MS: u64 = 15_000;
const DEFAULT_SCRATCHPAD_MAX_SQL_BYTES: usize = 65_536;

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

    /// Scratchpad session TTL in seconds.
    #[arg(
        long,
        env = "GOOGLE_SEARCH_CONSOLE_MCP_SCRATCHPAD_SESSION_TTL_SECS",
        default_value_t = DEFAULT_SCRATCHPAD_SESSION_TTL_SECS
    )]
    pub scratchpad_session_ttl_secs: u64,

    /// Maximum number of active scratchpad sessions.
    #[arg(
        long,
        env = "GOOGLE_SEARCH_CONSOLE_MCP_SCRATCHPAD_MAX_SESSIONS",
        default_value_t = DEFAULT_SCRATCHPAD_MAX_SESSIONS
    )]
    pub scratchpad_max_sessions: usize,

    /// Maximum number of tables tracked per scratchpad session.
    #[arg(
        long,
        env = "GOOGLE_SEARCH_CONSOLE_MCP_SCRATCHPAD_MAX_TABLES_PER_SESSION",
        default_value_t = DEFAULT_SCRATCHPAD_MAX_TABLES_PER_SESSION
    )]
    pub scratchpad_max_tables_per_session: usize,

    /// Maximum number of rows tracked per scratchpad session.
    #[arg(
        long,
        env = "GOOGLE_SEARCH_CONSOLE_MCP_SCRATCHPAD_MAX_ROWS_PER_SESSION",
        default_value_t = DEFAULT_SCRATCHPAD_MAX_ROWS_PER_SESSION
    )]
    pub scratchpad_max_rows_per_session: usize,

    /// Maximum DuckDB memory limit in MB per scratchpad session connection.
    #[arg(
        long,
        env = "GOOGLE_SEARCH_CONSOLE_MCP_SCRATCHPAD_MAX_MEMORY_MB",
        default_value_t = DEFAULT_SCRATCHPAD_MAX_MEMORY_MB
    )]
    pub scratchpad_max_memory_mb: usize,

    /// Scratchpad query timeout in milliseconds.
    #[arg(
        long,
        env = "GOOGLE_SEARCH_CONSOLE_MCP_SCRATCHPAD_QUERY_TIMEOUT_MS",
        default_value_t = DEFAULT_SCRATCHPAD_QUERY_TIMEOUT_MS
    )]
    pub scratchpad_query_timeout_ms: u64,

    /// Maximum SQL payload size accepted by scratchpad query guardrails.
    #[arg(
        long,
        env = "GOOGLE_SEARCH_CONSOLE_MCP_SCRATCHPAD_MAX_SQL_BYTES",
        default_value_t = DEFAULT_SCRATCHPAD_MAX_SQL_BYTES
    )]
    pub scratchpad_max_sql_bytes: usize,

    /// Scratchpad database root directory. Defaults to the OS temp directory.
    #[arg(long, env = "GOOGLE_SEARCH_CONSOLE_MCP_SCRATCHPAD_ROOT_DIR")]
    pub scratchpad_root_dir: Option<PathBuf>,

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
    pub scratchpad_session_ttl: Duration,
    pub scratchpad_max_sessions: usize,
    pub scratchpad_max_tables_per_session: usize,
    pub scratchpad_max_rows_per_session: usize,
    pub scratchpad_max_memory_mb: usize,
    pub scratchpad_query_timeout: Duration,
    pub scratchpad_max_sql_bytes: usize,
    pub scratchpad_root_dir: PathBuf,
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
        if cli.scratchpad_session_ttl_secs == 0 {
            return Err(anyhow!("scratchpad session ttl must be greater than zero"));
        }
        if cli.scratchpad_max_sessions == 0 {
            return Err(anyhow!("scratchpad max sessions must be greater than zero"));
        }
        if cli.scratchpad_max_tables_per_session == 0 {
            return Err(anyhow!(
                "scratchpad max tables per session must be greater than zero"
            ));
        }
        if cli.scratchpad_max_rows_per_session == 0 {
            return Err(anyhow!(
                "scratchpad max rows per session must be greater than zero"
            ));
        }
        if cli.scratchpad_max_memory_mb == 0 {
            return Err(anyhow!("scratchpad max memory mb must be greater than zero"));
        }
        if cli.scratchpad_query_timeout_ms == 0 {
            return Err(anyhow!("scratchpad query timeout must be greater than zero"));
        }
        if cli.scratchpad_max_sql_bytes == 0 {
            return Err(anyhow!("scratchpad max sql bytes must be greater than zero"));
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
            scratchpad_session_ttl: Duration::from_secs(cli.scratchpad_session_ttl_secs),
            scratchpad_max_sessions: cli.scratchpad_max_sessions,
            scratchpad_max_tables_per_session: cli.scratchpad_max_tables_per_session,
            scratchpad_max_rows_per_session: cli.scratchpad_max_rows_per_session,
            scratchpad_max_memory_mb: cli.scratchpad_max_memory_mb,
            scratchpad_query_timeout: Duration::from_millis(cli.scratchpad_query_timeout_ms),
            scratchpad_max_sql_bytes: cli.scratchpad_max_sql_bytes,
            scratchpad_root_dir: cli.scratchpad_root_dir.unwrap_or_else(std::env::temp_dir),
            print_tools: cli.print_tools,
            print_tool_schema: cli.print_tool_schema,
        })
    }
}

fn trim_trailing_slash(value: String) -> String {
    value.trim_end_matches('/').to_string()
}
