use anyhow::Result;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::sync::atomic::Ordering;
use tokio::time::{sleep, Duration};
use crate::config::Config;
use crate::health::HealthState;
use crate::adapters::providers::provider::{Provider, Message, DeliveryResult};

/// Run the poll receiver loop.
pub async fn run_poll(
    config: Config,
    provider: Box<dyn Provider>,
    interval_secs: u64,
    health: Option<Arc<HealthState>>,
) -> Result<()> {
    let seen = Arc::new(Mutex::new(load_seen(&config)?));
    let config = Arc::new(config);
    let provider: Arc<Box<dyn Provider>> = Arc::new(provider);

    let info = provider.info();
    eprintln!("[signaldock] Receiver started: @{} provider={} interval={}s",
        config.agent_id, info.display_name, interval_secs);

    // Mark as connected for health endpoint (polling mode is considered connected)
    if let Some(h) = health.as_ref() {
        h.connected.store(true, Ordering::Relaxed);
    }

    let mut consecutive_errors: u32 = 0;

    loop {
        match poll_once_inner(&config, &provider, &seen, &health).await {
            Ok(count) => {
                if count > 0 { eprintln!("[signaldock] Delivered {} messages", count); }
                consecutive_errors = 0;
            }
            Err(e) => {
                consecutive_errors += 1;
                eprintln!("[signaldock] Poll error ({}x): {}", consecutive_errors, e);
            }
        }

        let delay = match consecutive_errors {
            0 => Duration::from_secs(interval_secs),
            1..=5 => Duration::from_secs(30),
            6..=10 => Duration::from_secs(60),
            _ => Duration::from_secs(120),
        };
        sleep(delay).await;
    }
}

/// Run a single poll cycle without persistent seen-set state.
///
/// Used by the SSE receiver as a fallback when the SSE connection drops.
/// Creates a temporary seen-set for this cycle only.
pub async fn poll_once_standalone(
    config: &Config,
    provider: &Arc<Box<dyn Provider>>,
    health: &Option<Arc<HealthState>>,
) -> Result<usize> {
    let seen = Arc::new(Mutex::new(load_seen(config)?));
    let count = poll_once_inner(&Arc::new(config.clone()), provider, &seen, health).await?;
    save_seen(config, &seen)?;
    Ok(count)
}

async fn poll_once_inner(
    config: &Config,
    provider: &Arc<Box<dyn Provider>>,
    seen: &Arc<Mutex<HashSet<String>>>,
    health: &Option<Arc<HealthState>>,
) -> Result<usize> {
    let client = reqwest::Client::new();
    let resp = client.get(format!(
            "{}/messages/peek?limit=50&mentioned={}",
            config.api_base,
            urlencoding::encode(&config.agent_id),
        ))
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("X-Agent-Id", &config.agent_id)
        .header("User-Agent", format!(
            "signaldock-runtime/{} ({})",
            env!("CARGO_PKG_VERSION"),
            config.agent_id,
        ))
        .timeout(Duration::from_secs(10))
        .send().await?;

    if !resp.status().is_success() {
        anyhow::bail!("API returned {}", resp.status());
    }

    // Update last heartbeat on each successful poll
    if let Some(h) = health {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        h.last_heartbeat.store(now, Ordering::Relaxed);
    }

    let body: serde_json::Value = resp.json().await?;
    let raw_msgs = match body.get("data").and_then(|d| d.get("messages")).and_then(|m| m.as_array()) {
        Some(m) if !m.is_empty() => m,
        _ => return Ok(0),
    };

    let mut delivered = 0;
    let mut ack_ids = Vec::new();

    for raw in raw_msgs {
        let msg_id = raw.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let from = raw.get("fromAgentId").and_then(|v| v.as_str()).unwrap_or("");
        if from == config.agent_id || from.is_empty() || msg_id.is_empty() { continue; }

        {
            let mut s = seen.lock().map_err(|e| anyhow::anyhow!("seen lock poisoned: {e}"))?;
            if s.contains(msg_id) { continue; }
            s.insert(msg_id.to_string());
        }

        let msg = Message {
            id: msg_id.to_string(),
            from: from.to_string(),
            from_name: raw.get("fromAgentName").and_then(|v| v.as_str()).map(|s| s.to_string()),
            content: raw.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            conversation_id: raw.get("conversationId").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            content_type: raw.get("contentType").and_then(|v| v.as_str()).unwrap_or("text").to_string(),
            created_at: raw.get("createdAt").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            metadata: raw.get("metadata").cloned().unwrap_or(serde_json::Value::Null),
        };

        eprintln!("[signaldock] New from @{}: {}...", msg.from, &msg.content[..msg.content.len().min(80)]);

        match provider.deliver(&msg) {
            Ok(DeliveryResult::Delivered) => {
                delivered += 1;
                ack_ids.push(msg_id.to_string());
                if let Some(h) = health {
                    h.messages_delivered.fetch_add(1, Ordering::Relaxed);
                }
            }
            Ok(DeliveryResult::Retry(reason)) => {
                eprintln!("[signaldock] Retry: {}", reason);
                if let Ok(mut s) = seen.lock() { s.remove(msg_id); } // Will catch next cycle
            }
            Ok(DeliveryResult::Failed(reason)) => {
                eprintln!("[signaldock] Failed: {}", reason);
                ack_ids.push(msg_id.to_string()); // Ack to prevent infinite loop
            }
            Err(e) => {
                eprintln!("[signaldock] Error: {}", e);
                if let Ok(mut s) = seen.lock() { s.remove(msg_id); }
            }
        }
    }

    if !ack_ids.is_empty() {
        let _ = client.post(format!("{}/messages/ack", config.api_base))
            .header("Authorization", format!("Bearer {}", config.api_key))
            .header("X-Agent-Id", &config.agent_id)
            .json(&serde_json::json!({ "messageIds": ack_ids }))
            .timeout(Duration::from_secs(5))
            .send().await;
    }

    save_seen(config, seen)?;
    Ok(delivered)
}

fn load_seen(config: &Config) -> Result<HashSet<String>> {
    let path = config.state_dir()?.join("seen_ids.json");
    if path.exists() {
        let data = std::fs::read_to_string(&path)?;
        Ok(serde_json::from_str::<Vec<String>>(&data).unwrap_or_default().into_iter().collect())
    } else {
        Ok(HashSet::new())
    }
}

fn save_seen(config: &Config, seen: &Arc<Mutex<HashSet<String>>>) -> Result<()> {
    let path = config.state_dir()?.join("seen_ids.json");
    let mut s = seen.lock().map_err(|e| anyhow::anyhow!("seen lock poisoned: {e}"))?;
    // Prune to most recent 1000 entries to prevent unbounded growth
    if s.len() > 1000 {
        let all: Vec<String> = s.drain().collect();
        s.extend(all.into_iter().rev().take(1000));
    }
    let ids: Vec<&String> = s.iter().collect();
    std::fs::write(path, serde_json::to_string(&ids)?)?;
    Ok(())
}
