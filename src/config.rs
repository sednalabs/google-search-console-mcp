//! CLI and environment-backed configuration.

use std::time::Duration;

use anyhow::{Result, anyhow};
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

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
        global = true,
        value_enum,
        default_value_t = CapabilityProfile::ReadOnly
    )]
    pub profile: CapabilityProfile,

    /// OAuth scope used for token acquisition.
    #[arg(
        long,
        env = "GOOGLE_SEARCH_CONSOLE_MCP_SCOPE",
        global = true,
        default_value = DEFAULT_SCOPE
    )]
    pub scope: String,

    /// Base URL for Webmasters v3 endpoints.
    #[arg(
        long,
        env = "GOOGLE_SEARCH_CONSOLE_MCP_API_BASE_URL",
        global = true,
        default_value = DEFAULT_API_BASE_URL
    )]
    pub api_base_url: String,

    /// Base URL for URL Inspection v1 endpoints.
    #[arg(
        long,
        env = "GOOGLE_SEARCH_CONSOLE_MCP_INSPECTION_BASE_URL",
        global = true,
        default_value = DEFAULT_INSPECTION_BASE_URL
    )]
    pub inspection_base_url: String,

    /// HTTP timeout budget in milliseconds.
    #[arg(
        long,
        env = "GOOGLE_SEARCH_CONSOLE_MCP_HTTP_TIMEOUT_MS",
        global = true,
        default_value_t = DEFAULT_HTTP_TIMEOUT_MS
    )]
    pub http_timeout_ms: u64,

    /// User-Agent string applied to outbound Google API requests.
    #[arg(
        long,
        env = "GOOGLE_SEARCH_CONSOLE_MCP_USER_AGENT",
        global = true,
        default_value = DEFAULT_USER_AGENT
    )]
    pub user_agent: String,

    /// Optional path to OAuth client-secret JSON (`installed` or `web`) for refresh-token auth.
    #[arg(
        long,
        env = "GOOGLE_SEARCH_CONSOLE_MCP_OAUTH_CLIENT_SECRET_JSON",
        global = true
    )]
    pub oauth_client_secret_json: Option<String>,

    /// Optional OAuth refresh token used with `GOOGLE_SEARCH_CONSOLE_MCP_OAUTH_CLIENT_SECRET_JSON`.
    #[arg(
        long,
        env = "GOOGLE_SEARCH_CONSOLE_MCP_OAUTH_REFRESH_TOKEN",
        global = true
    )]
    pub oauth_refresh_token: Option<String>,

    /// Optional path to service-account JSON. Standard GOOGLE_APPLICATION_CREDENTIALS also works.
    #[arg(
        long,
        env = "GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON_PATH",
        global = true
    )]
    pub service_account_json_path: Option<String>,

    /// Optional raw service-account JSON for MCP clients that cannot mount files.
    #[arg(
        long,
        env = "GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON",
        global = true
    )]
    pub service_account_json: Option<String>,

    /// Optional quota/billing project for Google APIs (`x-goog-user-project`).
    #[arg(long, env = "GOOGLE_SEARCH_CONSOLE_MCP_QUOTA_PROJECT", global = true)]
    pub quota_project: Option<String>,

    /// Maximum allowed Search Analytics rowLimit.
    #[arg(
        long,
        env = "GOOGLE_SEARCH_CONSOLE_MCP_MAX_ROW_LIMIT",
        global = true,
        default_value_t = DEFAULT_MAX_ROW_LIMIT
    )]
    pub max_row_limit: u32,

    /// Print registered tool names and exit.
    #[arg(long)]
    pub print_tools: bool,

    /// Print full tool schema snapshot JSON and exit.
    #[arg(long)]
    pub print_tool_schema: bool,

    /// Optional command. Omit to run the stdio MCP server.
    #[command(subcommand)]
    pub command: Option<CliCommand>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum CliCommand {
    /// Run the stdio MCP server. This is also the default when no command is supplied.
    Serve,
    /// Login, verify, and diagnose Google Search Console credentials.
    Auth(AuthCli),
}

