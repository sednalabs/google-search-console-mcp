use std::path::Path;
use std::time::Instant;

use mcp_toolkit_auth::provider_auth::{
    GoogleProviderAuthConfig, GoogleProviderAuthSetupPlan, format_provider_auth_command,
    google_adc_quota_project_command,
};
use mcp_toolkit_core::tool_inventory::{ToolOperation, ToolSearchFilter, ToolSearchResponse};
use mcp_toolkit_core::tool_schema::tool_schema_snapshot_value;
use mcp_toolkit_scratchpad::{
    ScratchpadIngestColumn, ScratchpadIngestMode, ScratchpadQueryProjection, ScratchpadSessionInfo,
    ScratchpadSessionSnapshot, ScratchpadTableInfo,
};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::tool;
use rmcp::tool_router;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::config::{
    CapabilityProfile, DEFAULT_SCOPE, WRITE_SCOPE, conventional_adc_credentials_path,
    server_adc_credentials_path, server_cloudsdk_config_dir,
};
use crate::contract;
use crate::error::SearchConsoleError;
use crate::gsc_client::{OperatorScopeCheck, SearchAnalyticsRequest};
use crate::server::{SearchConsoleMcp, tool_inventory_policy_for_profile};
use crate::tool_surface::build_tool_inventory;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FindToolsArgs {
    /// Keyword query matched against tool names, descriptions, and keywords.
    #[serde(default)]
    pub query: Option<String>,
    /// Optional group filter such as sites, search_analytics, url_inspection, or sitemaps.
    #[serde(default)]
    pub group: Option<String>,
    /// Optional read-only filter.
    #[serde(default)]
    pub read_only: Option<bool>,
    /// Maximum result count, 1..100.
    #[serde(default)]
    pub limit: Option<usize>,
    /// Include matching MCP tool schemas in the response.
    #[serde(default)]
    pub include_schema: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SiteUrlArgs {
    /// Search Console property URL, for example `https://www.example.com/` or `sc-domain:example.com`.
    pub site_url: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AuthStatusArgs {
    /// When true, acquire a Google access token to prove credentials work. The token is never returned.
    #[serde(default)]
    pub verify_token: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AuthLoginCommandArgs {
    /// Use the write-capable Search Console scope needed for operator sitemap/site mutations.
    #[serde(default)]
    pub write_scope: bool,
    /// Include the headless browser flag for SSH or remote environments.
    #[serde(default)]
    pub headless: bool,
    /// Optional Google OAuth client id file for gcloud ADC login.
    #[serde(default)]
    pub client_id_file: Option<String>,
    /// Optional quota project to include as a follow-up command.
    #[serde(default)]
    pub quota_project: Option<String>,
    /// Use the conventional shared gcloud ADC file instead of the server-specific file.
    #[serde(default)]
    pub shared_adc: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchAnalyticsQueryArgs {
    /// Search Console property URL, for example `https://www.example.com/` or `sc-domain:example.com`.
    pub site_url: String,
    /// Start date in YYYY-MM-DD format, in Search Console's Pacific Time reporting calendar.
    pub start_date: String,
    /// End date in YYYY-MM-DD format, in Search Console's Pacific Time reporting calendar.
    pub end_date: String,
    /// Optional dimensions such as query, page, country, device, date, hour, or searchAppearance.
    /// Compatibility depends on search_type and data_state; for example googleNews/discover do not
    /// support query, and hourly_all uses hour without date.
    #[serde(default)]
    pub dimensions: Vec<String>,
    /// Optional Search Console type: web, image, video, news, googleNews, or discover.
    /// Some dimensions are search-type specific.
    #[serde(default)]
    pub search_type: Option<String>,
    /// Optional official dimensionFilterGroups structure. snake_case keys are converted to camelCase.
    #[serde(default)]
    pub dimension_filter_groups: Option<Value>,
    /// Optional aggregation type: auto, byPage, byProperty, or byNewsShowcasePanel.
    #[serde(default)]
    pub aggregation_type: Option<String>,
    /// Maximum rows to return, 1..25,000 by default configuration.
    #[serde(default)]
    pub row_limit: Option<u32>,
    /// Zero-based first row offset for paging.
    #[serde(default)]
    pub start_row: Option<u32>,
    /// Optional data state: final, all, or hourly_all. hourly_all requires the hour dimension.
    #[serde(default)]
    pub data_state: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UrlInspectionArgs {
    /// Search Console property URL, for example `https://www.example.com/` or `sc-domain:example.com`.
    pub site_url: String,
    /// Fully-qualified URL to inspect. Must be under the property specified by `site_url`.
    pub inspection_url: String,
    /// Optional IETF BCP-47 language code for translated issue messages.
    #[serde(default)]
    pub language_code: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SitemapsListArgs {
    /// Search Console property URL, for example `https://www.example.com/` or `sc-domain:example.com`.
    pub site_url: String,
    /// Optional sitemap index URL used to filter child sitemap entries.
    #[serde(default)]
    pub sitemap_index: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SitemapArgs {
    /// Search Console property URL, for example `https://www.example.com/` or `sc-domain:example.com`.
    pub site_url: String,
    /// Sitemap URL.
    pub feed_path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScratchpadSessionArgs {
    /// Scratchpad session identifier. Use stable names such as seo_evidence_2026_06_18.
    pub session_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScratchpadInventoryArgs {
    /// Maximum sessions to return.
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScratchpadTableInventoryArgs {
    /// Scratchpad session identifier.
    pub session_id: String,
    /// Maximum tables to return.
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScratchpadDropTableArgs {
    /// Scratchpad session identifier.
    pub session_id: String,
    /// Scratchpad table name to drop.
    pub table_name: String,
    /// Return ok when the table is already absent.
    #[serde(default)]
    pub if_exists: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScratchpadQueryArgs {
    /// Scratchpad session identifier.
    pub session_id: String,
    /// Read-only DuckDB SQL. SELECT, WITH, DESCRIBE, SUMMARIZE, and EXPLAIN are allowed.
    pub sql: String,
    /// Zero-based row offset.
    #[serde(default)]
    pub offset: Option<u64>,
    /// Maximum rows to return from this query page.
    #[serde(default)]
    pub page_size: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScratchpadIngestSearchAnalyticsArgs {
    /// Scratchpad session identifier.
    pub session_id: String,
    /// Scratchpad table name to create or append to.
    pub table_name: String,
    /// Append rows into an existing table instead of creating a new table.
    #[serde(default)]
    pub append: bool,
    /// Search Console property URL, for example `https://www.example.com/` or `sc-domain:example.com`.
    pub site_url: String,
    /// Start date in YYYY-MM-DD format, in Search Console's Pacific Time reporting calendar.
    pub start_date: String,
    /// End date in YYYY-MM-DD format, in Search Console's Pacific Time reporting calendar.
    pub end_date: String,
    /// Optional dimensions such as query, page, country, device, date, hour, or searchAppearance.
    #[serde(default)]
    pub dimensions: Vec<String>,
    /// Optional Search Console type: web, image, video, news, googleNews, or discover.
    #[serde(default)]
    pub search_type: Option<String>,
    /// Optional official dimensionFilterGroups structure. snake_case keys are converted to camelCase.
    #[serde(default)]
    pub dimension_filter_groups: Option<Value>,
    /// Optional aggregation type: auto, byPage, byProperty, or byNewsShowcasePanel.
    #[serde(default)]
    pub aggregation_type: Option<String>,
    /// Maximum rows to fetch, 1..25,000 by default configuration.
    #[serde(default)]
    pub row_limit: Option<u32>,
    /// Zero-based first row offset for upstream paging.
    #[serde(default)]
    pub start_row: Option<u32>,
    /// Optional data state: final, all, or hourly_all.
    #[serde(default)]
    pub data_state: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScratchpadEvidenceBundleArgs {
    /// Scratchpad session identifier.
    pub session_id: String,
    /// Optional table subset. When omitted, all visible scratchpad tables are included.
    #[serde(default)]
    pub tables: Option<Vec<String>>,
    /// Number of sample rows to include per table.
    #[serde(default)]
    pub sample_rows_per_table: Option<u64>,
}

pub fn registered_tool_names(profile: CapabilityProfile) -> Vec<String> {
    registered_tools(profile)
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect()
}

pub fn registered_tool_schema_snapshot(profile: CapabilityProfile) -> serde_json::Value {
    tool_schema_snapshot_value(&registered_tools(profile))
        .expect("registered tool definitions should serialize")
}

fn registered_tools(profile: CapabilityProfile) -> Vec<rmcp::model::Tool> {
    let inventory = build_tool_inventory().expect("google-search-console-mcp inventory");
    let policy = tool_inventory_policy_for_profile(profile);
    inventory.filter_tools(
        SearchConsoleMcp::tool_router_search_console().list_all(),
        ToolOperation::List,
        &policy,
        |tool| tool.name.as_ref(),
    )
}

fn redact_tool_error_message(err: &impl std::fmt::Display) -> String {
    contract::redact_secret_text(&err.to_string())
}

#[tool_router(router = tool_router_search_console, vis = "pub")]
impl SearchConsoleMcp {
    /// Search tools for OpenAI tool_search and deferred-loading clients.
    #[tool(
        name = "find_tools",
        description = "Search Google Search Console MCP tools by keyword, group, and read-only status for tool_search/deferred-loading clients."
    )]
    async fn find_tools(
        &self,
        Parameters(args): Parameters<FindToolsArgs>,
    ) -> Result<CallToolResult, crate::McpError> {
        let started = Instant::now();
        let limit = args.limit.unwrap_or(20).clamp(1, 100);
        let filter = ToolSearchFilter {
            query: args.query.clone(),
            group: args.group.clone(),
            read_only: args.read_only,
            limit: Some(limit),
        };
        let results =
            self.tool_inventory
                .search(&filter, ToolOperation::List, &self.tool_inventory_policy);
        let schemas = if args.include_schema {
            let tools = self.visible_tools();
            let mut schema_map = serde_json::Map::new();
            for result in &results {
                if let Some(tool) = tools.iter().find(|tool| tool.name.as_ref() == result.name) {
                    schema_map.insert(result.name.clone(), json!(tool));
                }
            }
            Some(Value::Object(schema_map))
        } else {
            None
        };
        let response =
            ToolSearchResponse::find_tools(args.query, args.group, args.read_only, results)
                .with_schemas(schemas)
                .with_metadata_label("Google Search Console MCP tool_search metadata");

        Ok(contract::success(response.to_value(), started))
    }

    /// Return the recommended first-run path.
    #[tool(
        name = "gsc_get_started",
        description = "Return the recommended first-run flow, credential options, and safe starter tools."
    )]
    async fn gsc_get_started(&self) -> Result<CallToolResult, crate::McpError> {
        let started = Instant::now();
        Ok(contract::success(
            json!({
                "server": "google-search-console-mcp",
                "profile": self.profile.as_str(),
                "auth_source": self.client.auth_source().as_str(),
                "scope": self.client.scope(),
                "first_steps": [
                    "Call gsc_auth_status with verify_token=false to inspect configuration without making a token request.",
                    "If no credentials are configured, call gsc_auth_login_command and run the returned gcloud command.",
                    "Call gsc_auth_status with verify_token=true to prove Google auth without returning a token.",
                    "Call gsc_sites_list to discover the exact Search Console property string.",
                    "Use the exact siteUrl from gsc_sites_list when querying analytics, URL inspection, or sitemaps."
                ],
                "credential_options": [
                    {
                        "name": "Application Default Credentials",
                        "best_for": "lowest-friction local use",
                        "env": []
                    },
                    {
                        "name": "GOOGLE_APPLICATION_CREDENTIALS",
                        "best_for": "standard service-account file configuration",
                        "env": ["GOOGLE_APPLICATION_CREDENTIALS"]
                    },
                    {
                        "name": "server-specific service account path",
                        "best_for": "MCP client configs that should not rely on global Google env",
                        "env": ["GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON_PATH"]
                    },
                    {
                        "name": "server-specific service account JSON",
                        "best_for": "hosted or sealed-secret deployments that cannot mount a file",
                        "env": ["GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON"]
                    }
                ],
                "safe_starter_tools": [
                    "gsc_sites_list",
                    "gsc_search_analytics_query",
                    "gsc_scratchpad_open_session",
                    "gsc_scratchpad_ingest_search_analytics",
                    "gsc_scratchpad_query",
                    "gsc_url_inspection_index_inspect",
                    "gsc_sitemaps_list"
                ],
                "evidence_workflow": [
                    "Open a scratchpad session for a focused investigation.",
                    "Ingest Search Analytics rows into a named table.",
                    "Use gsc_scratchpad_query for bounded read-only DuckDB analysis.",
                    "Use gsc_scratchpad_export_evidence_bundle for compact markdown evidence."
                ],
                "operator_note": "Mutation tools are hidden behind the operator profile and require the webmasters write scope."
            }),
            started,
        ))
    }

    /// Explain configured auth without exposing secrets.
    #[tool(
        name = "gsc_auth_status",
        description = "Explain configured Google auth source and optionally verify token acquisition without returning secrets."
    )]
    async fn gsc_auth_status(
        &self,
        Parameters(args): Parameters<AuthStatusArgs>,
    ) -> Result<CallToolResult, crate::McpError> {
        let started = Instant::now();
        let token_check = if args.verify_token {
            match self.client.verify_token().await {
                Ok(()) => json!({ "checked": true, "ok": true }),
                Err(err) => json!({
                    "checked": true,
                    "ok": false,
                    "error": redact_tool_error_message(&err)
                }),
            }
        } else {
            json!({ "checked": false })
        };
        let token_ok = token_check.get("ok").and_then(Value::as_bool);
        let operator_scope_relevant =
            self.profile.allows_mutation() || self.client.scope() == WRITE_SCOPE;
        let should_check_operator_scope =
            operator_scope_relevant && args.verify_token && token_ok == Some(true);
        let operator_scope_check = if should_check_operator_scope {
            match self.client.verify_operator_scope().await {
                Ok(check) => operator_scope_check_to_json(check),
                Err(err) => json!({
                    "checked": true,
                    "ok": false,
                    "required_scope": WRITE_SCOPE,
                    "error": redact_tool_error_message(&err),
                }),
            }
        } else if operator_scope_relevant && args.verify_token {
            json!({
                "checked": true,
                "ok": false,
                "required_scope": WRITE_SCOPE,
                "error": "skipped because token verification failed",
            })
        } else {
            json!({
                "checked": false,
                "required_scope": WRITE_SCOPE,
                "reason": if operator_scope_relevant {
                    "set verify_token=true to prove operator write-scope readiness"
                } else {
                    "not using operator profile or write scope"
                },
            })
        };
        let operator_scope_ok = operator_scope_check.get("ok").and_then(Value::as_bool);

        Ok(contract::success(
            json!({
                "auth_source": self.client.auth_source().as_str(),
                "scope": self.client.scope(),
                "profile": self.profile.as_str(),
                "operator_tools_enabled": self.profile.allows_mutation(),
                "quota_project_configured": self.client.quota_project_configured(),
                "adc_file": selected_adc_file_status(),
                "detected_env": auth_env_presence(),
                "token_check": token_check,
                "operator_scope_check": operator_scope_check,
                "next_steps": auth_next_steps(
                    args.verify_token,
                    token_ok,
                    operator_scope_relevant,
                    operator_scope_ok,
                    self.client.scope(),
                ),
                "secrets_returned": false
            }),
            started,
        ))
    }

    /// Return a gcloud ADC login command.
    #[tool(
        name = "gsc_auth_login_command",
        description = "Return a copyable gcloud Application Default Credentials login command for Search Console."
    )]
    async fn gsc_auth_login_command(
        &self,
        Parameters(args): Parameters<AuthLoginCommandArgs>,
    ) -> Result<CallToolResult, crate::McpError> {
        let started = Instant::now();
        let scope = if args.write_scope {
            WRITE_SCOPE
        } else {
            self.client.scope()
        };
        let shared_adc = args.shared_adc.unwrap_or(false);
        let cloudsdk_config = if shared_adc {
            None
        } else {
            server_cloudsdk_config_dir()
        };
        if !shared_adc && cloudsdk_config.is_none() {
            return Ok(contract::error(
                SearchConsoleError::invalid(
                    "shared_adc",
                    "failed to determine the server-specific gcloud config directory; set HOME/XDG_CONFIG_HOME on Unix or APPDATA on Windows, or pass shared_adc=true to intentionally use conventional shared ADC",
                ),
                started,
            ));
        }
        let credential_file = if shared_adc {
            conventional_adc_credentials_path()
        } else {
            server_adc_credentials_path()
        };
        let command = gcloud_adc_login_command(
            scope,
            args.client_id_file.as_deref().map(Path::new),
            args.headless,
        );
        let headless_command =
            gcloud_adc_login_command(scope, args.client_id_file.as_deref().map(Path::new), true);
        let setup_plan = gsc_auth_setup_plan(scope);
        let quota_project = args.quota_project.as_deref();
        let follow_up_commands = quota_project
            .map(|project| {
                vec![shell_join_with_cloudsdk_config(
                    &gcloud_set_quota_project_command(project),
                    cloudsdk_config.as_deref(),
                )]
            })
            .unwrap_or_default();
        Ok(contract::success(
            json!({
                "command": shell_join_with_cloudsdk_config(&command, cloudsdk_config.as_deref()),
                "headless_command": shell_join_with_cloudsdk_config(&headless_command, cloudsdk_config.as_deref()),
                "client_id_file_command": shell_join_with_cloudsdk_config(
                    &gcloud_adc_login_command(scope, Some(Path::new("/path/to/client_id.json")), args.headless),
                    cloudsdk_config.as_deref(),
                ),
                "client_id_file_headless_command": shell_join_with_cloudsdk_config(
                    &gcloud_adc_login_command(scope, Some(Path::new("/path/to/client_id.json")), true),
                    cloudsdk_config.as_deref(),
                ),
                "quota_project_command": shell_join_with_cloudsdk_config(
                    &gcloud_set_quota_project_command("YOUR_PROJECT"),
                    cloudsdk_config.as_deref(),
                ),
                "api_enable_command": setup_plan.api_enable.as_ref().map(|command| command.shell.as_str()),
                "follow_up_commands": follow_up_commands,
                "adc_scopes": setup_plan.scopes.clone(),
                "cloudsdk_config": cloudsdk_config.as_ref().map(|path| path.display().to_string()),
                "credential_file": credential_file.as_ref().map(|path| path.display().to_string()),
                "shared_adc": shared_adc,
                "scope": scope,
                "write_scope": args.write_scope,
                "headless": args.headless,
                "client_id_file": args.client_id_file,
                "quota_project": args.quota_project,
                "next_steps": setup_plan.next_steps.clone(),
                "notes": [
                    "By default this command writes a Google Search Console-specific ADC file for this OS user.",
                    "Set shared_adc=true only when you intentionally want the conventional shared gcloud ADC file; set GOOGLE_SEARCH_CONSOLE_MCP_SHARED_ADC=true when the runtime should use it.",
                    "No token or client secret is returned by this tool.",
                    "Use write_scope=true only when preparing credentials for operator sitemap/site mutations."
                ],
                "after_login": "Restart stdio MCP clients that keep long-lived server processes, then call gsc_auth_status with verify_token=true.",
                "service_account_alternative": {
                    "standard_env": "GOOGLE_APPLICATION_CREDENTIALS=/path/to/service-account.json",
                    "server_specific_path_env": "GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON_PATH=/path/to/service-account.json"
                }
            }),
            started,
        ))
    }

    /// List Search Console properties visible to the authenticated account.
    #[tool(
        name = "gsc_sites_list",
        description = "List Search Console properties visible to the authenticated Google account."
    )]
    async fn gsc_sites_list(&self) -> Result<CallToolResult, crate::McpError> {
        let started = Instant::now();
        match self.client.list_sites().await {
            Ok(value) => Ok(contract::success(value, started)),
            Err(err) => Ok(contract::error(err, started)),
        }
    }

    /// Get permission metadata for one Search Console property.
    #[tool(
        name = "gsc_site_get",
        description = "Get permission metadata for one Search Console property by exact site_url."
    )]
    async fn gsc_site_get(
        &self,
        Parameters(args): Parameters<SiteUrlArgs>,
    ) -> Result<CallToolResult, crate::McpError> {
        let started = Instant::now();
        match self.client.get_site(&args.site_url).await {
            Ok(value) => Ok(contract::success(value, started)),
            Err(err) => Ok(contract::error(err, started)),
        }
    }

    /// Query Search Console performance rows.
    #[tool(
        name = "gsc_search_analytics_query",
        description = "Query Search Console Search Analytics performance rows for a property and date range."
    )]
    async fn gsc_search_analytics_query(
        &self,
        Parameters(args): Parameters<SearchAnalyticsQueryArgs>,
    ) -> Result<CallToolResult, crate::McpError> {
        let started = Instant::now();
        let request = SearchAnalyticsRequest {
            site_url: args.site_url,
            start_date: args.start_date,
            end_date: args.end_date,
            dimensions: args.dimensions,
            search_type: args.search_type,
            dimension_filter_groups: args.dimension_filter_groups,
            aggregation_type: args.aggregation_type,
            row_limit: args.row_limit,
            start_row: args.start_row,
            data_state: args.data_state,
        };
        match self.client.search_analytics_query(request).await {
            Ok(value) => Ok(contract::success(value, started)),
            Err(err) => Ok(contract::error(err, started)),
        }
    }

    /// Open or refresh a scratchpad session.
    #[tool(
        name = "gsc_scratchpad_open_session",
        description = "Open or refresh a bounded DuckDB scratchpad session for Search Console evidence work."
    )]
    async fn gsc_scratchpad_open_session(
        &self,
        Parameters(args): Parameters<ScratchpadSessionArgs>,
    ) -> Result<CallToolResult, crate::McpError> {
        let started = Instant::now();
        match self.scratchpad_sessions.open_session(&args.session_id) {
            Ok(info) => Ok(contract::success(
                scratchpad_session_info_to_json(info),
                started,
            )),
            Err(err) => Ok(contract::scratchpad_error(err, started)),
        }
    }

    /// Close a scratchpad session and remove its local database.
    #[tool(
        name = "gsc_scratchpad_close_session",
        description = "Close a Search Console scratchpad session and remove its local DuckDB database."
    )]
    async fn gsc_scratchpad_close_session(
        &self,
        Parameters(args): Parameters<ScratchpadSessionArgs>,
    ) -> Result<CallToolResult, crate::McpError> {
        let started = Instant::now();
        match self.scratchpad_sessions.release_session(&args.session_id) {
            Ok(released) => Ok(contract::success(
                json!({
                    "session_id": args.session_id,
                    "released": released,
                }),
                started,
            )),
            Err(err) => Ok(contract::scratchpad_error(err, started)),
        }
    }

    /// List active scratchpad sessions.
    #[tool(
        name = "gsc_scratchpad_list_sessions",
        description = "List active Search Console scratchpad sessions."
    )]
    async fn gsc_scratchpad_list_sessions(
        &self,
        Parameters(args): Parameters<ScratchpadInventoryArgs>,
    ) -> Result<CallToolResult, crate::McpError> {
        let started = Instant::now();
        let limit = args.limit.unwrap_or(20).clamp(1, 100);
        match self.scratchpad_sessions.list_sessions(limit) {
            Ok(sessions) => Ok(contract::success(
                json!({
                    "sessions": sessions
                        .into_iter()
                        .map(scratchpad_session_info_to_json)
                        .collect::<Vec<_>>(),
                    "limit": limit,
                }),
                started,
            )),
            Err(err) => Ok(contract::scratchpad_error(err, started)),
        }
    }

    /// List scratchpad tables in a session.
    #[tool(
        name = "gsc_scratchpad_list_tables",
        description = "List tables in a Search Console scratchpad session."
    )]
    async fn gsc_scratchpad_list_tables(
        &self,
        Parameters(args): Parameters<ScratchpadTableInventoryArgs>,
    ) -> Result<CallToolResult, crate::McpError> {
        let started = Instant::now();
        let limit = args.limit.unwrap_or(50).clamp(1, 200);
        match self
            .scratchpad_sessions
            .list_tables(&args.session_id, limit)
        {
            Ok(tables) => Ok(contract::success(
                json!({
                    "session_id": args.session_id,
                    "tables": tables
                        .into_iter()
                        .map(scratchpad_table_info_to_json)
                        .collect::<Vec<_>>(),
                    "limit": limit,
                }),
                started,
            )),
            Err(err) => Ok(contract::scratchpad_error(err, started)),
        }
    }

    /// Drop a table from a scratchpad session.
    #[tool(
        name = "gsc_scratchpad_drop_table",
        description = "Drop one table from a Search Console scratchpad session."
    )]
    async fn gsc_scratchpad_drop_table(
        &self,
        Parameters(args): Parameters<ScratchpadDropTableArgs>,
    ) -> Result<CallToolResult, crate::McpError> {
        let started = Instant::now();
        match self.scratchpad_sessions.drop_table(
            &args.session_id,
            &args.table_name,
            args.if_exists,
        ) {
            Ok(stats) => Ok(contract::success(
                json!({
                    "session_id": args.session_id,
                    "table_name": args.table_name,
                    "dropped": stats.dropped,
                    "rows_removed": stats.rows_removed,
                    "session": scratchpad_snapshot_to_json(stats.session_snapshot),
                }),
                started,
            )),
            Err(err) => Ok(contract::scratchpad_error(err, started)),
        }
    }

    /// Query a scratchpad session with guarded SQL.
    #[tool(
        name = "gsc_scratchpad_query",
        description = "Run bounded read-only DuckDB SQL against a Search Console scratchpad session."
    )]
    async fn gsc_scratchpad_query(
        &self,
        Parameters(args): Parameters<ScratchpadQueryArgs>,
    ) -> Result<CallToolResult, crate::McpError> {
        let started = Instant::now();
        let offset = args.offset.unwrap_or(0);
        let page_size = args.page_size.unwrap_or(100).clamp(1, 1_000);
        match self
            .scratchpad_sessions
            .query_rows(&args.session_id, &args.sql, offset, page_size)
        {
            Ok(projection) => Ok(contract::success(
                scratchpad_query_projection_to_json(projection, offset, page_size),
                started,
            )),
            Err(err) => Ok(contract::scratchpad_error(err, started)),
        }
    }

    /// Fetch Search Analytics rows and ingest them into a scratchpad table.
    #[tool(
        name = "gsc_scratchpad_ingest_search_analytics",
        description = "Fetch Search Analytics rows and ingest them into a Search Console scratchpad table."
    )]
    async fn gsc_scratchpad_ingest_search_analytics(
        &self,
        Parameters(args): Parameters<ScratchpadIngestSearchAnalyticsArgs>,
    ) -> Result<CallToolResult, crate::McpError> {
        let started = Instant::now();
        let dimensions = args
            .dimensions
            .iter()
            .map(|dimension| dimension.trim().to_string())
            .collect::<Vec<_>>();
        let request = SearchAnalyticsRequest {
            site_url: args.site_url,
            start_date: args.start_date,
            end_date: args.end_date,
            dimensions: dimensions.clone(),
            search_type: args.search_type,
            dimension_filter_groups: args.dimension_filter_groups,
            aggregation_type: args.aggregation_type,
            row_limit: args.row_limit,
            start_row: args.start_row,
            data_state: args.data_state,
        };
        let upstream = match self.client.search_analytics_query(request).await {
            Ok(value) => value,
            Err(err) => return Ok(contract::error(err, started)),
        };
        let columns = search_analytics_ingest_columns(&dimensions);
        let rows = search_analytics_rows_for_scratchpad(&upstream, &dimensions);
        let ingest_mode = if args.append {
            ScratchpadIngestMode::Append
        } else {
            ScratchpadIngestMode::Create
        };
        match self.scratchpad_sessions.ingest_rows_with_mode(
            &args.session_id,
            &args.table_name,
            &columns,
            &rows,
            ingest_mode,
        ) {
            Ok(stats) => Ok(contract::success(
                json!({
                    "session_id": args.session_id,
                    "table_name": args.table_name,
                    "mode": ingest_mode_label(ingest_mode),
                    "rows_inserted": stats.rows_inserted,
                    "columns_inserted": stats.columns_inserted,
                    "columns": columns
                        .into_iter()
                        .map(|column| json!({
                            "name": column.name,
                            "logical_type": column.logical_type,
                        }))
                        .collect::<Vec<_>>(),
                    "session": scratchpad_snapshot_to_json(stats.session_snapshot),
                    "upstream_summary": {
                        "row_count": rows.len(),
                        "dimensions": dimensions,
                    },
                }),
                started,
            )),
            Err(err) => Ok(contract::scratchpad_error(err, started)),
        }
    }

    /// Export a bounded markdown evidence bundle from scratchpad tables.
    #[tool(
        name = "gsc_scratchpad_export_evidence_bundle",
        description = "Export a bounded markdown evidence bundle from Search Console scratchpad tables."
    )]
    async fn gsc_scratchpad_export_evidence_bundle(
        &self,
        Parameters(args): Parameters<ScratchpadEvidenceBundleArgs>,
    ) -> Result<CallToolResult, crate::McpError> {
        let started = Instant::now();
        let sample_rows = args.sample_rows_per_table.unwrap_or(10).clamp(1, 100);
        let table_names = match args.tables {
            Some(tables) => tables,
            None => match self.scratchpad_sessions.list_tables(&args.session_id, 100) {
                Ok(tables) => tables.into_iter().map(|table| table.name).collect(),
                Err(err) => return Ok(contract::scratchpad_error(err, started)),
            },
        };

        let mut bundle = format!(
            "# Search Console Scratchpad Evidence Bundle\n\n- Session: `{}`\n- Tables: `{}`\n- Sample rows per table: `{}`\n\n",
            args.session_id,
            table_names.len(),
            sample_rows,
        );
        let mut summaries = Vec::new();
        for table_name in table_names {
            let quoted = quote_scratchpad_ident(&table_name);
            let count_sql = format!("SELECT COUNT(*) AS row_count FROM {quoted}");
            let count_projection =
                match self
                    .scratchpad_sessions
                    .query_rows(&args.session_id, &count_sql, 0, 1)
                {
                    Ok(projection) => projection,
                    Err(err) => {
                        append_evidence_table_error(&mut bundle, &table_name, &err);
                        summaries.push(json!({
                            "table_name": table_name,
                            "error": err.to_string(),
                        }));
                        continue;
                    }
                };
            let row_count = count_projection
                .rows
                .first()
                .and_then(|row| row.get("row_count"))
                .and_then(json_u64)
                .unwrap_or(0);
            let sample_sql = format!("SELECT * FROM {quoted}");
            let sample_projection = match self.scratchpad_sessions.query_rows(
                &args.session_id,
                &sample_sql,
                0,
                sample_rows,
            ) {
                Ok(projection) => projection,
                Err(err) => {
                    append_evidence_table_error(&mut bundle, &table_name, &err);
                    summaries.push(json!({
                        "table_name": table_name,
                        "row_count": row_count,
                        "error": err.to_string(),
                    }));
                    continue;
                }
            };

            bundle.push_str(&format!("## `{table_name}`\n\n"));
            bundle.push_str(&format!("- Rows: `{row_count}`\n"));
            bundle.push_str(&format!(
                "- Columns: `{}`\n\n",
                sample_projection.columns.len()
            ));
            bundle.push_str(&markdown_table(&sample_projection));
            bundle.push('\n');
            summaries.push(json!({
                "table_name": table_name,
                "row_count": row_count,
                "sample_rows": sample_projection.rows.len(),
                "columns": sample_projection.columns
                    .into_iter()
                    .map(|column| json!({
                        "name": column.name,
                        "logical_type": column.logical_type,
                        "nullable": column.nullable,
                    }))
                    .collect::<Vec<_>>(),
            }));
        }

        Ok(contract::success(
            json!({
                "session_id": args.session_id,
                "format": "markdown",
                "bundle": bundle,
                "tables": summaries,
            }),
            started,
        ))
    }

    /// Inspect Google-indexed URL status.
    #[tool(
        name = "gsc_url_inspection_index_inspect",
        description = "Inspect Google-indexed status for a URL under a Search Console property."
    )]
    async fn gsc_url_inspection_index_inspect(
        &self,
        Parameters(args): Parameters<UrlInspectionArgs>,
    ) -> Result<CallToolResult, crate::McpError> {
        let started = Instant::now();
        match self
            .client
            .inspect_url(&args.site_url, &args.inspection_url, args.language_code)
            .await
        {
            Ok(value) => Ok(contract::success(value, started)),
            Err(err) => Ok(contract::error(err, started)),
        }
    }

    /// List submitted sitemaps.
    #[tool(
        name = "gsc_sitemaps_list",
        description = "List submitted sitemaps for a Search Console property."
    )]
    async fn gsc_sitemaps_list(
        &self,
        Parameters(args): Parameters<SitemapsListArgs>,
    ) -> Result<CallToolResult, crate::McpError> {
        let started = Instant::now();
        match self
            .client
            .list_sitemaps(&args.site_url, args.sitemap_index)
            .await
        {
            Ok(value) => Ok(contract::success(value, started)),
            Err(err) => Ok(contract::error(err, started)),
        }
    }

    /// Get one submitted sitemap.
    #[tool(
        name = "gsc_sitemap_get",
        description = "Get metadata for one submitted sitemap by exact sitemap URL."
    )]
    async fn gsc_sitemap_get(
        &self,
        Parameters(args): Parameters<SitemapArgs>,
    ) -> Result<CallToolResult, crate::McpError> {
        let started = Instant::now();
        match self
            .client
            .get_sitemap(&args.site_url, &args.feed_path)
            .await
        {
            Ok(value) => Ok(contract::success(value, started)),
            Err(err) => Ok(contract::error(err, started)),
        }
    }

    /// Submit a sitemap URL.
    #[tool(
        name = "gsc_sitemap_submit",
        description = "Submit a sitemap URL to Google Search Console. Requires operator profile and webmasters scope."
    )]
    async fn gsc_sitemap_submit(
        &self,
        Parameters(args): Parameters<SitemapArgs>,
    ) -> Result<CallToolResult, crate::McpError> {
        let started = Instant::now();
        match self
            .client
            .submit_sitemap(&args.site_url, &args.feed_path)
            .await
        {
            Ok(value) => Ok(contract::success_with_meta(
                value,
                json!({ "mutation": true }),
                started,
            )),
            Err(err) => Ok(contract::error(err, started)),
        }
    }

    /// Delete a sitemap URL.
    #[tool(
        name = "gsc_sitemap_delete",
        description = "Delete a sitemap from Google Search Console. Requires operator profile and webmasters scope."
    )]
    async fn gsc_sitemap_delete(
        &self,
        Parameters(args): Parameters<SitemapArgs>,
    ) -> Result<CallToolResult, crate::McpError> {
        let started = Instant::now();
        match self
            .client
            .delete_sitemap(&args.site_url, &args.feed_path)
            .await
        {
            Ok(value) => Ok(contract::success_with_meta(
                value,
                json!({ "mutation": true }),
                started,
            )),
            Err(err) => Ok(contract::error(err, started)),
        }
    }

    /// Add a site to Search Console.
    #[tool(
        name = "gsc_site_add",
        description = "Add a site to the authenticated user's Search Console account. Requires operator profile and webmasters scope."
    )]
    async fn gsc_site_add(
        &self,
        Parameters(args): Parameters<SiteUrlArgs>,
    ) -> Result<CallToolResult, crate::McpError> {
        let started = Instant::now();
        match self.client.add_site(&args.site_url).await {
            Ok(value) => Ok(contract::success_with_meta(
                value,
                json!({ "mutation": true }),
                started,
            )),
            Err(err) => Ok(contract::error(err, started)),
        }
    }

    /// Remove a site from Search Console.
    #[tool(
        name = "gsc_site_delete",
        description = "Remove a site from the authenticated user's Search Console account. Requires operator profile and webmasters scope."
    )]
    async fn gsc_site_delete(
        &self,
        Parameters(args): Parameters<SiteUrlArgs>,
    ) -> Result<CallToolResult, crate::McpError> {
        let started = Instant::now();
        match self.client.delete_site(&args.site_url).await {
            Ok(value) => Ok(contract::success_with_meta(
                value,
                json!({ "mutation": true }),
                started,
            )),
            Err(err) => Ok(contract::error(err, started)),
        }
    }
}

