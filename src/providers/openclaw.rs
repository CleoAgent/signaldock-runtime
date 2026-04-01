//! OpenClaw provider — delivers messages via /hooks/agent.
//!
//! Detection: ~/.openclaw/openclaw.json with hooks.enabled = true
//! Delivery: POST http://127.0.0.1:{port}/hooks/agent

use anyhow::{Context, Result};
use super::provider::*;

pub struct OpenClawProvider {
    hooks_url: String,
    hooks_token: String,
    port: u16,
}

impl OpenClawProvider {
    pub fn new(hooks_url: String, hooks_token: String, port: u16) -> Self {
        Self { hooks_url, hooks_token, port }
    }

    pub fn new_default() -> Self {
        Self {
            hooks_url: "http://127.0.0.1:18789/hooks/agent".into(),
            hooks_token: String::new(),
            port: 18789,
        }
    }
}

impl Provider for OpenClawProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            name: "openclaw",
            display_name: "OpenClaw",
            version: "2026.x",
            config_paths: &["~/.openclaw/openclaw.json"],
            docs_url: "https://docs.openclaw.ai",
        }
    }

    fn detect() -> Option<Box<dyn Provider>> {
        let home = dirs::home_dir()?;
        let config_path = home.join(".openclaw/openclaw.json");
        let content = std::fs::read_to_string(&config_path).ok()?;
        let json: serde_json::Value = serde_json::from_str(&content).ok()?;

        // Must have hooks enabled
        let enabled = json.get("hooks")?.get("enabled")?.as_bool()?;
        if !enabled { return None; }

        let port = json.get("gateway")
            .and_then(|g| g.get("port"))
            .and_then(|p| p.as_u64())
            .unwrap_or(18789) as u16;

        let token = json.get("hooks")
            .and_then(|h| h.get("token"))
            .and_then(|t| t.as_str())?
            .to_string();

        eprintln!("[signaldock] Detected OpenClaw on port {} with hooks enabled", port);

        Some(Box::new(Self {
            hooks_url: format!("http://127.0.0.1:{}/hooks/agent", port),
            hooks_token: token,
            port,
        }))
    }

    fn deliver(&self, msg: &Message) -> Result<DeliveryResult> {
        let payload = serde_json::json!({
            "message": format!("SignalDock message from @{}:\n\n{}", msg.from, msg.content),
            "name": "SignalDock",
            "deliver": true,
            "channel": "telegram",
            "wakeMode": "now"
        });

        let client = reqwest::blocking::Client::new();
        let resp = client.post(&self.hooks_url)
            .header("Authorization", format!("Bearer {}", self.hooks_token))
            .json(&payload)
            .timeout(std::time::Duration::from_secs(10))
            .send();

        match resp {
            Ok(r) if r.status().is_success() => {
                eprintln!("[signaldock] Delivered to OpenClaw hooks/agent (port {})", self.port);
                Ok(DeliveryResult::Delivered)
            }
            Ok(r) if r.status().is_server_error() => {
                Ok(DeliveryResult::Retry(format!("OpenClaw {} — server error", r.status())))
            }
            Ok(r) => Ok(DeliveryResult::Failed(format!("OpenClaw {} — check hooks config", r.status()))),
            Err(e) if e.is_timeout() || e.is_connect() => {
                Ok(DeliveryResult::Retry(format!("OpenClaw connection error: {}", e)))
            }
            Err(e) => Ok(DeliveryResult::Failed(format!("OpenClaw error: {}", e))),
        }
    }

    fn is_healthy(&self) -> bool {
        reqwest::blocking::Client::new()
            .get(format!("http://127.0.0.1:{}/health", self.port))
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}
