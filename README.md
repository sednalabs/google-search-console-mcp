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

## Easy Login

For most users, use Application Default Credentials (ADC): login once with `gcloud`, then let the
MCP server reuse that local credential. Start with this:

```bash
google-search-console-mcp auth login
google-search-console-mcp auth status --verify-token
```

If verification says local ADC requires a quota project, attach a Google Cloud project to the ADC
file and verify again. The project must have the Search Console API enabled and your account must
be allowed to use it for quota:

```bash
gcloud services enable searchconsole.googleapis.com --project YOUR_PROJECT
gcloud auth application-default set-quota-project YOUR_PROJECT
google-search-console-mcp auth status --verify-token
```

Only use a Desktop OAuth client file if Google rejects the Search Console scope during login:

```bash
google-search-console-mcp auth login --client-id-file /path/to/client_id.json
```

After verification passes, restart any MCP client that keeps long-lived stdio server processes and
call `gsc_sites_list` to discover the exact Search Console property strings.

Useful auth commands:

```bash
# Show local auth state and suggested fixes.
google-search-console-mcp auth doctor

# SSH or remote hosts where the browser cannot launch locally.
google-search-console-mcp auth login --headless

# Prepare for operator-only sitemap/site mutation tools.
google-search-console-mcp auth login --write-scope
export GOOGLE_SEARCH_CONSOLE_MCP_PROFILE=operator
export GOOGLE_SEARCH_CONSOLE_MCP_SCOPE=https://www.googleapis.com/auth/webmasters
# Or put the same runtime state in your MCP launcher command:
# google-search-console-mcp --profile operator --scope https://www.googleapis.com/auth/webmasters

# Print the underlying gcloud command without running it.
google-search-console-mcp auth command
```

No command starts the stdio MCP server, preserving the normal MCP client launch path:

```bash
google-search-console-mcp
```

## MCP First Run

The server also exposes setup tools that do not return secrets:

- `gsc_get_started` shows the recommended first-run flow.
- `gsc_auth_status` reports credential source and can optionally verify token acquisition.
- `gsc_auth_login_command` returns a copyable `gcloud` Application Default Credentials command
  for clients that need setup help inside MCP.
- `gsc_sites_list` discovers exact Search Console property strings after auth works.

The CLI helper and setup tool both use the same low-friction Application Default Credentials
model. The underlying read-only ADC login command is:

```bash
gcloud auth application-default login \
  --scopes=https://www.googleapis.com/auth/cloud-platform,https://www.googleapis.com/auth/webmasters.readonly
```

If Google asks for a quota project after login, run
`gcloud auth application-default set-quota-project YOUR_PROJECT`, then call `gsc_auth_status` with
`verify_token=true`. After verification passes, restart any stdio MCP client that keeps long-lived
child processes.

## Authentication

The server uses the Google credential chain plus server-specific overrides. By default it requests
the read-only Search Console scope:

```text
https://www.googleapis.com/auth/webmasters.readonly
```

Supported credential sources, in the order selected by the server:

- Server-specific raw service-account JSON via `GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON`.
- Server-specific service-account file via
  `GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON_PATH`.
- OAuth refresh-token auth via
  `GOOGLE_SEARCH_CONSOLE_MCP_OAUTH_CLIENT_SECRET_JSON` and
  `GOOGLE_SEARCH_CONSOLE_MCP_OAUTH_REFRESH_TOKEN`.
- Application Default Credentials from `google-search-console-mcp auth login` or
  `gcloud auth application-default login`.

For operator-only sitemap/site mutations, use credentials that have:

```text
https://www.googleapis.com/auth/webmasters
```

The server never returns raw tokens, private keys, refresh tokens, or client secrets in tool
responses.

### Application Default Credentials

For read-only use:

```bash
google-search-console-mcp auth login
```

Equivalent underlying command:

```bash
gcloud auth application-default login \
  --scopes=https://www.googleapis.com/auth/cloud-platform,https://www.googleapis.com/auth/webmasters.readonly
```

The `cloud-platform` scope is included because `gcloud auth application-default login` requires it
when writing user credentials for ADC with explicit scopes. The MCP server still defaults to
read-only Search Console access.

If verification reports that local ADC requires a quota project, attach one to the ADC file. The
project must have the Search Console API enabled:

```bash
gcloud services enable searchconsole.googleapis.com --project YOUR_PROJECT
gcloud auth application-default set-quota-project YOUR_PROJECT
```

The server automatically uses the ADC file's `quota_project_id` as the `x-goog-user-project`
header. Set `GOOGLE_SEARCH_CONSOLE_MCP_QUOTA_PROJECT` only when you need to override that project
for a specific deployment.

With a project-specific OAuth client:

```bash
google-search-console-mcp auth login --client-id-file /path/to/client_id.json
```

For operator use:

```bash
google-search-console-mcp auth login --write-scope
export GOOGLE_SEARCH_CONSOLE_MCP_PROFILE=operator
export GOOGLE_SEARCH_CONSOLE_MCP_SCOPE=https://www.googleapis.com/auth/webmasters
# Or configure the MCP launcher command with:
# google-search-console-mcp --profile operator --scope https://www.googleapis.com/auth/webmasters
```

For SSH or browser-forwarded hosts, add `--headless`. To use a project-specific OAuth client,
pass `--client-id-file /path/to/client_id.json`.

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
| `GOOGLE_SEARCH_CONSOLE_MCP_QUOTA_PROJECT` | ADC `quota_project_id`, when present | Optional `x-goog-user-project` header override |
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
