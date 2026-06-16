use std::time::Instant;

use mcp_toolkit_core::tool_inventory::{ToolOperation, ToolSearchFilter, ToolSearchResponse};
use mcp_toolkit_core::tool_schema::tool_schema_snapshot_value;
use mcp_toolkit_scratchpad::{
    ScratchpadIngestColumn, ScratchpadIngestMode, ScratchpadQueryProjection,
    ScratchpadSessionInfo, ScratchpadTableInfo,
};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::tool;
use rmcp::tool_router;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::auth_ux::{
    auth_login_cli_command, local_credential_material_detected, login_command_for_scope,
};
use crate::config::{
    CapabilityProfile, DEFAULT_SCOPE, WRITE_SCOPE, scope_allows_mutation, scope_allows_read,
};
use crate::contract;
use crate::error::SearchConsoleError;
use crate::gsc_client::{AuthSource, SearchAnalyticsRequest};
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
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SearchAnalyticsResponseMode {
    /// Return the raw Google Search Console API response.
    #[default]
    Raw,
    /// Return rows in a compact, export-friendly shape with paging receipts.
    Compact,
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
    /// Maximum rows to return, 1..25,000 by default configuration.
    #[serde(default)]
    pub row_limit: Option<u32>,
    /// Zero-based first row offset for paging.
    #[serde(default)]
    pub start_row: Option<u32>,
    /// Optional data state: final, all, or hourly_all.
    #[serde(default)]
    pub data_state: Option<String>,
    /// Response shape. Use compact for agent-friendly batch evidence rows.
    #[serde(default)]
    pub response_mode: SearchAnalyticsResponseMode,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScratchpadSessionArgs {
    /// Scratchpad session id. Use a short stable name for the current analysis thread.
    pub session_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScratchpadListSessionsArgs {
    /// Maximum sessions to return, 1..100.
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScratchpadListTablesArgs {
    /// Scratchpad session id.
    pub session_id: String,
    /// Maximum tables to return, 1..100.
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScratchpadQueryArgs {
    /// Scratchpad session id.
    pub session_id: String,
    /// Read-only DuckDB SQL. SELECT/WITH are paginated; DESCRIBE/SUMMARIZE are allowed helpers.
    pub sql: String,
    /// Zero-based row offset.
    #[serde(default)]
    pub offset: Option<u64>,
    /// Maximum rows to return, 1..1,000.
    #[serde(default)]
    pub page_size: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScratchpadDropTableArgs {
    /// Scratchpad session id.
    pub session_id: String,
    /// Scratchpad table name.
    pub table_name: String,
    /// Return success when the table is already absent.
    #[serde(default)]
    pub if_exists: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScratchpadRuntimeLimitsArgs {
    /// Optional replacement maximum active sessions.
    #[serde(default)]
    pub max_sessions: Option<usize>,
    /// Optional replacement maximum tables per session.
    #[serde(default)]
    pub max_tables_per_session: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScratchpadIngestSearchAnalyticsArgs {
    /// Scratchpad session id. Opened automatically when missing.
    pub session_id: String,
    /// Destination table name. Must use [A-Za-z0-9_] and start with a letter or underscore.
    pub table_name: String,
    /// Append to an existing table instead of creating a new table.
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
    /// Maximum rows to ingest, 1..25,000 by default configuration.
    #[serde(default)]
    pub row_limit: Option<u32>,
    /// Zero-based first row offset for paging.
    #[serde(default)]
    pub start_row: Option<u32>,
    /// Optional data state: final, all, or hourly_all.
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

fn compact_search_analytics_response(
    value: Value,
    dimensions: &[String],
    row_limit: Option<u32>,
    start_row: Option<u32>,
) -> Value {
    let requested_row_limit = row_limit.unwrap_or(1_000);
    let start_row = start_row.unwrap_or(0);
    let rows = value
        .get("rows")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let compact_rows: Vec<Value> = rows
        .iter()
        .map(|row| compact_search_analytics_row(row, dimensions))
        .collect();
    let returned_rows = compact_rows.len() as u32;
    let next_start_row = if requested_row_limit > 0 && returned_rows == requested_row_limit {
        Some(start_row.saturating_add(returned_rows))
    } else {
        None
    };

    let mut summary = Map::new();
    summary.insert("row_count".to_string(), json!(returned_rows));
    summary.insert("start_row".to_string(), json!(start_row));
    summary.insert(
        "requested_row_limit".to_string(),
        json!(requested_row_limit),
    );
    summary.insert("has_more_hint".to_string(), json!(next_start_row.is_some()));
    if let Some(next_start_row) = next_start_row {
        summary.insert("next_start_row".to_string(), json!(next_start_row));
    }
    summary.insert("dimensions".to_string(), json!(dimensions));
    summary.insert(
        "metrics".to_string(),
        json!(["clicks", "impressions", "ctr", "position"]),
    );
    if let Some(response_aggregation_type) = value.get("responseAggregationType") {
        summary.insert(
            "response_aggregation_type".to_string(),
            response_aggregation_type.clone(),
        );
    }

    let mut columns: Vec<Value> = dimensions
        .iter()
        .map(|dimension| json!(dimension))
        .collect();
    columns.extend(["clicks", "impressions", "ctr", "position"].map(|metric| json!(metric)));

    json!({
        "summary": summary,
        "columns": columns,
        "rows": compact_rows,
    })
}

fn compact_search_analytics_row(row: &Value, dimensions: &[String]) -> Value {
    let keys = row
        .get("keys")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut compact = Map::new();

    for (index, dimension) in dimensions.iter().enumerate() {
        let value = keys.get(index).cloned().unwrap_or(Value::Null);
        compact.insert(dimension.clone(), value);
    }
    if dimensions.is_empty() && !keys.is_empty() {
        compact.insert("keys".to_string(), Value::Array(keys));
    }

    for metric in ["clicks", "impressions", "ctr", "position"] {
        compact.insert(
            metric.to_string(),
            row.get(metric).cloned().unwrap_or(Value::Null),
        );
    }

    Value::Object(compact)
}

fn search_analytics_request_from_scratchpad_args(
    args: ScratchpadIngestSearchAnalyticsArgs,
) -> SearchAnalyticsRequest {
    SearchAnalyticsRequest {
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
    value: &Value,
    dimensions: &[String],
) -> Vec<Map<String, Value>> {
    value
        .get("rows")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .map(|row| search_analytics_row_for_scratchpad(row, dimensions))
                .collect()
        })
        .unwrap_or_default()
}

fn search_analytics_row_for_scratchpad(row: &Value, dimensions: &[String]) -> Map<String, Value> {
    let keys = row
        .get("keys")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut projected = Map::new();
    for (index, dimension) in dimensions.iter().enumerate() {
        projected.insert(
            normalize_scratchpad_column_name(dimension),
            keys.get(index).cloned().unwrap_or(Value::Null),
        );
    }
    for metric in ["clicks", "impressions", "ctr", "position"] {
        projected.insert(
            metric.to_string(),
            row.get(metric).cloned().unwrap_or(Value::Null),
        );
    }
    projected
}

fn normalize_scratchpad_column_name(value: &str) -> String {
    let mut normalized = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            normalized.push(ch.to_ascii_lowercase());
        } else {
            normalized.push('_');
        }
    }
    let normalized = normalized.trim_matches('_');
    if normalized.is_empty() {
        return "column".to_string();
    }
    if normalized
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_digit())
    {
        format!("col_{normalized}")
    } else {
        normalized.to_string()
    }
}

fn scratchpad_projection_value(
    projection: ScratchpadQueryProjection,
    session_id: &str,
    offset: u64,
    page_size: u64,
) -> Value {
    let row_count_returned = projection.rows.len();
    let next_offset = offset.saturating_add(row_count_returned as u64);
    let next_offset = if (next_offset as usize) < projection.row_count_total {
        Some(next_offset)
    } else {
        None
    };
    json!({
        "session_id": session_id,
        "summary": {
            "row_count_total": projection.row_count_total,
            "row_count_returned": row_count_returned,
            "offset": offset,
            "page_size": page_size,
            "has_more": next_offset.is_some(),
            "next_offset": next_offset,
            "pagination_mode": projection.pagination_mode,
            "query_hints": projection.query_hints,
        },
        "columns": projection.columns.into_iter().map(scratchpad_column_value).collect::<Vec<_>>(),
        "rows": projection.rows,
    })
}

fn scratchpad_session_value(info: ScratchpadSessionInfo) -> Value {
    json!({
        "session_id": info.session_id,
        "tables_used": info.tables_used,
        "tables_remaining": info.tables_remaining,
        "rows_used": info.rows_used,
        "rows_remaining": info.rows_remaining,
        "ttl_seconds_remaining": info.ttl_seconds_remaining,
    })
}

fn scratchpad_table_value(table: ScratchpadTableInfo) -> Value {
    json!({
        "schema": table.schema,
        "name": table.name,
        "table_type": table.table_type,
        "column_count": table.column_count,
        "columns_truncated": table.columns_truncated,
        "columns": table.columns.into_iter().map(scratchpad_column_value).collect::<Vec<_>>(),
    })
}

fn scratchpad_column_value(column: mcp_toolkit_scratchpad::ScratchpadTableColumnInfo) -> Value {
    json!({
        "name": column.name,
        "logical_type": column.logical_type,
        "nullable": column.nullable,
    })
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
                "auth_source_candidate": self.client.auth_source().as_str(),
                "auth_source_note": "A candidate auth source is not proof credentials exist; call gsc_auth_status or auth status --verify-token.",
                "scope": self.client.scope(),
                "recommended_cli": {
                    "login": "google-search-console-mcp auth login",
                    "login_with_client_id_file": "google-search-console-mcp auth login --client-id-file /path/to/client_id.json",
                    "status": "google-search-console-mcp auth status --verify-token",
                    "doctor": "google-search-console-mcp auth doctor --verify-token"
                },
                "first_steps": [
                    "Before starting a long-lived MCP client, run google-search-console-mcp auth login for the easiest browser login.",
                    "Call google-search-console-mcp auth status --verify-token or gsc_auth_status with verify_token=true to prove Google auth without returning a token.",
                    "If verification says local ADC requires a quota project, enable searchconsole.googleapis.com on a Google Cloud project and run gcloud auth application-default set-quota-project YOUR_PROJECT.",
                    "If Google rejects the Search Console scope during login, create a Desktop OAuth client and rerun google-search-console-mcp auth login --client-id-file /path/to/client_id.json.",
                    "If you are already inside MCP, call gsc_auth_status with verify_token=false to inspect configuration without making a token request.",
                    "If credentials are missing, call gsc_auth_login_command and run the returned gcloud command.",
                    "Call gsc_sites_list to discover the exact Search Console property string.",
                    "Use the exact siteUrl from gsc_sites_list when querying analytics, URL inspection, or sitemaps."
                ],
                "credential_options": [
                    {
                        "name": "Application Default Credentials",
                        "best_for": "lowest-friction local browser login",
                        "command": "google-search-console-mcp auth login",
                        "client_id_file_command": "google-search-console-mcp auth login --client-id-file /path/to/client_id.json",
                        "quota_project_command": "gcloud auth application-default set-quota-project YOUR_PROJECT",
                        "quota_project_note": "Only needed when Google reports local ADC requires a quota project; the project must have the Search Console API enabled.",
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
                    "gsc_scratchpad_ingest_search_analytics",
                    "gsc_scratchpad_query",
                    "gsc_url_inspection_index_inspect",
                    "gsc_sitemaps_list"
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
        let auth_source_candidate = self.client.auth_source();
        let credential_material_detected = credential_material_detected_for_auth_source(
            auth_source_candidate,
            local_credential_material_detected(),
        );
        let auth_source = if matches!(
            auth_source_candidate,
            AuthSource::GoogleDefaultProviderChain
        ) && !credential_material_detected
            && token_ok != Some(true)
        {
            Value::Null
        } else {
            json!(auth_source_candidate.as_str())
        };

        Ok(contract::success(
            json!({
                "auth_source": auth_source,
                "auth_source_candidate": auth_source_candidate.as_str(),
                "scope": self.client.scope(),
                "profile": self.profile.as_str(),
                "operator_tools_enabled": self.profile.allows_mutation(),
                "quota_project_configured": self.client.quota_project_configured(),
                "credential_material_detected": credential_material_detected,
                "detected_env": auth_env_presence(),
                "token_check": token_check,
                "next_steps": auth_next_steps(self.profile, self.client.scope(), args.verify_token, token_ok),
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
        let scope = login_scope_for_mcp_command(self.client.scope(), args.write_scope);
        let command = login_command_for_scope(
            scope,
            args.headless,
            args.client_id_file.as_deref().map(std::path::Path::new),
        );
        let preferred_cli = auth_login_cli_command(
            scope,
            args.write_scope,
            args.headless,
            args.client_id_file.as_deref().map(std::path::Path::new),
        );
        let after_login = after_login_instruction(args.write_scope, self.client.scope(), scope);
        Ok(contract::success(
            json!({
                "command": command,
                "preferred_cli": preferred_cli,
                "scope": scope,
                "write_scope": args.write_scope,
                "headless": args.headless,
                "client_id_file": args.client_id_file,
                "after_login": after_login,
                "client_id_file_hint": "Search Console scopes may require a Google OAuth client id file; pass client_id_file when Google rejects the requested scope.",
                "quota_project_hint": "If verification says local ADC requires a quota project, run `gcloud services enable searchconsole.googleapis.com --project YOUR_PROJECT` and `gcloud auth application-default set-quota-project YOUR_PROJECT`, then verify again.",
                "operator_env": if args.write_scope {
                    json!({
                        "GOOGLE_SEARCH_CONSOLE_MCP_PROFILE": "operator",
                        "GOOGLE_SEARCH_CONSOLE_MCP_SCOPE": WRITE_SCOPE
                    })
                } else {
                    Value::Null
                },
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
        description = "Query Search Console Search Analytics performance rows for a property and date range. Set response_mode to compact for export-friendly batch evidence."
    )]
    async fn gsc_search_analytics_query(
        &self,
        Parameters(args): Parameters<SearchAnalyticsQueryArgs>,
    ) -> Result<CallToolResult, crate::McpError> {
        let started = Instant::now();
        let dimensions = args.dimensions.clone();
        let row_limit = args.row_limit;
        let start_row = args.start_row;
        let response_mode = args.response_mode;
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
            Ok(value) => {
                let value = match response_mode {
                    SearchAnalyticsResponseMode::Raw => value,
                    SearchAnalyticsResponseMode::Compact => {
                        compact_search_analytics_response(value, &dimensions, row_limit, start_row)
                    }
                };
                Ok(contract::success(value, started))
            }
            Err(err) => Ok(contract::error(err, started)),
        }
    }

    /// Open or refresh a scratchpad session.
    #[tool(
        name = "gsc_scratchpad_open_session",
        description = "Open or refresh a bounded DuckDB scratchpad session for local Search Console analysis."
    )]
    async fn gsc_scratchpad_open_session(
        &self,
        Parameters(args): Parameters<ScratchpadSessionArgs>,
    ) -> Result<CallToolResult, crate::McpError> {
        let started = Instant::now();
        match self.scratchpad_sessions.open_session(&args.session_id) {
            Ok(info) => Ok(contract::success(scratchpad_session_value(info), started)),
            Err(err) => Ok(contract::error(SearchConsoleError::from(err), started)),
        }
    }

    /// Release a scratchpad session and delete its local DuckDB file.
    #[tool(
        name = "gsc_scratchpad_release_session",
        description = "Release a scratchpad session and delete its local DuckDB data."
    )]
    async fn gsc_scratchpad_release_session(
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
            Err(err) => Ok(contract::error(SearchConsoleError::from(err), started)),
        }
    }

    /// List active scratchpad sessions.
    #[tool(
        name = "gsc_scratchpad_list_sessions",
        description = "List active bounded DuckDB scratchpad sessions."
    )]
    async fn gsc_scratchpad_list_sessions(
        &self,
        Parameters(args): Parameters<ScratchpadListSessionsArgs>,
    ) -> Result<CallToolResult, crate::McpError> {
        let started = Instant::now();
        let limit = args.limit.unwrap_or(20).clamp(1, 100);
        match self.scratchpad_sessions.list_sessions(limit) {
            Ok(sessions) => Ok(contract::success(
                json!({
                    "sessions": sessions.into_iter().map(scratchpad_session_value).collect::<Vec<_>>(),
                    "limit": limit,
                }),
                started,
            )),
            Err(err) => Ok(contract::error(SearchConsoleError::from(err), started)),
        }
    }

    /// List scratchpad tables for one session.
    #[tool(
        name = "gsc_scratchpad_list_tables",
        description = "List tables and column previews for one scratchpad session."
    )]
    async fn gsc_scratchpad_list_tables(
        &self,
        Parameters(args): Parameters<ScratchpadListTablesArgs>,
    ) -> Result<CallToolResult, crate::McpError> {
        let started = Instant::now();
        let limit = args.limit.unwrap_or(50).clamp(1, 100);
        match self.scratchpad_sessions.list_tables(&args.session_id, limit) {
            Ok(tables) => Ok(contract::success(
                json!({
                    "session_id": args.session_id,
                    "tables": tables.into_iter().map(scratchpad_table_value).collect::<Vec<_>>(),
                    "limit": limit,
                }),
                started,
            )),
            Err(err) => Ok(contract::error(SearchConsoleError::from(err), started)),
        }
    }

    /// Query scratchpad tables with restricted read-only SQL.
    #[tool(
        name = "gsc_scratchpad_query",
        description = "Run read-only DuckDB SQL against a scratchpad session and return a bounded page of rows."
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
                scratchpad_projection_value(projection, &args.session_id, offset, page_size),
                started,
            )),
            Err(err) => Ok(contract::error(SearchConsoleError::from(err), started)),
        }
    }

    /// Drop one scratchpad table.
    #[tool(
        name = "gsc_scratchpad_drop_table",
        description = "Drop a table from a scratchpad session and update local row/table accounting."
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
                    "session_snapshot": {
                        "tables_used": stats.session_snapshot.tables_used,
                        "tables_remaining": stats.session_snapshot.tables_remaining,
                        "rows_used": stats.session_snapshot.rows_used,
                        "rows_remaining": stats.session_snapshot.rows_remaining,
                    },
                }),
                started,
            )),
            Err(err) => Ok(contract::error(SearchConsoleError::from(err), started)),
        }
    }

    /// Return scratchpad runtime limits.
    #[tool(
        name = "gsc_scratchpad_get_runtime_limits",
        description = "Return configured and current runtime limits for scratchpad sessions."
    )]
    async fn gsc_scratchpad_get_runtime_limits(&self) -> Result<CallToolResult, crate::McpError> {
        let started = Instant::now();
        let config = self.scratchpad_sessions.config();
        Ok(contract::success(
            json!({
                "session_ttl_secs": config.session_ttl.as_secs(),
                "max_sessions": self.scratchpad_sessions.max_sessions_limit(),
                "configured_max_sessions": config.max_sessions,
                "max_tables_per_session": self.scratchpad_sessions.max_tables_per_session_limit(),
                "configured_max_tables_per_session": config.max_tables_per_session,
                "max_rows_per_session": config.max_rows_per_session,
                "max_memory_mb": config.max_memory_mb,
                "query_timeout_ms": config.query_timeout.as_millis(),
                "max_sql_bytes": config.max_sql_bytes,
                "root_dir": config.root_dir.display().to_string(),
            }),
            started,
        ))
    }

    /// Adjust scratchpad runtime session/table limits.
    #[tool(
        name = "gsc_scratchpad_set_runtime_limits",
        description = "Adjust scratchpad runtime session and table limits without restarting the server."
    )]
    async fn gsc_scratchpad_set_runtime_limits(
        &self,
        Parameters(args): Parameters<ScratchpadRuntimeLimitsArgs>,
    ) -> Result<CallToolResult, crate::McpError> {
        let started = Instant::now();
        if let Some(max_sessions) = args.max_sessions {
            if let Err(err) = self
                .scratchpad_sessions
                .set_max_sessions_limit(max_sessions)
            {
                return Ok(contract::error(SearchConsoleError::from(err), started));
            }
        }
        if let Some(max_tables_per_session) = args.max_tables_per_session {
            if let Err(err) = self
                .scratchpad_sessions
                .set_max_tables_per_session_limit(max_tables_per_session)
            {
                return Ok(contract::error(SearchConsoleError::from(err), started));
            }
        }
        Ok(contract::success(
            json!({
                "max_sessions": self.scratchpad_sessions.max_sessions_limit(),
                "max_tables_per_session": self.scratchpad_sessions.max_tables_per_session_limit(),
            }),
            started,
        ))
    }

    /// Ingest Search Analytics rows into a scratchpad table.
    #[tool(
        name = "gsc_scratchpad_ingest_search_analytics",
        description = "Query Search Console Search Analytics and ingest the returned rows into a bounded DuckDB scratchpad table."
    )]
    async fn gsc_scratchpad_ingest_search_analytics(
        &self,
        Parameters(args): Parameters<ScratchpadIngestSearchAnalyticsArgs>,
    ) -> Result<CallToolResult, crate::McpError> {
        let started = Instant::now();
        let session_id = args.session_id.clone();
        let table_name = args.table_name.clone();
        let append = args.append;
        let site_url = args.site_url.clone();
        let start_date = args.start_date.clone();
        let end_date = args.end_date.clone();
        let dimensions = args.dimensions.clone();
        let row_limit = args.row_limit;
        let requested_row_limit = row_limit.unwrap_or(1_000);
        let start_row = args.start_row.unwrap_or(0);
        let request = search_analytics_request_from_scratchpad_args(args);
        let upstream = match self.client.search_analytics_query(request).await {
            Ok(value) => value,
            Err(err) => return Ok(contract::error(err, started)),
        };
        let columns = search_analytics_ingest_columns(&dimensions);
        let rows = search_analytics_rows_for_scratchpad(&upstream, &dimensions);
        let mode = if append {
            ScratchpadIngestMode::Append
        } else {
            ScratchpadIngestMode::Create
        };
        if let Err(err) = self.scratchpad_sessions.open_session(&session_id) {
            return Ok(contract::error(SearchConsoleError::from(err), started));
        }
        match self.scratchpad_sessions.ingest_rows_with_mode(
            &session_id,
            &table_name,
            &columns,
            &rows,
            mode,
        ) {
            Ok(stats) => Ok(contract::success(
                json!({
                    "session_id": session_id,
                    "table_name": table_name,
                    "mode": if append { "append" } else { "create" },
                    "rows_inserted": stats.rows_inserted,
                    "columns_inserted": stats.columns_inserted,
                    "columns": columns.into_iter().map(|column| {
                        json!({
                            "name": column.name,
                            "logical_type": column.logical_type,
                        })
                    }).collect::<Vec<_>>(),
                    "source": {
                        "site_url": site_url,
                        "start_date": start_date,
                        "end_date": end_date,
                        "dimensions": dimensions,
                        "start_row": start_row,
                        "requested_row_limit": requested_row_limit,
                        "has_more_hint": requested_row_limit > 0
                            && stats.rows_inserted as u32 == requested_row_limit,
                        "next_start_row": if requested_row_limit > 0
                            && stats.rows_inserted as u32 == requested_row_limit {
                            Some(start_row.saturating_add(stats.rows_inserted as u32))
                        } else {
                            None
                        },
                    },
                    "session_snapshot": {
                        "tables_used": stats.session_snapshot.tables_used,
                        "tables_remaining": stats.session_snapshot.tables_remaining,
                        "rows_used": stats.session_snapshot.rows_used,
                        "rows_remaining": stats.session_snapshot.rows_remaining,
                    },
                }),
                started,
            )),
            Err(err) => Ok(contract::error(SearchConsoleError::from(err), started)),
        }
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

fn auth_env_presence() -> Value {
    json!({
        "GOOGLE_APPLICATION_CREDENTIALS": std::env::var_os("GOOGLE_APPLICATION_CREDENTIALS").is_some(),
        "GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON_PATH": std::env::var_os("GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON_PATH").is_some(),
        "GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON": std::env::var_os("GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON").is_some(),
        "GOOGLE_SEARCH_CONSOLE_MCP_OAUTH_CLIENT_SECRET_JSON": std::env::var_os("GOOGLE_SEARCH_CONSOLE_MCP_OAUTH_CLIENT_SECRET_JSON").is_some(),
        "GOOGLE_SEARCH_CONSOLE_MCP_OAUTH_REFRESH_TOKEN": std::env::var_os("GOOGLE_SEARCH_CONSOLE_MCP_OAUTH_REFRESH_TOKEN").is_some(),
        "CLOUDSDK_CONFIG": std::env::var_os("CLOUDSDK_CONFIG").is_some(),
    })
}

fn credential_material_detected_for_auth_source(
    auth_source: AuthSource,
    local_detected: bool,
) -> bool {
    local_detected || !matches!(auth_source, AuthSource::GoogleDefaultProviderChain)
}

fn login_scope_for_mcp_command(current_scope: &str, write_scope: bool) -> &str {
    if write_scope {
        WRITE_SCOPE
    } else if scope_allows_read(current_scope) && !scope_allows_mutation(current_scope) {
        current_scope
    } else {
        DEFAULT_SCOPE
    }
}

fn after_login_instruction(write_scope: bool, current_scope: &str, login_scope: &str) -> String {
    let ambient_scope = std::env::var("GOOGLE_SEARCH_CONSOLE_MCP_SCOPE").ok();
    after_login_instruction_with_env(
        write_scope,
        current_scope,
        login_scope,
        ambient_scope.as_deref(),
    )
}

fn after_login_instruction_with_env(
    write_scope: bool,
    current_scope: &str,
    login_scope: &str,
    ambient_scope: Option<&str>,
) -> String {
    if write_scope {
        "Set GOOGLE_SEARCH_CONSOLE_MCP_PROFILE=operator and GOOGLE_SEARCH_CONSOLE_MCP_SCOPE=https://www.googleapis.com/auth/webmasters, or start the MCP server with `--profile operator --scope https://www.googleapis.com/auth/webmasters`, before using mutation tools; then restart stdio MCP clients and verify auth.".to_string()
    } else if current_scope != login_scope
        || ambient_scope
            .filter(|scope| !scope.is_empty())
            .is_some_and(|scope| scope != login_scope)
    {
        format!(
            "Unset GOOGLE_SEARCH_CONSOLE_MCP_SCOPE, set GOOGLE_SEARCH_CONSOLE_MCP_SCOPE={login_scope}, or update any MCP launcher `--scope` argument before restarting stdio MCP clients; stale scope configuration overrides the login scope."
        )
    } else {
        "Restart stdio MCP clients that keep long-lived server processes, then call gsc_auth_status with verify_token=true or run google-search-console-mcp auth status --verify-token.".to_string()
    }
}

fn auth_next_steps(
    profile: CapabilityProfile,
    scope: &str,
    verified: bool,
    token_ok: Option<bool>,
) -> Vec<String> {
    let operator_missing_write_scope = profile.allows_mutation() && !scope_allows_mutation(scope);
    let missing_search_console_scope = !scope_allows_read(scope);
    let read_scope_step = format!(
        "Set GOOGLE_SEARCH_CONSOLE_MCP_SCOPE={DEFAULT_SCOPE} or start the MCP server with `--scope {DEFAULT_SCOPE}` for read-only tools; use {WRITE_SCOPE} or `--scope {WRITE_SCOPE}` for operator tools."
    );
    let login_command = if operator_missing_write_scope {
        "google-search-console-mcp auth login --write-scope"
    } else if missing_search_console_scope {
        "google-search-console-mcp auth login --scope https://www.googleapis.com/auth/webmasters.readonly"
    } else {
        "google-search-console-mcp auth login"
    };
    match (verified, token_ok) {
        (false, _) => {
            let mut steps = Vec::new();
            if missing_search_console_scope {
                steps.push(read_scope_step.clone());
            }
            if operator_missing_write_scope {
                steps.push(format!(
                    "Set GOOGLE_SEARCH_CONSOLE_MCP_SCOPE={WRITE_SCOPE} or start the MCP server with `--scope {WRITE_SCOPE}` before using operator mode."
                ));
            }
            steps.push("Run google-search-console-mcp auth status --verify-token, or call gsc_auth_status with verify_token=true, when you are ready to prove credentials.".to_string());
            steps.push(format!(
                "If credentials are missing, run {login_command} or call gsc_auth_login_command for the gcloud command."
            ));
            steps.push(
                "Call gsc_sites_list after auth is verified to discover exact property strings."
                    .to_string(),
            );
            steps
        }
        (true, Some(true)) => {
            let mut steps = Vec::new();
            if missing_search_console_scope {
                steps.push(read_scope_step);
            }
            if operator_missing_write_scope {
                steps.push(format!(
                    "Set GOOGLE_SEARCH_CONSOLE_MCP_SCOPE={WRITE_SCOPE} or start the MCP server with `--scope {WRITE_SCOPE}` before using operator tools."
                ));
            }
            steps.push(
                "Restart MCP clients that keep long-lived stdio server processes.".to_string(),
            );
            steps.push("Call gsc_sites_list to discover exact property strings.".to_string());
            steps.push(
                "Use gsc_search_analytics_query for Search Console performance data.".to_string(),
            );
            steps
        }
        (true, Some(false)) | (true, None) => {
            let mut steps = vec![
                format!("Run {login_command} for local browser login."),
                "If the token check reports that local ADC requires a quota project, run gcloud auth application-default set-quota-project YOUR_PROJECT after enabling searchconsole.googleapis.com on that project.".to_string(),
                "Call gsc_auth_login_command if you need a copyable gcloud command inside MCP."
                    .to_string(),
                "For service accounts, set GOOGLE_APPLICATION_CREDENTIALS or GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON_PATH.".to_string(),
                "If server-specific credential env vars are malformed, fix or clear them before browser login because they override Application Default Credentials.".to_string(),
                "Ensure the authenticated principal has access to the Search Console property."
                    .to_string(),
            ];
            if missing_search_console_scope {
                steps.insert(0, read_scope_step);
            }
            if operator_missing_write_scope {
                steps.insert(
                    1,
                    format!(
                        "Set GOOGLE_SEARCH_CONSOLE_MCP_SCOPE={WRITE_SCOPE} or start the MCP server with `--scope {WRITE_SCOPE}` before using operator mode."
                    ),
                );
            }
            steps
        }
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
    fn auth_status_next_steps_use_write_scope_for_operator_default_scope() {
        let steps = auth_next_steps(
            CapabilityProfile::Operator,
            "https://www.googleapis.com/auth/drive",
            false,
            None,
        );

        assert!(
            steps
                .iter()
                .any(|step| step.contains("auth login --write-scope"))
        );
        assert!(
            steps
                .iter()
                .any(|step| step.contains("GOOGLE_SEARCH_CONSOLE_MCP_SCOPE"))
        );
    }

    #[test]
    fn auth_status_next_steps_require_search_console_scope() {
        let steps = auth_next_steps(
            CapabilityProfile::ReadOnly,
            "https://www.googleapis.com/auth/drive",
            true,
            Some(true),
        );

        assert!(
            steps
                .iter()
                .any(|step| step.contains("GOOGLE_SEARCH_CONSOLE_MCP_SCOPE"))
        );
    }

    #[test]
    fn mcp_login_command_scope_repairs_bad_read_only_scope() {
        assert_eq!(
            login_scope_for_mcp_command("https://www.googleapis.com/auth/drive", false),
            DEFAULT_SCOPE
        );
        assert_eq!(
            login_scope_for_mcp_command(WRITE_SCOPE, false),
            DEFAULT_SCOPE
        );
        assert_eq!(
            login_scope_for_mcp_command("https://www.googleapis.com/auth/drive", true),
            WRITE_SCOPE
        );
    }

    #[test]
    fn mcp_status_counts_selected_non_adc_auth_as_credential_material() {
        assert!(!credential_material_detected_for_auth_source(
            AuthSource::GoogleDefaultProviderChain,
            false
        ));
        assert!(credential_material_detected_for_auth_source(
            AuthSource::GoogleDefaultProviderChain,
            true
        ));
        assert!(credential_material_detected_for_auth_source(
            AuthSource::ServiceAccountJsonPath,
            false
        ));
        assert!(credential_material_detected_for_auth_source(
            AuthSource::ServiceAccountJsonEnv,
            false
        ));
        assert!(credential_material_detected_for_auth_source(
            AuthSource::OAuthRefreshToken,
            false
        ));
    }

    #[test]
    fn mcp_after_login_calls_out_repaired_runtime_scope() {
        let instruction = after_login_instruction_with_env(
            false,
            "https://www.googleapis.com/auth/drive",
            DEFAULT_SCOPE,
            None,
        );

        assert!(instruction.contains("GOOGLE_SEARCH_CONSOLE_MCP_SCOPE"));
        assert!(instruction.contains(DEFAULT_SCOPE));
        assert!(instruction.contains("--scope"));
    }

    #[test]
    fn mcp_after_login_calls_out_stale_env_for_explicit_scope() {
        let custom_scope = "https://www.googleapis.com/auth/webmasters.readonly,https://www.googleapis.com/auth/userinfo.email";
        let instruction = after_login_instruction_with_env(
            false,
            custom_scope,
            custom_scope,
            Some("https://www.googleapis.com/auth/drive"),
        );

        assert!(instruction.contains("GOOGLE_SEARCH_CONSOLE_MCP_SCOPE"));
        assert!(instruction.contains(custom_scope));
        assert!(instruction.contains("--scope"));
    }

    #[test]
    fn compact_search_analytics_response_shapes_rows_and_receipt() {
        let upstream = json!({
            "responseAggregationType": "byPage",
            "rows": [
                {
                    "keys": ["https://www.example.com/a", "rust mcp"],
                    "clicks": 12,
                    "impressions": 240,
                    "ctr": 0.05,
                    "position": 7.2
                },
                {
                    "keys": ["https://www.example.com/b", "search console"],
                    "clicks": 3,
                    "impressions": 80,
                    "ctr": 0.0375,
                    "position": 11.0
                }
            ]
        });
        let compact = compact_search_analytics_response(
            upstream,
            &["page".to_string(), "query".to_string()],
            Some(2),
            Some(4),
        );

        assert_eq!(compact["summary"]["row_count"], json!(2));
        assert_eq!(compact["summary"]["start_row"], json!(4));
        assert_eq!(compact["summary"]["requested_row_limit"], json!(2));
        assert_eq!(compact["summary"]["next_start_row"], json!(6));
        assert_eq!(compact["summary"]["has_more_hint"], json!(true));
        assert_eq!(
            compact["summary"]["response_aggregation_type"],
            json!("byPage")
        );
        assert_eq!(
            compact["columns"],
            json!(["page", "query", "clicks", "impressions", "ctr", "position"])
        );
        assert_eq!(
            compact["rows"][0]["page"],
            json!("https://www.example.com/a")
        );
        assert_eq!(compact["rows"][0]["query"], json!("rust mcp"));
        assert_eq!(compact["rows"][0]["clicks"], json!(12));
        assert_eq!(compact["rows"][0]["position"], json!(7.2));
    }

    #[test]
    fn compact_search_analytics_response_handles_missing_rows() {
        let compact =
            compact_search_analytics_response(json!({}), &["page".to_string()], Some(10), None);

        assert_eq!(compact["summary"]["row_count"], json!(0));
        assert_eq!(compact["summary"]["has_more_hint"], json!(false));
        assert_eq!(compact["rows"], json!([]));
    }
}