fn search_analytics_ingest_columns(dimensions: &[String]) -> Vec<ScratchpadIngestColumn> {
    let mut columns = dimensions
        .iter()
        .map(|dimension| ScratchpadIngestColumn {
            name: normalize_scratchpad_column_name(dimension),
            logical_type: search_analytics_dimension_logical_type(dimension).to_string(),
        })
        .collect::<Vec<_>>();
    columns.extend([
        ScratchpadIngestColumn {
            name: "clicks".to_string(),
            logical_type: "integer".to_string(),
        },
        ScratchpadIngestColumn {
            name: "impressions".to_string(),
            logical_type: "integer".to_string(),
        },
        ScratchpadIngestColumn {
            name: "ctr".to_string(),
            logical_type: "number".to_string(),
        },
        ScratchpadIngestColumn {
            name: "position".to_string(),
            logical_type: "number".to_string(),
        },
    ]);
    columns
}

fn search_analytics_dimension_logical_type(dimension: &str) -> &'static str {
    match dimension {
        "date" => "date",
        "hour" => "integer",
        _ => "string",
    }
}

fn search_analytics_rows_for_scratchpad(
    upstream: &Value,
    dimensions: &[String],
) -> Vec<Map<String, Value>> {
    upstream
        .get("rows")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .map(|row| search_analytics_row_for_scratchpad(row, dimensions))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn search_analytics_row_for_scratchpad(row: &Value, dimensions: &[String]) -> Map<String, Value> {
    let mut out = Map::new();
    let keys = row
        .get("keys")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for (index, dimension) in dimensions.iter().enumerate() {
        out.insert(
            normalize_scratchpad_column_name(dimension),
            keys.get(index).cloned().unwrap_or(Value::Null),
        );
    }
    for metric in ["clicks", "impressions", "ctr", "position"] {
        out.insert(
            metric.to_string(),
            row.get(metric).cloned().unwrap_or(Value::Null),
        );
    }
    out
}

fn normalize_scratchpad_column_name(raw: &str) -> String {
    match raw {
        "searchAppearance" => "search_appearance".to_string(),
        other => other
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() {
                    ch.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect(),
    }
}

fn scratchpad_session_info_to_json(info: ScratchpadSessionInfo) -> Value {
    json!({
        "session_id": info.session_id,
        "tables_used": info.tables_used,
        "tables_remaining": info.tables_remaining,
        "rows_used": info.rows_used,
        "rows_remaining": info.rows_remaining,
        "ttl_seconds_remaining": info.ttl_seconds_remaining,
    })
}

fn scratchpad_snapshot_to_json(snapshot: ScratchpadSessionSnapshot) -> Value {
    json!({
        "tables_used": snapshot.tables_used,
        "tables_remaining": snapshot.tables_remaining,
        "rows_used": snapshot.rows_used,
        "rows_remaining": snapshot.rows_remaining,
    })
}

fn scratchpad_table_info_to_json(table: ScratchpadTableInfo) -> Value {
    json!({
        "schema": table.schema,
        "name": table.name,
        "table_type": table.table_type,
        "column_count": table.column_count,
        "columns": table.columns
            .into_iter()
            .map(|column| json!({
                "name": column.name,
                "logical_type": column.logical_type,
                "nullable": column.nullable,
            }))
            .collect::<Vec<_>>(),
        "columns_truncated": table.columns_truncated,
    })
}

fn scratchpad_query_projection_to_json(
    projection: ScratchpadQueryProjection,
    offset: u64,
    page_size: u64,
) -> Value {
    json!({
        "rows": projection.rows,
        "row_count_total": projection.row_count_total,
        "columns": projection.columns
            .into_iter()
            .map(|column| json!({
                "name": column.name,
                "logical_type": column.logical_type,
                "nullable": column.nullable,
            }))
            .collect::<Vec<_>>(),
        "offset": offset,
        "page_size": page_size,
        "has_more": offset.saturating_add(page_size) < projection.row_count_total as u64,
        "pagination_mode": projection.pagination_mode,
        "query_hints": projection.query_hints,
    })
}

fn ingest_mode_label(mode: ScratchpadIngestMode) -> &'static str {
    match mode {
        ScratchpadIngestMode::Create => "create",
        ScratchpadIngestMode::Append => "append",
    }
}

fn quote_scratchpad_ident(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn append_evidence_table_error(
    bundle: &mut String,
    table_name: &str,
    err: &mcp_toolkit_scratchpad::ScratchpadError,
) {
    bundle.push_str(&format!("## `{table_name}`\n\n"));
    bundle.push_str(&format!(
        "- Error: `{}`\n\n",
        escape_markdown_cell(&err.to_string())
    ));
}

fn json_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|number| u64::try_from(number).ok()))
        .or_else(|| value.as_f64().map(|number| number as u64))
}

