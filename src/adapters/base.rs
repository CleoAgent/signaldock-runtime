//! Adapter trait — the SSOT interface for delivery transport mechanisms.
//!
//! Adapters are low-level: they take a URL/path and a JSON payload,
//! and deliver it. They don't know about SignalDock messages or providers.

use anyhow::Result;

/// Configuration for creating an adapter.
/// Currently unused — reserved for future adapter factory pattern.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum AdapterConfig {
    /// HTTP POST to a URL with optional auth header.
    Http { url: String, auth_header: Option<String> },
    /// Write JSON files to a directory.
    File { dir: String },
    /// Print JSON to stdout.
    Stdout,
}

/// Result of a delivery attempt at the transport level.
#[derive(Debug)]
pub enum TransportResult {
    Ok,
    RetryableError(String),
    PermanentError(String),
}

/// Low-level delivery adapter. Providers compose these internally.
pub trait Adapter: Send + Sync {
    /// Adapter name for logging. Implemented by all adapters for future
    /// observability use (tracing, structured logs, metrics tags).
    #[allow(dead_code)]
    fn name(&self) -> &str;

    /// Send a JSON payload to the target.
    fn send(&self, payload: &serde_json::Value) -> Result<TransportResult>;
}