#[derive(Debug, Clone, Args)]
pub struct AuthCli {
    #[command(subcommand)]
    pub command: AuthSubcommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum AuthSubcommand {
    /// Run the browser-based gcloud Application Default Credentials login flow.
    Login(AuthLoginArgs),
    /// Print the exact gcloud login command without running it.
    Command(AuthCommandArgs),
    /// Show the configured credential source and optional token verification result.
    Status(AuthStatusCliArgs),
    /// Check the local auth environment and suggest the next action.
    Doctor(AuthDoctorArgs),
}

#[derive(Debug, Clone, Args)]
pub struct AuthLoginArgs {
    /// Request the write-capable Search Console scope needed for operator tools.
    #[arg(long)]
    pub write_scope: bool,

    /// Print a browser URL instead of launching a browser where supported by gcloud.
    #[arg(long)]
    pub headless: bool,

    /// Optional Google OAuth client id file for gcloud ADC login. Recommended for Search Console scopes.
    #[arg(long)]
    pub client_id_file: Option<PathBuf>,

    /// Print the command that would run, without invoking gcloud.
    #[arg(long)]
    pub dry_run: bool,

    /// Skip post-login token acquisition verification.
    #[arg(long)]
    pub no_verify: bool,
}

#[derive(Debug, Clone, Args)]
pub struct AuthCommandArgs {
    /// Request the write-capable Search Console scope needed for operator tools.
    #[arg(long)]
    pub write_scope: bool,

    /// Include the headless browser flag in the printed gcloud command.
    #[arg(long)]
    pub headless: bool,

    /// Optional Google OAuth client id file for gcloud ADC login. Recommended for Search Console scopes.
    #[arg(long)]
    pub client_id_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct AuthStatusCliArgs {
    /// Acquire a Google access token to prove credentials work. The token is never printed.
    #[arg(long)]
    pub verify_token: bool,

    /// Emit machine-readable JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Clone, Args)]
pub struct AuthDoctorArgs {
    /// Acquire a Google access token to prove credentials work. The token is never printed.
    #[arg(long)]
    pub verify_token: bool,

    /// Emit machine-readable JSON.
    #[arg(long)]
    pub json: bool,
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
    pub command: Option<CliCommand>,
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
            command: cli.command,
        })
    }
}

pub fn scope_allows_mutation(scope: &str) -> bool {
    scope_contains(scope, WRITE_SCOPE)
}

pub fn scope_allows_read(scope: &str) -> bool {
    scope_contains(scope, DEFAULT_SCOPE) || scope_allows_mutation(scope)
}

fn scope_contains(scope: &str, expected: &str) -> bool {
    scope
        .split(|ch: char| ch == ',' || ch.is_ascii_whitespace())
        .any(|candidate| candidate == expected)
}

fn trim_trailing_slash(value: String) -> String {
    value.trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_allows_mutation_only_when_write_scope_is_present() {
        assert!(scope_allows_mutation(WRITE_SCOPE));
        assert!(scope_allows_mutation(&format!(
            "{DEFAULT_SCOPE},{WRITE_SCOPE}"
        )));
        assert!(!scope_allows_mutation(DEFAULT_SCOPE));
        assert!(!scope_allows_mutation(
            "https://www.googleapis.com/auth/drive"
        ));
        assert!(scope_allows_read(DEFAULT_SCOPE));
        assert!(scope_allows_read(WRITE_SCOPE));
        assert!(!scope_allows_read("https://www.googleapis.com/auth/drive"));
    }

    #[test]
    fn auth_subcommands_accept_runtime_auth_flags_after_subcommand() {
        let cli = Cli::try_parse_from([
            "google-search-console-mcp",
            "auth",
            "status",
            "--service-account-json-path",
            "/tmp/service-account.json",
            "--json",
        ])
        .expect("auth status should accept runtime auth flags after the subcommand");
        let settings = Settings::from_cli(cli).expect("settings");

        assert_eq!(
            settings.service_account_json_path.as_deref(),
            Some("/tmp/service-account.json")
        );
        assert!(matches!(
            settings.command,
            Some(CliCommand::Auth(AuthCli {
                command: AuthSubcommand::Status(AuthStatusCliArgs { json: true, .. })
            }))
        ));
    }
}
