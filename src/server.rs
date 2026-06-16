//! MCP protocol handler and tool router.

use std::future::Future;
use std::sync::Arc;

use mcp_toolkit_core::rmcp_models;
use mcp_toolkit_core::tool_inventory::{ToolInventory, ToolInventoryPolicy, ToolOperation};
use mcp_toolkit_core::tool_schema::tool_schema_snapshot_value;
use mcp_toolkit_observability::{EventContext, Level, emit_event, safe_text};
use mcp_toolkit_scratchpad::SharedScratchpadSessionManager;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, Implementation, ListToolsResult, PaginatedRequestParams,
    ProtocolVersion, ServerCapabilities, ServerInfo,
};
use rmcp::service::RequestContext;
use rmcp::{RoleServer, ServerHandler};
use serde_json::Value;

use crate::config::CapabilityProfile;
use crate::contract;
use crate::error::SearchConsoleError;
use crate::gsc_client::SearchConsoleClient;
use crate::tool_surface::build_tool_inventory;

#[derive(Clone)]
pub struct SearchConsoleMcp {
    pub client: Arc<SearchConsoleClient>,
    pub profile: CapabilityProfile,
    pub scratchpad_sessions: SharedScratchpadSessionManager,
    pub(crate) tool_inventory: ToolInventory,
    pub(crate) tool_inventory_policy: ToolInventoryPolicy,
    tool_router: ToolRouter<SearchConsoleMcp>,
}

impl SearchConsoleMcp {
    pub fn new(
        client: Arc<SearchConsoleClient>,
        profile: CapabilityProfile,
        scratchpad_sessions: SharedScratchpadSessionManager,
    ) -> Self {
        let tool_inventory =
            build_tool_inventory().expect("google-search-console-mcp tool inventory should build");
        Self {
            client,
            profile,
            scratchpad_sessions,
            tool_inventory,
            tool_inventory_policy: tool_inventory_policy_for_profile(profile),
            tool_router: Self::tool_router_search_console(),
        }
    }

    pub fn tool_names(&self) -> Vec<String> {
        self.visible_tools()
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect()
    }

    pub fn tool_schema_snapshot(&self) -> Value {
        tool_schema_snapshot_value(&self.visible_tools())
            .expect("registered tool definitions should serialize")
    }

    pub(crate) fn visible_tools(&self) -> Vec<rmcp::model::Tool> {
        self.tool_inventory.filter_tools(
            self.tool_router.list_all(),
            ToolOperation::List,
            &self.tool_inventory_policy,
            |tool| tool.name.as_ref(),
        )
    }

    fn is_tool_allowed(&self, tool_name: &str) -> bool {
        self.tool_inventory
            .is_allowed(tool_name, ToolOperation::Call, &self.tool_inventory_policy)
    }
}

impl ServerHandler for SearchConsoleMcp {
    fn get_info(&self) -> ServerInfo {
        rmcp_models::server_info(
            ProtocolVersion::V_2024_11_05,
            ServerCapabilities::builder().enable_tools().build(),
            Implementation::from_build_env(),
            Some(
                "Google Search Console MCP Rust server. Use read-only tools by default; sitemap/site mutations require operator profile."
                    .to_string(),
            ),
        )
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, rmcp::ErrorData>> + Send + '_ {
        let tools = self.visible_tools();
        std::future::ready(Ok(ListToolsResult {
            meta: None,
            tools,
            next_cursor: None,
        }))
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<CallToolResult, rmcp::ErrorData>> + Send + '_ {
        let tool_name = request.name.to_string();
        let tool_context = ToolCallContext::new(self, request, context);
        async move {
            if !self.is_tool_allowed(&tool_name) {
                let err =
                    SearchConsoleError::policy_denied(self.profile.as_str(), tool_name.clone());
                return Ok(contract::error(err, std::time::Instant::now()));
            }

            emit_event(
                Level::INFO,
                "gsc_mcp.tool.start",
                &EventContext::new().with_tool_name(&tool_name),
                &[
                    safe_text("tool", &tool_name),
                    safe_text("profile", self.profile.as_str()),
                ],
            );
            let result = self.tool_router.call(tool_context).await;
            emit_event(
                Level::INFO,
                "gsc_mcp.tool.finish",
                &EventContext::new().with_tool_name(&tool_name),
                &[safe_text("tool", &tool_name)],
            );
            result
        }
    }
}

