use mcp_toolkit_core::tool_inventory::{
    ToolCapability, ToolDiscoveryMetadata, ToolInventory, ToolInventoryError,
};

pub(crate) fn build_tool_inventory() -> Result<ToolInventory, ToolInventoryError> {
    ToolInventory::from_capabilities(tool_capabilities())
}

fn tool_capabilities() -> Vec<ToolCapability> {
    vec![
        cap(
            "find_tools",
            "discovery",
            true,
            "Search Google Search Console MCP tools by keyword, group, and read-only status.",
            [
                "tool_search",
                "deferred",
                "discover",
                "tools",
                "openai",
                "search-console",
            ],
        ),
        cap(
            "gsc_get_started",
            "setup",
            true,
            "Return the recommended first-run flow, credential options, and safe starter tools.",
            ["google", "search-console", "setup", "first-run", "help"],
        ),
        cap(
            "gsc_auth_status",
            "setup",
            true,
            "Explain configured Google auth source and optionally verify token acquisition.",
            ["google", "oauth", "auth", "credentials", "status"],
        ),
        cap(
            "gsc_auth_login_command",
            "setup",
            true,
            "Return a copyable gcloud Application Default Credentials login command.",
            ["google", "oauth", "gcloud", "adc", "login"],
        ),
        cap(
            "gsc_sites_list",
            "sites",
            true,
            "List Search Console properties visible to the authenticated Google account.",
            ["google", "search-console", "sites", "properties", "list"],
        ),
        cap(
            "gsc_site_get",
            "sites",
            true,
            "Get permission metadata for one Search Console property.",
            ["google", "search-console", "site", "property", "permission"],
        ),
        cap(
            "gsc_search_analytics_query",
            "search_analytics",
            true,
            "Query Search Console performance rows for a property and date range.",
            [
                "google",
                "search-console",
                "search-analytics",
                "query",
                "clicks",
                "impressions",
                "ctr",
                "position",
            ],
        ),
        cap(
            "gsc_scratchpad_open_session",
            "scratchpad",
            true,
            "Open or refresh a bounded DuckDB scratchpad session.",
            ["scratchpad", "duckdb", "session", "open", "evidence"],
        ),
        cap(
            "gsc_scratchpad_close_session",
            "scratchpad",
            true,
            "Close a scratchpad session and remove its local database.",
            ["scratchpad", "duckdb", "session", "close", "cleanup"],
        ),
        cap(
            "gsc_scratchpad_list_sessions",
            "scratchpad",
            true,
            "List active scratchpad sessions.",
            ["scratchpad", "duckdb", "session", "list"],
        ),
        cap(
            "gsc_scratchpad_list_tables",
            "scratchpad",
            true,
            "List tables in a scratchpad session.",
            ["scratchpad", "duckdb", "tables", "schema"],
        ),
        cap(
            "gsc_scratchpad_drop_table",
            "scratchpad",
            true,
            "Drop one table from a scratchpad session.",
            ["scratchpad", "duckdb", "drop", "table"],
        ),
        cap(
            "gsc_scratchpad_query",
            "scratchpad",
            true,
            "Run bounded read-only DuckDB SQL against scratchpad tables.",
            ["scratchpad", "duckdb", "sql", "query", "evidence"],
        ),
        cap(
            "gsc_scratchpad_ingest_search_analytics",
            "scratchpad",
            true,
            "Fetch Search Analytics rows and ingest them into a scratchpad table.",
            [
                "scratchpad",
                "search-analytics",
                "ingest",
                "clicks",
                "impressions",
                "evidence",
            ],
        ),
        cap(
            "gsc_scratchpad_export_evidence_bundle",
            "scratchpad",
            true,
            "Export a bounded markdown evidence bundle from scratchpad tables.",
            ["scratchpad", "evidence", "export", "markdown", "bundle"],
        ),
        cap(
            "gsc_url_inspection_index_inspect",
            "url_inspection",
            true,
            "Inspect Google-indexed URL status for a Search Console property.",
            [
                "google",
                "search-console",
                "url-inspection",
                "index",
                "coverage",
            ],
        ),
        cap(
            "gsc_sitemaps_list",
            "sitemaps",
            true,
            "List submitted sitemaps for a Search Console property.",
            ["google", "search-console", "sitemaps", "list"],
        ),
        cap(
            "gsc_sitemap_get",
            "sitemaps",
            true,
            "Get metadata for one submitted sitemap.",
            ["google", "search-console", "sitemap", "get", "metadata"],
        ),
        cap(
            "gsc_sitemap_submit",
            "sitemaps",
            false,
            "Submit a sitemap URL to Google Search Console.",
            ["google", "search-console", "sitemap", "submit", "mutation"],
        ),
        cap(
            "gsc_sitemap_delete",
            "sitemaps",
            false,
            "Delete a sitemap from a Search Console property.",
            ["google", "search-console", "sitemap", "delete", "mutation"],
        ),
        cap(
            "gsc_site_add",
            "sites",
            false,
            "Add a site to the authenticated user's Search Console account.",
            ["google", "search-console", "site", "add", "mutation"],
        ),
        cap(
            "gsc_site_delete",
            "sites",
            false,
            "Remove a site from the authenticated user's Search Console account.",
            ["google", "search-console", "site", "delete", "mutation"],
        ),
    ]
}

fn cap<const N: usize>(
    name: &'static str,
    group: &'static str,
    read_only: bool,
    description: &'static str,
    keywords: [&'static str; N],
) -> ToolCapability {
    ToolCapability::new(name)
        .with_group(group)
        .with_read_only(read_only)
        .with_discovery(ToolDiscoveryMetadata::new(description, keywords))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_toolkit_core::tool_inventory::{ToolInventoryPolicy, ToolOperation, ToolSearchFilter};

    #[test]
    fn inventory_search_finds_search_analytics_tool() {
        let inventory = build_tool_inventory().expect("inventory");
        let results = inventory.search(
            &ToolSearchFilter {
                query: Some("click impressions".to_string()),
                group: None,
                read_only: Some(true),
                limit: Some(10),
            },
            ToolOperation::List,
            &ToolInventoryPolicy::strict(),
        );
        assert!(
            results
                .iter()
                .any(|result| result.name == "gsc_search_analytics_query")
        );
    }

    #[test]
    fn inventory_search_finds_scratchpad_tools() {
        let inventory = build_tool_inventory().expect("inventory");
        let results = inventory.search(
            &ToolSearchFilter {
                query: Some("scratchpad evidence".to_string()),
                group: Some("scratchpad".to_string()),
                read_only: Some(true),
                limit: Some(10),
            },
            ToolOperation::List,
            &ToolInventoryPolicy::strict(),
        );
        assert!(
            results
                .iter()
                .any(|result| result.name == "gsc_scratchpad_ingest_search_analytics")
        );
    }
}
