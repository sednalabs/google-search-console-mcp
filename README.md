# Google Search Console MCP

Lightweight Rust MCP server for Google Search Console, built for the Sedna Labs Rust MCP
ecosystem and `mcp-toolkit-rs`.

The first release focuses on direct, low-overhead access to the official Search Console APIs:

- list and inspect Search Console properties
- query Search Analytics performance data
- inspect URL indexing status
- list and retrieve sitemap metadata
- submit/delete sitemaps and add/remove sites only when the operator profile is enabled
- expose `find_tools` metadata for deferred-loading and tool-search clients

## Install

```bash
cargo install --git https://github.com/sednalabs/google-search-console-mcp google-search-console-mcp
```

For local development:

```bash
cargo run -- --print-tools
```

## First Run

The server exposes setup tools that do not return secrets:

- `gsc_get_started`
- `gsc_auth_status`
- `gsc_auth_login_command`

For local use, ask the auth-login helper for a server-specific ADC command:

```text
gsc_auth_login_command { "quota_project": "<PROJECT_ID>" }
```

Run the returned `shell_command`. The helper uses Google Application Default
Credentials in a Search-Console-specific gcloud config directory and requests
the required `cloud-platform` ADC scope plus the read-only Search Console
scope. Keeping a server-specific ADC file prevents a login for another Google
MCP from replacing this server's refresh token or scope grant.

Then restart any stdio MCP client that keeps a long-lived child process and call:

```text
gsc_auth_status { "verify_token": true, "verify_access": true }
```

The status response separates `token_check`, `access_check`,
`operator_scope_check`, `adc_quota_project`, and `runtime_quota_project`.
For Search Console, `verify_access=true` uses the low-cost `sites.list` probe.
`adc_quota_project` describes the selected ADC file metadata; `runtime_quota_project`
describes the optional `GOOGLE_SEARCH_CONSOLE_MCP_QUOTA_PROJECT` header setting
that the server will send upstream.

After auth is proven:

1. `gsc_sites_list`
2. `gsc_search_analytics_query`
3. `gsc_url_inspection_index_inspect`
4. `gsc_sitemaps_list`
5. Operator-only sitemap/site mutations only after enabling the operator
   profile and confirming `operator_scope_check.ok`

## Authentication

The server uses the Google credential chain plus server-specific overrides. By default it requests
the read-only Search Console scope:

```text
https://www.googleapis.com/auth/webmasters.readonly
```

Supported credential sources, in precedence order:

- Server-specific raw service-account JSON via `GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON`.
- Server-specific service-account file via
  `GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON_PATH`.
- OAuth refresh-token auth via
  `GOOGLE_SEARCH_CONSOLE_MCP_OAUTH_CLIENT_SECRET_JSON` and
  `GOOGLE_SEARCH_CONSOLE_MCP_OAUTH_REFRESH_TOKEN`.
- Standard Google credentials selected by `GOOGLE_APPLICATION_CREDENTIALS`.
- Search Console-specific local ADC at
  `<user-config>/google-search-console-mcp/gcloud/application_default_credentials.json`.
- Conventional shared ADC only when explicitly enabled with
  `GOOGLE_SEARCH_CONSOLE_MCP_SHARED_ADC=true` or `--shared-adc`.

For operator-only sitemap/site mutations, use credentials that have:

```text
https://www.googleapis.com/auth/webmasters
```

Before submitting/deleting sitemaps or adding/removing sites, call `gsc_auth_status` with
`verify_token=true` and `verify_access=true`, then confirm:

- `token_check.ok` is `true`
- `access_check.ok` is `true`
- `operator_scope_check.ok` is `true`

The server never returns raw tokens, private keys, refresh tokens, or client secrets in tool
responses.

### Application Default Credentials

`gsc_auth_login_command` targets a Search Console-specific Cloud SDK config directory by default so
Ad Manager, GA4, and other Google MCPs can keep their own OAuth grants and scopes. Its `command`
field is an argv array and `shell_command` is the copyable shell string. It also returns headless,
client-id-file, quota-project, API-enable, selected ADC path, scope, `shared_adc`, `next_steps`,
`notes`, and `after_login` fields using the same Google MCP auth helper shape as other Google
servers. Set `shared_adc=true` only when you intentionally want the conventional shared gcloud ADC
file. To make the running server use that shared ADC file, also set
`GOOGLE_SEARCH_CONSOLE_MCP_SHARED_ADC=true` or start the binary with `--shared-adc`.

For read-only use:

```bash
CLOUDSDK_CONFIG="${XDG_CONFIG_HOME:-$HOME/.config}/google-search-console-mcp/gcloud" \
  gcloud auth application-default login \
  --scopes=https://www.googleapis.com/auth/cloud-platform,https://www.googleapis.com/auth/webmasters.readonly
```

For operator use:

```bash
CLOUDSDK_CONFIG="${XDG_CONFIG_HOME:-$HOME/.config}/google-search-console-mcp/gcloud" \
  gcloud auth application-default login \
  --scopes=https://www.googleapis.com/auth/cloud-platform,https://www.googleapis.com/auth/webmasters
```

