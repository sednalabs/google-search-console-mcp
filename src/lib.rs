//! Rust MCP server for Google Search Console.

pub mod auth_ux;
pub mod config;
pub mod contract;
pub mod error;
pub mod gsc_client;
pub mod server;
pub mod tool_surface;
pub mod tools;

pub type McpError = rmcp::ErrorData;
