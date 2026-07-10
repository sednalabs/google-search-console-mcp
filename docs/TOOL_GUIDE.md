# Tool Guide

## Setup Tools

Use `gsc_get_started` immediately after install. It returns the recommended auth flow, safe
starter tools, and credential options without making an upstream Google request.

Use `gsc_auth_status` to inspect auth configuration. Set `verify_token=true` when you want to prove
token acquisition, and set `verify_access=true` when you also want the low-cost Search Console
`sites.list` access probe. The response separates `token_check`, `access_check`,
`operator_scope_check`, `adc_quota_project`, and `runtime_quota_project`; the tool never returns
the token. `adc_quota_project` is read from the selected ADC file, while
`runtime_quota_project` reflects the optional server runtime quota-project header setting.

Use `gsc_auth_login_command` for an Application Default Credentials login helper. The `command`
field is argv, `shell_command` is the copyable shell string, and both target a Search
Console-specific gcloud config directory by default so sibling Google MCPs keep their own tokens and
scopes. The response also includes the shared Google MCP headless, client-id-file, quota-project,
API-enable, selected ADC path, scope, `shared_adc`, `next_steps`, `notes`, and `after_login` fields.
Set `shared_adc=true` only when you intentionally want the conventional shared gcloud ADC file; set
`GOOGLE_SEARCH_CONSOLE_MCP_SHARED_ADC=true` or start the server with `--shared-adc` when the runtime
should use that shared file. Set `write_scope=true` only when preparing to run operator tools.

## Read Tools

Use `gsc_sites_list` first to discover exact Search Console property strings. URL-prefix properties
must preserve the trailing slash. Domain properties use `sc-domain:example.com`.

Use `gsc_search_analytics_query` for performance rows. The request supports the official Search
Analytics dimensions and filter group structure. `row_limit` is validated against the documented
maximum of 25,000 rows.

Dimension compatibility is validated locally before the Google API call. In particular:

- `search_type=googleNews` and `search_type=discover` do not accept the `query` dimension
- `data_state=hourly_all` requires the `hour` dimension
- `hour` cannot be combined with `date`; use `hour` alone for hourly rows

Use `gsc_url_inspection_index_inspect` for the Google-indexed status of a URL. The Google API does
not provide live indexability testing through this method.

Use `gsc_sitemaps_list` and `gsc_sitemap_get` to inspect submitted sitemap metadata.

## Scratchpad Tools

Use the scratchpad flow when Search Analytics evidence is too large or too iterative for a single
tool response:

1. `gsc_scratchpad_open_session`
2. `gsc_scratchpad_ingest_search_analytics`
3. `gsc_scratchpad_query`
4. `gsc_scratchpad_export_evidence_bundle`
5. `gsc_scratchpad_close_session`

Scratchpad SQL is guarded by the toolkit read-only policy. Use `SELECT`, `WITH`, `DESCRIBE`,
`SUMMARIZE`, or `EXPLAIN`; file/external scan helpers and multi-statement SQL are rejected.

The Search Analytics ingest tool creates typed columns for the requested dimensions plus `clicks`,
`impressions`, `ctr`, and `position`. Use `append=true` only when the target table already exists
with the same columns.

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

Before using an operator tool, call `gsc_auth_status` with `verify_token=true` and
`verify_access=true`, then confirm `token_check.ok`, `access_check.ok`, and
`operator_scope_check.ok` are all `true`. If the operator-scope check is false, re-run the ADC login
command returned by `gsc_auth_login_command` with `write_scope=true`.
