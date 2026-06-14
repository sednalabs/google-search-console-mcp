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
cargo run -- --print-tools | jq -e 'index("gsc_sitemap_submit") | not'
cargo run -- --profile operator --print-tools | jq -e 'index("gsc_sitemap_submit") != null'
cargo run -- --print-tool-schema | jq -e '.tools | map(.name) | index("gsc_sitemap_submit") | not'
cargo run -- --profile operator --print-tool-schema | jq -e '.tools | map(.name) | index("gsc_sitemap_submit") != null'
```

Use hosted GitHub validation as the shared proof surface for public changes whenever available.
