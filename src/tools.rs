use std::time::Instant;

use mcp_toolkit_core::tool_inventory::{ToolOperation, ToolSearchFilter, ToolSearchResponse};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::tool;
use rmcp::tool_router;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::config::WRITE_SCOPE;
use crate::contract;
use crate::gsc_client::SearchAnalyticsRequest;
use crate::server::SearchConsoleMcp;

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
            let tools = Self::tool_router_search_console().list_all();
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
                    "error": err.to_string()
                }),
            }
        } else {
            json!({ "checked": false })
        };
        let token_ok = token_check.get("ok").and_then(Value::as_bool);

        Ok(contract::success(
            json!({
                "auth_source": self.client.auth_source().as_str(),
                "scope": self.client.scope(),
                "profile": self.profile.as_str(),
                "operator_tools_enabled": self.profile.allows_mutation(),
                "quota_project_configured": self.client.quota_project_configured(),
                "detected_env": auth_env_presence(),
                "token_check": token_check,
                "next_steps": auth_next_steps(args.verify_token, token_ok),
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
        Ok(contract::success(
            json!({
                "command": format!("gcloud auth application-default login --scopes={scope}"),
                "scope": scope,
                "write_scope": args.write_scope,
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

fn auth_next_steps(verified: bool, token_ok: Option<bool>) -> Vec<&'static str> {
    match (verified, token_ok) {
        (false, _) => vec![
            "Run gsc_auth_status with verify_token=true when you are ready to prove credentials.",
            "If credentials are missing, call gsc_auth_login_command for the local ADC command.",
            "Call gsc_sites_list after auth is verified to discover exact property strings.",
        ],
        (true, Some(true)) => vec![
            "Call gsc_sites_list to discover exact property strings.",
            "Use gsc_search_analytics_query for Search Console performance data.",
        ],
        (true, Some(false)) | (true, None) => vec![
            "Call gsc_auth_login_command and run the returned command for local ADC.",
            "For service accounts, set GOOGLE_APPLICATION_CREDENTIALS or GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON_PATH.",
            "Ensure the authenticated principal has access to the Search Console property.",
        ],
    }
}