### Service Accounts

For unattended deployments, create a service account, grant it access to the Search Console
property, then set one of:

```bash
export GOOGLE_APPLICATION_CREDENTIALS=/path/to/service-account.json
export GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON_PATH=/path/to/service-account.json
```

Use `GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON` only when your deployment platform cannot
mount a file and can provide the JSON as a sealed secret.

## Configuration

| Setting | Default | Purpose |
| --- | --- | --- |
| `GOOGLE_SEARCH_CONSOLE_MCP_PROFILE` | `read_only` | `read_only` or `operator` |
| `GOOGLE_SEARCH_CONSOLE_MCP_SCOPE` | `https://www.googleapis.com/auth/webmasters.readonly` | OAuth scope requested for ADC/OAuth refresh |
| `GOOGLE_APPLICATION_CREDENTIALS` | unset | Standard Google service-account credential path |
| `GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON_PATH` | unset | Server-specific service-account credential path |
| `GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON` | unset | Server-specific raw service-account JSON |
| `GOOGLE_SEARCH_CONSOLE_MCP_QUOTA_PROJECT` | unset | Optional `x-goog-user-project` header |
| `GOOGLE_SEARCH_CONSOLE_MCP_SHARED_ADC` | `false` | Use conventional shared gcloud ADC instead of the server-specific ADC file |
| `GOOGLE_SEARCH_CONSOLE_MCP_HTTP_TIMEOUT_MS` | `15000` | Upstream request timeout |
| `GOOGLE_SEARCH_CONSOLE_MCP_MAX_ROW_LIMIT` | `25000` | Maximum Search Analytics `rowLimit` |
| `GOOGLE_SEARCH_CONSOLE_MCP_SCRATCHPAD_SESSION_TTL_SECS` | `900` | Scratchpad session idle TTL |
| `GOOGLE_SEARCH_CONSOLE_MCP_SCRATCHPAD_MAX_SESSIONS` | `64` | Maximum active scratchpad sessions |
| `GOOGLE_SEARCH_CONSOLE_MCP_SCRATCHPAD_MAX_TABLES_PER_SESSION` | `32` | Maximum scratchpad tables per session |
| `GOOGLE_SEARCH_CONSOLE_MCP_SCRATCHPAD_MAX_ROWS_PER_SESSION` | `1000000` | Maximum scratchpad rows tracked per session |
| `GOOGLE_SEARCH_CONSOLE_MCP_SCRATCHPAD_MAX_MEMORY_MB` | `256` | DuckDB memory limit per scratchpad connection |
| `GOOGLE_SEARCH_CONSOLE_MCP_SCRATCHPAD_QUERY_TIMEOUT_MS` | `15000` | Scratchpad query timeout |
| `GOOGLE_SEARCH_CONSOLE_MCP_SCRATCHPAD_MAX_SQL_BYTES` | `65536` | Maximum SQL payload size for scratchpad queries |
| `GOOGLE_SEARCH_CONSOLE_MCP_SCRATCHPAD_ROOT_DIR` | OS temp dir | Scratchpad database root directory |

## Tools

- `find_tools`
- `gsc_get_started`
- `gsc_auth_status`
- `gsc_auth_login_command`
- `gsc_sites_list`
- `gsc_site_get`
- `gsc_search_analytics_query`
- `gsc_scratchpad_open_session`
- `gsc_scratchpad_close_session`
- `gsc_scratchpad_list_sessions`
- `gsc_scratchpad_list_tables`
- `gsc_scratchpad_drop_table`
- `gsc_scratchpad_query`
- `gsc_scratchpad_ingest_search_analytics`
- `gsc_scratchpad_export_evidence_bundle`
- `gsc_url_inspection_index_inspect`
- `gsc_sitemaps_list`
- `gsc_sitemap_get`
- `gsc_sitemap_submit` (`operator`)
- `gsc_sitemap_delete` (`operator`)
- `gsc_site_add` (`operator`)
- `gsc_site_delete` (`operator`)

All tool responses use a Contract V1 envelope:

```json
{
  "ok": true,
  "data": {},
  "meta": {
    "elapsed_ms": 12
  }
}
```

## Notes

Search Console URL-prefix properties must include their trailing slash, for example
`https://www.example.com/`. Domain properties use `sc-domain:example.com`.

Search Analytics result volume is bounded by Google Search Console API limits. The server validates
`rowLimit` against the documented 1 to 25,000 range and returns the upstream result as structured
JSON without inventing SEO scores or rankings.

The server also rejects several invalid Search Analytics combinations before the upstream request:

- `search_type=googleNews` and `search_type=discover` do not support the `query` dimension
- `data_state=hourly_all` requires the `hour` dimension
- `hour` cannot be combined with `date`; use `hour` alone for hourly rows

For larger evidence passes, use the scratchpad flow instead of returning every row through chat:
open a session, ingest Search Analytics rows into a named table, query with bounded read-only
DuckDB SQL, then export a compact markdown evidence bundle.
