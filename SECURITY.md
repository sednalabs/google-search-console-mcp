# Security Policy

## Supported Versions

The `main` branch receives security fixes.

## Reporting a Vulnerability

Please report suspected vulnerabilities privately through the repository security advisory flow when
available.

## Credential Handling

This server is designed to avoid exposing Google credentials:

- access tokens, refresh tokens, client secrets, and private keys must not be logged
- tool responses redact common secret-bearing substrings
- OAuth client files are referenced by path and are not returned to MCP clients
- mutating Search Console operations are blocked unless the `operator` profile is selected

Use the read-only OAuth scope unless sitemap/site mutation tools are explicitly needed.
