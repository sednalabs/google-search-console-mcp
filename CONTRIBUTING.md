# Contributing

Thanks for helping improve Google Search Console MCP.

## Development

- Keep the default profile read-only.
- Keep new dependencies justified and lightweight.
- Keep modules aligned with `AGENTS.md`.
- Do not commit credentials, captured Google API tokens, raw OAuth client secrets, or private
  environment details.

Before opening a pull request, run:

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo run --quiet -- auth command | grep -Fx -- 'gcloud auth application-default login --scopes=https://www.googleapis.com/auth/webmasters.readonly'
GOOGLE_SEARCH_CONSOLE_MCP_SCOPE=https://www.googleapis.com/auth/drive cargo run --quiet -- auth command | grep -Fx -- 'gcloud auth application-default login --scopes=https://www.googleapis.com/auth/webmasters.readonly'
GOOGLE_SEARCH_CONSOLE_MCP_SCOPE=https://www.googleapis.com/auth/drive cargo run --quiet -- auth login --dry-run | grep -Fx -- 'gcloud auth application-default login --scopes=https://www.googleapis.com/auth/webmasters.readonly'
GOOGLE_SEARCH_CONSOLE_MCP_SCOPE=https://www.googleapis.com/auth/webmasters cargo run --quiet -- auth command | grep -Fx -- 'gcloud auth application-default login --scopes=https://www.googleapis.com/auth/webmasters.readonly'
custom_scope='https://www.googleapis.com/auth/webmasters.readonly,https://www.googleapis.com/auth/userinfo.email'
GOOGLE_SEARCH_CONSOLE_MCP_SCOPE="$custom_scope" cargo run --quiet -- auth command | grep -Fx -- "gcloud auth application-default login '--scopes=https://www.googleapis.com/auth/webmasters.readonly,https://www.googleapis.com/auth/userinfo.email'"
GOOGLE_SEARCH_CONSOLE_MCP_SCOPE="$custom_scope" cargo run --quiet -- auth command --scope "$custom_scope" | grep -Fx -- "gcloud auth application-default login '--scopes=https://www.googleapis.com/auth/webmasters.readonly,https://www.googleapis.com/auth/userinfo.email'"
if cargo run --quiet -- auth command --write-scope --scope "$custom_scope" >/tmp/gsc-auth-command.out 2>/tmp/gsc-auth-command.err; then echo 'expected --write-scope plus explicit --scope to fail' >&2; exit 1; fi
grep -F -- '--write-scope' /tmp/gsc-auth-command.err >/dev/null
cargo run --quiet -- auth command --write-scope | grep -Fx -- 'gcloud auth application-default login --scopes=https://www.googleapis.com/auth/webmasters'
cargo run --quiet -- auth command --client-id-file /tmp/client_id.json | grep -Fx -- 'gcloud auth application-default login --scopes=https://www.googleapis.com/auth/webmasters.readonly --client-id-file /tmp/client_id.json'
cargo run -- auth status --json | jq -e '.verification.status == "not_checked" and .ready == false'
cargo run --quiet -- auth status --service-account-json-path /tmp/does-not-exist-google-search-console-mcp.json --json | jq -e '.verification.status == "config_error" and .credential_material_detected == false'
cargo run -- --print-tools | jq -e 'index("gsc_sitemap_submit") | not'
cargo run -- --profile operator --print-tools | jq -e 'index("gsc_sitemap_submit") != null'
cargo run -- --print-tool-schema | jq -e '.tools | map(.name) | index("gsc_sitemap_submit") | not'
cargo run -- --profile operator --print-tool-schema | jq -e '.tools | map(.name) | index("gsc_sitemap_submit") != null'
```

Use hosted GitHub validation as the shared proof surface for public changes whenever available.
