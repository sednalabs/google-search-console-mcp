# Tool Guide

## Setup Tools

For humans at a shell, prefer the CLI auth flow before starting an MCP client. The normal path is
login, verify, then start or restart the MCP client:

```bash
google-search-console-mcp auth login
google-search-console-mcp auth status --verify-token
```

The helper includes `https://www.googleapis.com/auth/cloud-platform` in the underlying ADC command
because gcloud requires it for explicit Application Default Credentials scopes. The server runtime
scope remains Search Console read-only unless you intentionally use operator mode.

If verification says local ADC requires a quota project, attach one and verify again:

```bash
gcloud services enable searchconsole.googleapis.com --project YOUR_PROJECT
gcloud auth application-default set-quota-project YOUR_PROJECT
google-search-console-mcp auth status --verify-token
```

The server automatically uses the ADC file's `quota_project_id` for `x-goog-user-project`. Set
`GOOGLE_SEARCH_CONSOLE_MCP_QUOTA_PROJECT` only when a deployment needs an explicit override.

Search Console scopes may require a Google OAuth client id file. If Google rejects the scope,
create a Desktop OAuth client and rerun
`google-search-console-mcp auth login --client-id-file /path/to/client_id.json`.

Use `google-search-console-mcp auth doctor` when credentials are not working. It reports the
selected credential source, whether `gcloud` is available, whether the ADC file exists, and the
next safest action without printing tokens or credential file contents.

Inside MCP, use `gsc_get_started` immediately after install. It returns the recommended CLI auth
flow, safe starter tools, and credential options without making an upstream Google request.

Use `gsc_auth_status` to inspect auth configuration. Set `verify_token=true` when you want to prove
token acquisition and Search Console API access; the tool never returns the token. In operator
mode, verification also checks that the active token actually includes the write-capable Search
Console scope needed for sitemap/site mutations.

Use `gsc_auth_login_command` only when the user needs a copyable `gcloud` Application Default
Credentials command inside MCP. Servers already running with the operator profile return a
write-scope login command by default; set `write_scope=true` when preparing operator credentials
from a read-only session. Set `headless=true` for SSH or remote hosts where a browser cannot launch
locally. Operator tools also need the server started with
`GOOGLE_SEARCH_CONSOLE_MCP_PROFILE=operator` and
`GOOGLE_SEARCH_CONSOLE_MCP_SCOPE=https://www.googleapis.com/auth/webmasters`, or with
`--profile operator --scope https://www.googleapis.com/auth/webmasters` in the MCP launcher. Set
`client_id_file` when Google requires or the user needs a project-specific Google OAuth client id
file.

## Read Tools

Use `gsc_sites_list` first to discover exact Search Console property strings. URL-prefix properties
must preserve the trailing slash. Domain properties use `sc-domain:example.com`.

Use `gsc_search_analytics_query` for performance rows. The request supports the official Search
Analytics dimensions and filter group structure. `row_limit` is validated against the documented
maximum of 25,000 rows. Set `response_mode` to `compact` for broad SEO batches that need concise,
copy/export-friendly rows and a pagination receipt instead of raw Google row objects.

```json
{
  "site_url": "https://www.example.com/",
  "start_date": "2026-05-01",
  "end_date": "2026-05-31",
  "dimensions": ["page", "query"],
  "row_limit": 1000,
  "response_mode": "compact"
}
```

Use `gsc_url_inspection_index_inspect` for the Google-indexed status of a URL. The Google API does
not provide live indexability testing through this method.

Use `gsc_sitemaps_list` and `gsc_sitemap_get` to inspect submitted sitemap metadata.

## Scratchpad Tools

Use scratchpad tools when Search Analytics batches are too large to keep returning through the MCP
transcript. Scratchpad tools are available in the default `read_only` profile because they only write
to a local bounded DuckDB session, not to Google Search Console.

Start by ingesting one Search Analytics page into a table:

```json
{
  "session_id": "seo_batch",
  "table_name": "may_pages",
  "site_url": "https://www.example.com/",
  "start_date": "2026-05-01",
  "end_date": "2026-05-31",
  "dimensions": ["page", "query"],
  "row_limit": 25000
}
```

Then query the local table with restricted read-only SQL:

```json
{
  "session_id": "seo_batch",
  "sql": "SELECT page, SUM(clicks) AS clicks, SUM(impressions) AS impressions FROM may_pages GROUP BY page ORDER BY impressions DESC",
  "page_size": 100
}
```

Available scratchpad tools:

- `gsc_scratchpad_open_session`
- `gsc_scratchpad_release_session`
- `gsc_scratchpad_list_sessions`
- `gsc_scratchpad_list_tables`
- `gsc_scratchpad_query`
- `gsc_scratchpad_drop_table`
- `gsc_scratchpad_get_runtime_limits`
- `gsc_scratchpad_set_runtime_limits`
- `gsc_scratchpad_ingest_search_analytics`

Scratchpad SQL allows read-only `SELECT` and `WITH` queries plus DuckDB `DESCRIBE` and `SUMMARIZE`
helpers. It denies mutating statements, multiple statements, extension loading, and file/external
scan primitives.

## Operator Tools

The following tools are blocked unless `GOOGLE_SEARCH_CONSOLE_MCP_PROFILE=operator`:

- `gsc_sitemap_submit`
- `gsc_sitemap_delete`
- `gsc_site_add`
- `gsc_site_delete`

Operator tools also require Google credentials with the write scope:

```text
https://www.googleapis.com/auth/webmasters
```

For local browser auth, run `google-search-console-mcp --profile operator auth login`, then start
the server with `GOOGLE_SEARCH_CONSOLE_MCP_PROFILE=operator` and
`GOOGLE_SEARCH_CONSOLE_MCP_SCOPE=https://www.googleapis.com/auth/webmasters`, or configure the MCP
launcher command with `--profile operator --scope https://www.googleapis.com/auth/webmasters`.
`google-search-console-mcp auth login --write-scope` is still available when you want to mint
operator-capable credentials before changing the runtime profile.
For SSH or browser-forwarded hosts, add `--headless`; for a project-specific OAuth client, add
`--client-id-file /path/to/client_id.json`.
