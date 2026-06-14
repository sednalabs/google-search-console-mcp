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
cargo run -- --print-tools
```

Use hosted GitHub validation as the shared proof surface for public changes whenever available.
