//! Generic providers: webhook, stdout, file.
//!
//! These are fallback/universal providers that work without
//! any specific agent platform installed.

use anyhow::Result;
use super::provider::*;

// ============================================================
// Webhook — POST JSON to any URL
// ============================================================

pub struct WebhookProvider {
    url: String,
}

impl WebhookProvider {
    pub fn new(url: String) -> Self { Self { url } }
}

impl Provider for WebhookProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            name: "webhook",
            display_name: "Webhook",
            version: "-",
            config_paths: &[],
            docs_url: "",
        }
    }

    fn detect() -> Option<Box<dyn Provider>> { None } // Never auto-detected

    fn deliver(&self, msg: &Message) -> Result<DeliveryResult> {
        let payload = serde_json::json!({
            "from": msg.from, "content": msg.content,
            "messageId": msg.id, "conversationId": msg.conversation_id,
            "contentType": msg.content_type, "createdAt": msg.created_at,
            "metadata": msg.metadata,
        });
        let client = reqwest::blocking::Client::new();
        match client.post(&self.url).json(&payload).timeout(std::time::Duration::from_secs(10)).send() {
            Ok(r) if r.status().is_success() => {
                eprintln!("[signaldock] Delivered to webhook");
                Ok(DeliveryResult::Delivered)
            }
            Ok(r) if r.status().is_server_error() => Ok(DeliveryResult::Retry(format!("{}", r.status()))),
            Ok(r) => Ok(DeliveryResult::Failed(format!("{}", r.status()))),
            Err(e) if e.is_timeout() || e.is_connect() => Ok(DeliveryResult::Retry(format!("{}", e))),
            Err(e) => Ok(DeliveryResult::Failed(format!("{}", e))),
        }
    }
}

// ============================================================
// Stdout — print JSON to stdout (pipe to anything)
// ============================================================

pub struct StdoutProvider;

impl Provider for StdoutProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            name: "stdout",
            display_name: "Stdout",
            version: "-",
            config_paths: &[],
            docs_url: "",
        }
    }

    fn detect() -> Option<Box<dyn Provider>> { None }

    fn deliver(&self, msg: &Message) -> Result<DeliveryResult> {
        println!("{}", serde_json::to_string(&serde_json::json!({
            "from": msg.from, "content": msg.content,
            "messageId": msg.id, "conversationId": msg.conversation_id,
            "createdAt": msg.created_at,
        }))?);
        Ok(DeliveryResult::Delivered)
    }
}

// ============================================================
// File — write JSON files to a directory
// ============================================================

pub struct FileProvider {
    output_dir: String,
}

impl FileProvider {
    pub fn new(dir: String) -> Self { Self { output_dir: dir } }
}

impl Provider for FileProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            name: "file",
            display_name: "File Output",
            version: "-",
            config_paths: &[],
            docs_url: "",
        }
    }

    fn detect() -> Option<Box<dyn Provider>> { None }

    fn deliver(&self, msg: &Message) -> Result<DeliveryResult> {
        std::fs::create_dir_all(&self.output_dir)?;
        let path = std::path::Path::new(&self.output_dir).join(format!("{}.json", msg.id));
        std::fs::write(&path, serde_json::to_string_pretty(&serde_json::json!({
            "from": msg.from, "content": msg.content,
            "messageId": msg.id, "conversationId": msg.conversation_id,
            "contentType": msg.content_type, "createdAt": msg.created_at,
            "metadata": msg.metadata,
        }))?)?;
        eprintln!("[signaldock] Written to {}", path.display());
        Ok(DeliveryResult::Delivered)
    }
}