pub(crate) fn tool_inventory_policy_for_profile(profile: CapabilityProfile) -> ToolInventoryPolicy {
    if profile.allows_mutation() {
        ToolInventoryPolicy::strict()
    } else {
        ToolInventoryPolicy::strict().with_read_only_only(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;

    async fn server(profile: CapabilityProfile) -> SearchConsoleMcp {
        let settings = Settings {
            profile,
            scope: crate::config::DEFAULT_SCOPE.to_string(),
            api_base_url: "https://www.googleapis.com/webmasters/v3".to_string(),
            inspection_base_url: "https://searchconsole.googleapis.com/v1".to_string(),
            http_timeout: std::time::Duration::from_secs(1),
            user_agent: "test".to_string(),
            oauth_client_secret_json: None,
            oauth_refresh_token: None,
            quota_project: None,
            service_account_json_path: None,
            service_account_json: None,
            max_row_limit: 25_000,
            scratchpad_root_dir: None,
            scratchpad_session_ttl: std::time::Duration::from_secs(10),
            scratchpad_max_sessions: 4,
            scratchpad_max_tables_per_session: 4,
            scratchpad_max_rows_per_session: 10_000,
            scratchpad_max_memory_mb: 64,
            scratchpad_query_timeout: std::time::Duration::from_secs(1),
            scratchpad_max_sql_bytes: 8_192,
            print_tools: false,
            print_tool_schema: false,
            command: None,
        };
        let client = Arc::new(
            SearchConsoleClient::from_settings(&settings)
                .await
                .expect("client"),
        );
        let scratchpad_sessions = Arc::new(
            mcp_toolkit_scratchpad::ScratchpadSessionManager::new(
                Arc::new(mcp_toolkit_scratchpad::DuckDbEngine::new().expect("engine")),
                mcp_toolkit_scratchpad::ScratchpadSessionConfig::new(
                    std::time::Duration::from_secs(10),
                    4,
                    4,
                    10_000,
                    64,
                ),
            )
            .expect("scratchpad manager"),
        );
        SearchConsoleMcp::new(client, profile, scratchpad_sessions)
    }

    #[tokio::test]
    async fn read_only_profile_blocks_mutating_tools() {
        let server = server(CapabilityProfile::ReadOnly).await;
        assert!(!server.is_tool_allowed("gsc_sitemap_submit"));
        assert!(server.is_tool_allowed("gsc_search_analytics_query"));
        assert!(server.is_tool_allowed("gsc_scratchpad_query"));
        assert!(server.is_tool_allowed("gsc_scratchpad_ingest_search_analytics"));
        assert!(!server.is_tool_allowed("gsc_scratchpad_set_runtime_limits"));
    }

    #[tokio::test]
    async fn operator_profile_allows_mutating_tools() {
        let server = server(CapabilityProfile::Operator).await;
        assert!(server.is_tool_allowed("gsc_sitemap_submit"));
    }

    #[tokio::test]
    async fn read_only_profile_hides_mutating_tools_from_list_surface() {
        let server = server(CapabilityProfile::ReadOnly).await;
        let names = server.tool_names();
        assert!(names.contains(&"gsc_search_analytics_query".to_string()));
        assert!(names.contains(&"gsc_scratchpad_query".to_string()));
        assert!(names.contains(&"gsc_scratchpad_ingest_search_analytics".to_string()));
        assert!(!names.contains(&"gsc_sitemap_submit".to_string()));
        assert!(!names.contains(&"gsc_site_delete".to_string()));
        assert!(!names.contains(&"gsc_scratchpad_set_runtime_limits".to_string()));
    }

    #[tokio::test]
    async fn operator_profile_lists_mutating_tools() {
        let server = server(CapabilityProfile::Operator).await;
        let names = server.tool_names();
        assert!(names.contains(&"gsc_sitemap_submit".to_string()));
        assert!(names.contains(&"gsc_site_delete".to_string()));
    }
}
