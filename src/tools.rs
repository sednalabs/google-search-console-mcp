use std::time::Instant;

use mcp_toolkit_core::tool_inventory::{ToolOperation, ToolSearchFilter, ToolSearchResponse};
use mcp_toolkit_core::tool_schema::tool_schema_snapshot_value;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::tool;
use rmcp::tool_router;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::auth_ux::{
    auth_login_cli_command, local_credential_material_detected, login_command_for_scope,
};
use crate::config::{
    CapabilityProfile, DEFAULT_SCOPE, WRITE_SCOPE, scope_allows_mutation, scope_allows_read,
};
use crate::contract;
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
}