fn markdown_table(projection: &ScratchpadQueryProjection) -> String {
    if projection.columns.is_empty() {
        return "_No columns returned._\n".to_string();
    }
    let headers = projection
        .columns
        .iter()
        .map(|column| escape_markdown_cell(&column.name))
        .collect::<Vec<_>>();
    let mut out = String::new();
    out.push('|');
    out.push_str(&headers.join("|"));
    out.push_str("|\n|");
    out.push_str(&vec!["---"; headers.len()].join("|"));
    out.push_str("|\n");
    for row in &projection.rows {
        out.push('|');
        let values = projection
            .columns
            .iter()
            .map(|column| {
                row.get(&column.name)
                    .map(markdown_value)
                    .unwrap_or_else(|| "".to_string())
            })
            .collect::<Vec<_>>();
        out.push_str(&values.join("|"));
        out.push_str("|\n");
    }
    out
}

fn markdown_value(value: &Value) -> String {
    match value {
        Value::Null => "".to_string(),
        Value::String(text) => escape_markdown_cell(text),
        other => escape_markdown_cell(&other.to_string()),
    }
}

fn escape_markdown_cell(value: &str) -> String {
    value
        .replace('|', "\\|")
        .replace('\r', "")
        .replace('\n', " ")
}

fn auth_env_presence() -> Value {
    json!({
        "GOOGLE_APPLICATION_CREDENTIALS": std::env::var_os("GOOGLE_APPLICATION_CREDENTIALS").is_some(),
        "GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON_PATH": std::env::var_os("GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON_PATH").is_some(),
        "GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON": std::env::var_os("GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON").is_some(),
        "GOOGLE_SEARCH_CONSOLE_MCP_OAUTH_CLIENT_SECRET_JSON": std::env::var_os("GOOGLE_SEARCH_CONSOLE_MCP_OAUTH_CLIENT_SECRET_JSON").is_some(),
        "GOOGLE_SEARCH_CONSOLE_MCP_OAUTH_REFRESH_TOKEN": std::env::var_os("GOOGLE_SEARCH_CONSOLE_MCP_OAUTH_REFRESH_TOKEN").is_some(),
        "GOOGLE_SEARCH_CONSOLE_MCP_SHARED_ADC": env_bool_true("GOOGLE_SEARCH_CONSOLE_MCP_SHARED_ADC"),
        "CLOUDSDK_CONFIG": std::env::var_os("CLOUDSDK_CONFIG").is_some(),
    })
}

