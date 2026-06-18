# Tool Guide

## Setup Tools

Use `gsc_get_started` immediately after install. It returns the recommended auth flow, safe
starter tools, and credential options without making an upstream Google request.

Use `gsc_auth_status` to inspect auth configuration. Set `verify_token=true` when you want to prove
token acquisition; the tool never returns the token.

Use `gsc_auth_login_command` for a copyable Application Default Credentials command. Set
`write_scope=true` only when preparing to run operator tools.

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
