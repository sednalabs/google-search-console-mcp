//! Shared error model.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SearchConsoleError {
    #[error("invalid {field}: {message}")]
    InvalidArgument {
        field: &'static str,
        message: String,
    },

    #[error("authentication bootstrap failed: {0}")]
    AuthBootstrap(String),

    #[error("token provider error: {0}")]
    TokenProvider(#[from] gcp_auth::Error),

    #[error("http client transport error: {0}")]
    Transport(#[from] reqwest::Error),

    #[error("failed to parse upstream JSON: {0}")]
    UpstreamJson(#[from] serde_json::Error),

    #[error("upstream API request failed with status {status}: {message}")]
    UpstreamApi { status: u16, message: String },

    #[error("tool '{tool}' is blocked by capability profile '{profile}'")]
    PolicyDenied { profile: String, tool: String },

    #[error(transparent)]
    Scratchpad(#[from] mcp_toolkit_scratchpad::ScratchpadError),
}

impl SearchConsoleError {
    pub fn invalid(field: &'static str, message: impl Into<String>) -> Self {
        Self::InvalidArgument {
            field,
            message: message.into(),
        }
    }

    pub fn policy_denied(profile: impl Into<String>, tool: impl Into<String>) -> Self {
        Self::PolicyDenied {
            profile: profile.into(),
            tool: tool.into(),
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidArgument { .. } => "INVALID_PARAMS",
            Self::AuthBootstrap(_) | Self::TokenProvider(_) => "AUTHENTICATION_FAILED",
            Self::Transport(_) => "UPSTREAM_TRANSPORT_ERROR",
            Self::UpstreamJson(_) => "UPSTREAM_RESPONSE_PARSE_ERROR",
            Self::UpstreamApi { status, .. } if *status >= 500 => "UPSTREAM_UNAVAILABLE",
            Self::UpstreamApi { .. } => "UPSTREAM_REJECTED",
            Self::PolicyDenied { .. } => "POLICY_DENIED",
            Self::Scratchpad(err) => err.code(),
        }
    }

    pub fn reason(&self) -> &'static str {
        match self {
            Self::InvalidArgument { .. } => "invalid_params",
            Self::AuthBootstrap(_) | Self::TokenProvider(_) => "auth_failed",
            Self::Transport(_) => "upstream_transport",
            Self::UpstreamJson(_) => "upstream_response_invalid",
            Self::UpstreamApi { status, .. } if *status >= 500 => "upstream_unavailable",
            Self::UpstreamApi { .. } => "upstream_rejected",
            Self::PolicyDenied { .. } => "policy_denied",
            Self::Scratchpad(err) => err.reason(),
        }
    }

    pub fn category(&self) -> &'static str {
        match self {
            Self::InvalidArgument { .. } => "validation",
            Self::AuthBootstrap(_) | Self::TokenProvider(_) => "auth",
            Self::Transport(_) => "transport",
            Self::UpstreamJson(_) => "upstream_parse",
            Self::UpstreamApi { .. } => "upstream_api",
            Self::PolicyDenied { .. } => "policy",
            Self::Scratchpad(err) => err.category(),
        }
    }

    pub fn hint(&self) -> Option<&'static str> {
        match self {
            Self::InvalidArgument { .. } => {
                Some("Check the tool argument schema and required fields.")
            }
            Self::AuthBootstrap(_) | Self::TokenProvider(_) => Some(
                "Configure Google Application Default Credentials or OAuth refresh-token settings with a Search Console scope.",
            ),
            Self::Transport(_) => Some("Check network connectivity and upstream API availability."),
            Self::UpstreamJson(_) => {
                Some("Retry later; upstream payload may be transiently malformed.")
            }
            Self::UpstreamApi { status, .. } if *status == 403 => {
                Some("Verify Search Console property access and OAuth scope permissions.")
            }
            Self::UpstreamApi { status, .. } if *status == 429 => Some("Retry with backoff."),
            Self::UpstreamApi { status, .. } if *status >= 500 => {
                Some("Upstream may be unavailable; retry with backoff.")
            }
            Self::UpstreamApi { .. } => None,
            Self::PolicyDenied { .. } => Some(
                "Switch capability profile to operator only for intentional sitemap/site mutations.",
            ),
            Self::Scratchpad(err) => err.hint(),
        }
    }
}