fn selected_adc_file_status() -> Value {
    if env_bool_true("GOOGLE_SEARCH_CONSOLE_MCP_SHARED_ADC") {
        return conventional_adc_credentials_path()
            .map(|path| {
                json!({
                    "kind": "shared",
                    "role": "explicit",
                    "path": path.display().to_string()
                })
            })
            .unwrap_or_else(|| {
                json!({
                    "kind": "shared",
                    "role": "explicit",
                    "path": null
                })
            });
    }
    if let Some(path) = server_adc_credentials_path() {
        return json!({
            "kind": "server_specific",
            "role": "preferred",
            "path": path.display().to_string()
        });
    }
    json!({
        "kind": "unknown",
        "role": "not_configured",
        "path": null
    })
}

fn env_bool_true(name: &str) -> bool {
    std::env::var(name)
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn operator_scope_check_to_json(check: OperatorScopeCheck) -> Value {
    json!({
        "checked": true,
        "ok": check.ok,
        "required_scope": check.required_scope,
        "granted_scopes": check.granted_scopes,
    })
}

fn auth_next_steps(
    verified: bool,
    token_ok: Option<bool>,
    operator_scope_relevant: bool,
    operator_scope_ok: Option<bool>,
    current_scope: &str,
) -> Vec<String> {
    let config_dir = server_cloudsdk_config_dir();
    let config_dir_ref = config_dir.as_deref();
    let current_login = shell_join_with_cloudsdk_config(
        &gcloud_adc_login_command(current_scope, None, true),
        config_dir_ref,
    );
    let read_login = shell_join_with_cloudsdk_config(
        &gcloud_adc_login_command(DEFAULT_SCOPE, None, true),
        config_dir_ref,
    );
    let operator_login = shell_join_with_cloudsdk_config(
        &gcloud_adc_login_command(WRITE_SCOPE, None, true),
        config_dir_ref,
    );
    let quota_project_command = shell_join_with_cloudsdk_config(
        &gcloud_set_quota_project_command("YOUR_PROJECT"),
        config_dir_ref,
    );

    match (
        verified,
        token_ok,
        operator_scope_relevant,
        operator_scope_ok,
    ) {
        (false, _, _, _) => vec![
            "Run gsc_auth_status with verify_token=true when you are ready to prove credentials.".to_string(),
            format!("If credentials are missing, call gsc_auth_login_command or run `{current_login}` for local ADC."),
            format!("Set an ADC quota project if Google asks for one: `{quota_project_command}`."),
            "Call gsc_sites_list after auth is verified to discover exact property strings.".to_string(),
        ],
        (true, Some(true), true, Some(false) | None) => vec![
            format!("Run gsc_auth_login_command with write_scope=true or reauthenticate with `{operator_login}`."),
            format!("Set a quota project if Google reports that local ADC credentials require one: `{quota_project_command}`."),
            "Call gsc_auth_status with verify_token=true again before sitemap/site mutations.".to_string(),
        ],
        (true, Some(true), _, _) => vec![
            "Call gsc_sites_list to discover exact property strings.".to_string(),
            "Use gsc_search_analytics_query for Search Console performance data.".to_string(),
        ],
        (true, Some(false), _, _) | (true, None, _, _) => vec![
            format!("Call gsc_auth_login_command and run the returned command for local ADC, or run `{read_login}`."),
            format!("If Google reports a quota-project problem, run `{quota_project_command}` and enable searchconsole.googleapis.com on that project."),
            "For service accounts, set GOOGLE_APPLICATION_CREDENTIALS or GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON_PATH.".to_string(),
            "Ensure the authenticated principal has access to the Search Console property.".to_string(),
        ],
    }
}

fn gsc_auth_setup_plan(scope: &str) -> GoogleProviderAuthSetupPlan {
    gsc_provider_auth_config(scope).adc_setup_plan()
}

fn gsc_provider_auth_config(scope: &str) -> GoogleProviderAuthConfig {
    GoogleProviderAuthConfig::new("Search Console API", vec![scope.to_string()])
        .with_api_service_name("searchconsole.googleapis.com")
}

fn gcloud_adc_login_command(
    scope: &str,
    client_id_file: Option<&Path>,
    headless: bool,
) -> Vec<String> {
    let config = gsc_provider_auth_config(scope);
    if let Some(path) = client_id_file {
        config.adc_login_command_with_client_id_file(headless, &path.display().to_string())
    } else {
        config.adc_login_command(headless)
    }
}

fn gcloud_set_quota_project_command(project: &str) -> Vec<String> {
    google_adc_quota_project_command(project)
}

fn shell_join(parts: &[String]) -> String {
    format_provider_auth_command(parts)
}

fn shell_join_with_cloudsdk_config(parts: &[String], cloudsdk_config: Option<&Path>) -> String {
    if let Some(dir) = cloudsdk_config {
        let dir_str = shell_join(&[dir.display().to_string()]);
        let command = shell_join(parts);
        if command.is_empty() {
            #[cfg(windows)]
            {
                format!("$env:CLOUDSDK_CONFIG={dir_str}")
            }
            #[cfg(not(windows))]
            {
                format!("CLOUDSDK_CONFIG={dir_str}")
            }
        } else {
            #[cfg(windows)]
            {
                format!("$env:CLOUDSDK_CONFIG={dir_str}; {command}")
            }
            #[cfg(not(windows))]
            {
                format!("CLOUDSDK_CONFIG={dir_str} {command}")
            }
        }
    } else {
        shell_join(parts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registered_tool_print_helpers_are_metadata_only() {
        let read_only_names = registered_tool_names(CapabilityProfile::ReadOnly);
        assert!(!read_only_names.is_empty());
        assert!(!read_only_names.contains(&"gsc_sitemap_submit".to_string()));
        assert!(
            registered_tool_names(CapabilityProfile::Operator)
                .contains(&"gsc_sitemap_submit".to_string())
        );
        assert!(
            registered_tool_schema_snapshot(CapabilityProfile::ReadOnly)
                .as_object()
                .map(|map| !map.is_empty())
                .unwrap_or(false)
        );
    }

    #[test]
    fn redacts_auth_status_errors() {
        let err = crate::error::SearchConsoleError::AuthBootstrap(
            "client_secret=abc refresh_token=xyz".to_string(),
        );
        let redacted = redact_tool_error_message(&err);
        assert!(!redacted.contains("abc"));
        assert!(!redacted.contains("xyz"));
        assert!(redacted.contains("[redacted]"));
    }

    #[test]
    fn cloudsdk_config_command_is_shell_appropriate() {
        let command = shell_join_with_cloudsdk_config(
            &["gcloud".to_string(), "auth".to_string()],
            Some(Path::new("/tmp/gsc adc")),
        );

        #[cfg(windows)]
        assert!(command.starts_with("$env:CLOUDSDK_CONFIG="));

        #[cfg(not(windows))]
        assert!(command.starts_with("CLOUDSDK_CONFIG='/tmp/gsc adc' gcloud auth"));
    }
}
