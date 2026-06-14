# AGENTS.md - google-search-console-mcp

## Scope

- Applies to this repository.

## Operating intent

- Provide a lightweight Rust MCP server for Google Search Console.
- Keep the default runtime read-only and local-credential friendly.
- Prefer direct Google REST API calls over heavyweight client frameworks.
- Never print, log, or return Google access tokens, refresh tokens, client secrets, or raw
  credential files.

## Architecture boundaries

- `main.rs`: bootstrap, CLI parsing, stdio transport.
- `server.rs`: MCP protocol handler and tool routing.
- `tools.rs`: MCP argument structs and tool handlers.
- `gsc_client.rs`: authenticated Search Console REST API adapter.
- `config.rs`: settings and capability profile.
- `contract.rs`: Contract V1 response envelopes.
- `error.rs`: shared error model.
- `tool_surface.rs`: tool inventory and discovery metadata.

## Safety

- Default profile is `read_only`.
- Mutating Search Console operations must fail closed unless
  `GOOGLE_SEARCH_CONSOLE_MCP_PROFILE=operator`.
- Mutating operators require Google credentials with the
  `https://www.googleapis.com/auth/webmasters` scope.
- Read-only tools should work with `https://www.googleapis.com/auth/webmasters.readonly`.
- Keep property and URL validation conservative, especially for URL-prefix trailing slash
  semantics and `sc-domain:` properties.

## Quality bar

- Use `mcp-toolkit-rs` for shared MCP model helpers, tool inventory, schema snapshots, and
  observability where sensible.
- Add tests for redaction, profile gates, validation, tool discovery, and URL/path encoding when
  those paths change.
- Keep README and docs current when tool names, env vars, or safety posture changes.
- Keep private research notes and environment-specific credentials out of tracked public docs.
