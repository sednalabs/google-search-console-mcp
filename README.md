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

- `gsc_get_started` shows the recommended first-run flow.
- `gsc_auth_status` reports credential source and can optionally verify token acquisition.
- `gsc_auth_login_command` returns a copyable `gcloud` Application Default Credentials command.
- `gsc_sites_list` discovers exact Search Console property strings after auth works.

For local use, the lowest-friction path is usually:

```bash
gcloud auth application-default login \
  --scopes=https://www.googleapis.com/auth/webmasters.readonly
```

Then restart any stdio MCP client that keeps long-lived child processes and call
`gsc_auth_status` with `verify_token=true`.

## Authentication

The server uses the Google credential chain plus server-specific overrides. By default it requests
the read-only Search Console scope:

```text
https://www.googleapis.com/auth/webmasters.readonly
```

Supported credential sources:

- Application Default Credentials from `gcloud auth application-default login`.
- Standard service-account file via `GOOGLE_APPLICATION_CREDENTIALS`.
- Server-specific service-account file via
  `GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON_PATH`.
- Server-specific raw service-account JSON via `GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON`.
- OAuth refresh-token auth via
  `GOOGLE_SEARCH_CONSOLE_MCP_OAUTH_CLIENT_SECRET_JSON` and
  `GOOGLE_SEARCH_CONSOLE_MCP_OAUTH_REFRESH_TOKEN`.

For operator-only sitemap/site mutations, use credentials that have:

```text
https://www.googleapis.com/auth/webmasters
```

The server never returns raw tokens, private keys, refresh tokens, or client secrets in tool
responses.

### Application Default Credentials

For read-only use:

```bash
gcloud auth application-default login \
  --scopes=https://www.googleapis.com/auth/webmasters.readonly
```

For operator use:

```bash
gcloud auth application-default login \
  --scopes=https://www.googleapis.com/auth/webmasters
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
| `GOOGLE_SEARCH_CONSOLE_MCP_HTTP_TIMEOUT_MS` | `15000` | Upstream request timeout |
| `GOOGLE_SEARCH_CONSOLE_MCP_MAX_ROW_LIMIT` | `25000` | Maximum Search Analytics `rowLimit` |

## Tools

- `find_tools`
- `gsc_get_started`
- `gsc_auth_status`
- `gsc_auth_login_command`
- `gsc_sites_list`
- `gsc_site_get`
- `gsc_search_analytics_query`
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
