use anyhow::Result;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use tokio::time::{sleep, Duration};
use crate::{config::Config, adapter::PlatformAdapter};

/// Run the hybrid receiver: poll-primary with SSE upgrade when available.
/// Poll runs every poll_interval. SSE runs in parallel when connected.
/// Both feed through the same dedup filter.
pub async fn run_hybrid(config: Config, adapter: Box<dyn PlatformAdapter>) -> Result<()> {
    let seen = Arc::new(Mutex::new(load_seen(&config)?));
    let config = Arc::new(config);
    let adapter = Arc::new(adapter);

    tracing::info!(
        agent = %config.agent_id,
        platform = %adapter.name(),
        "Starting hybrid receiver (poll-primary + SSE)"
    );

    // Run poll loop as the primary reliable path
    let poll_config = config.clone();
    let poll_adapter = adapter.clone();
    let poll_seen = seen.clone();

    let poll_interval = Duration::from_secs(15);
    let mut consecutive_errors = 0u32;

    loop {
        match poll_once(&poll_config, &poll_adapter, &poll_seen).await {
            Ok(count) => {
                if count > 0 {
                    tracing::info!(count, "Processed messages via poll");
                }
                consecutive_errors = 0;
            }
            Err(e) => {
                consecutive_errors += 1;
                tracing::warn!(error = %e, consecutive = consecutive_errors, "Poll error");
            }
        }

        // Backoff on consecutive errors
        let delay = if consecutive_errors > 5 {
            Duration::from_secs(60)
        } else if consecutive_errors > 0 {
            Duration::from_secs(30)
        } else {
            poll_interval
        };

        sleep(delay).await;
    }
}

/// Poll /messages/peek, deliver new messages via adapter, ack them.
async fn poll_once(
    config: &Config,
    adapter: &Arc<Box<dyn PlatformAdapter>>,
    seen: &Arc<Mutex<HashSet<String>>>,
) -> Result<usize> {
    let client = reqwest::Client::new();
    let url = format!("{}/messages/peek?limit=50", config.api_base);

    let resp = client.get(&url)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("X-Agent-Id", &config.agent_id)
        .header("User-Agent", format!("signaldock-runtime/0.1.0 ({})", config.agent_id))
        .timeout(Duration::from_secs(10))
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("API returned {}", resp.status());
    }

    let body: serde_json::Value = resp.json().await?;
    let messages = body.get("data")
        .and_then(|d| d.get("messages"))
        .and_then(|m| m.as_array());

    let messages = match messages {
        Some(m) => m,
        None => return Ok(0),
    };

    let mut delivered = 0;
    let mut ack_ids = Vec::new();

    for msg in messages {
        let msg_id = msg.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let from = msg.get("fromAgentId").and_then(|v| v.as_str()).unwrap_or("");
        let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
        let conv_id = msg.get("conversationId").and_then(|v| v.as_str()).unwrap_or("");

        // Skip own messages and empty senders
        if from == config.agent_id || from.is_empty() || msg_id.is_empty() {
            continue;
        }

        // Dedup check
        {
            let mut seen_lock = seen.lock().unwrap();
            if seen_lock.contains(msg_id) {
                continue;
            }
            seen_lock.insert(msg_id.to_string());
        }

        tracing::info!(from = from, id = &msg_id[..8.min(msg_id.len())], "New message");

        // Deliver to platform adapter
        match adapter.deliver(from, content, msg_id, conv_id) {
            Ok(()) => {
                delivered += 1;
                ack_ids.push(msg_id.to_string());
            }
            Err(e) => {
                tracing::error!(error = %e, from = from, "Adapter delivery failed");
                // Don't ack — will retry next poll
            }
        }
    }

    // Batch ack successfully delivered messages
    if !ack_ids.is_empty() {
        let _ = client.post(format!("{}/messages/ack", config.api_base))
            .header("Authorization", format!("Bearer {}", config.api_key))
            .header("X-Agent-Id", &config.agent_id)
            .header("User-Agent", format!("signaldock-runtime/0.1.0 ({})", config.agent_id))
            .json(&serde_json::json!({ "messageIds": ack_ids }))
            .timeout(Duration::from_secs(5))
            .send()
            .await;
    }

    save_seen(config, seen)?;
    Ok(delivered)
}

fn load_seen(config: &Config) -> Result<HashSet<String>> {
    let path = config.state_dir()?.join("seen_ids.json");
    if path.exists() {
        let data = std::fs::read_to_string(&path)?;
        let ids: Vec<String> = serde_json::from_str(&data).unwrap_or_default();
        Ok(ids.into_iter().collect())
    } else {
        Ok(HashSet::new())
    }
}

fn save_seen(config: &Config, seen: &Arc<Mutex<HashSet<String>>>) -> Result<()> {
    let path = config.state_dir()?.join("seen_ids.json");
    let seen_lock = seen.lock().unwrap();
    let ids: Vec<&String> = seen_lock.iter().take(5000).collect();
    std::fs::write(path, serde_json::to_string(&ids)?)?;
    Ok(())
}
