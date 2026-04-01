//! Platform adapters for SignalDock Runtime.
//!
//! Each adapter implements the `PlatformAdapter` trait to deliver
//! incoming SignalDock messages to a specific agent platform.
//!
//! To add a new adapter:
//! 1. Create `src/adapters/my_platform.rs`
//! 2. Implement `PlatformAdapter` trait
//! 3. Add `mod my_platform;` here
//! 4. Register in `create()` below

pub mod base;
mod openclaw;
mod webhook;
mod stdout;
mod file_output;

pub use base::{PlatformAdapter, Message, DeliveryResult};

use anyhow::{Context, Result};
use crate::config::Config;

/// Auto-detect which agent platform is running on this machine.
pub fn detect() -> String {
    // OpenClaw: check for config with hooks enabled
    if let Some(home) = dirs::home_dir() {
        let oc_path = home.join(".openclaw/openclaw.json");
        if oc_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&oc_path) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                    if json.get("hooks").and_then(|h| h.get("enabled")).and_then(|e| e.as_bool()) == Some(true) {
                        eprintln!("[signaldock] Auto-detected: OpenClaw (hooks enabled)");
                        return "openclaw".into();
                    }
                }
            }
            eprintln!("[signaldock] Found OpenClaw but hooks not enabled — use stdout");
        }

        // Claude Code
        if home.join(".claude").exists() {
            eprintln!("[signaldock] Auto-detected: Claude Code");
            return "file".into(); // Write to .claude/messages/
        }

        // Cursor
        if home.join(".cursor").exists() {
            eprintln!("[signaldock] Auto-detected: Cursor");
            return "file".into();
        }
    }

    eprintln!("[signaldock] No platform detected — using stdout");
    "stdout".into()
}

/// Create an adapter from config.
pub fn create(config: &Config) -> Result<Box<dyn PlatformAdapter>> {
    match config.platform.as_str() {
        "openclaw" => {
            let a = openclaw::OpenClawAdapter::from_auto_detect()
                .context("Failed to configure OpenClaw adapter")?;
            Ok(Box::new(a))
        }
        "webhook" => {
            let url = config.webhook_url.as_ref()
                .context("--webhook URL required for webhook platform")?;
            Ok(Box::new(webhook::WebhookAdapter::new(url.clone())))
        }
        "file" => {
            let dir = config.file_output_dir.clone()
                .unwrap_or_else(|| {
                    dirs::home_dir()
                        .unwrap_or_default()
                        .join(".signaldock/messages")
                        .to_string_lossy()
                        .to_string()
                });
            Ok(Box::new(file_output::FileAdapter::new(dir)))
        }
        "stdout" | _ => Ok(Box::new(stdout::StdoutAdapter)),
    }
}
