//! Contract V1 response envelopes.

use std::time::Instant;

use rmcp::model::CallToolResult;
use serde_json::{Map, Value, json};

use crate::error::SearchConsoleError;
use mcp_toolkit_scratchpad::ScratchpadError;

pub fn elapsed_ms(started: Instant) -> u64 {
    let elapsed = started.elapsed().as_millis();
    if elapsed > u128::from(u64::MAX) {
        u64::MAX
    } else {
        elapsed as u64
    }
}

pub fn success(data: Value, started: Instant) -> CallToolResult {
    CallToolResult::structured(json!({
        "ok": true,
        "data": data,
        "meta": {
            "elapsed_ms": elapsed_ms(started),
        }
    }))
}

pub fn success_with_meta(data: Value, meta: Value, started: Instant) -> CallToolResult {
    let mut meta_map = match meta {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    meta_map.insert("elapsed_ms".to_string(), json!(elapsed_ms(started)));
    CallToolResult::structured(json!({
        "ok": true,
        "data": data,
        "meta": meta_map,
    }))
}

pub fn error(err: SearchConsoleError, started: Instant) -> CallToolResult {
    CallToolResult::structured(json!({
        "ok": false,
        "error": {
            "code": err.code(),
            "reason": err.reason(),
            "message": redact_secret_text(&err.to_string()),
            "category": err.category(),
            "hint": err.hint(),
        },
        "meta": {
            "elapsed_ms": elapsed_ms(started),
        }
    }))
}

pub fn scratchpad_error(err: ScratchpadError, started: Instant) -> CallToolResult {
    CallToolResult::structured(json!({
        "ok": false,
        "error": {
            "code": err.code(),
            "reason": err.reason(),
            "message": redact_secret_text(&err.to_string()),
            "category": err.category(),
            "detail": err.detail(),
            "hint": err.hint(),
        },
        "meta": {
            "elapsed_ms": elapsed_ms(started),
        }
    }))
}

pub fn redact_secret_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for token in input.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        if looks_secret_bearing(token) {
            out.push_str("[redacted]");
        } else {
            out.push_str(token);
        }
    }
    out
}

fn looks_secret_bearing(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    lower.contains("refresh_token")
        || lower.contains("access_token")
        || lower.contains("client_secret")
        || lower.contains("private_key")
        || lower.starts_with("ya29.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_secret_bearing_tokens() {
        let redacted =
            redact_secret_text("refresh_token=abc client_secret:xyz access_token ya29.secret ok");
        assert!(!redacted.contains("abc"));
        assert!(!redacted.contains("xyz"));
        assert!(!redacted.contains("ya29.secret"));
        assert!(redacted.contains("ok"));
    }
}
