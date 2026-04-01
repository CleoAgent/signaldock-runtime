use anyhow::Result;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use tokio::time::{sleep, Duration};
use crate::{config::Config, adapter::PlatformAdapter};

/// Hybrid receiver: SSE for real-time + poll-on-reconnect to catch gaps.
pub async fn run_hybrid(config: Config, adapter: Box<dyn PlatformAdapter>) -> Result<()> {
    let seen = Arc::new(Mutex::new(load_seen(&config)?));
    let config = Arc::new(config);
    let adapter = Arc::new(adapter);

    let mut retry_delay = Duration::from_secs(5);
    let max_delay = Duration::from_secs(60);

    loop {
        // Poll for any missed messages on every connect/reconnect
        tracing::info!("Polling for missed messages...");
        if let Err(e) = poll_messages(&config, &adapter, &seen).await {
            tracing::warn!(error = %e, "Poll failed");
        }

        // Connect SSE
        tracing::info!("Connecting to SSE...");
        match run_sse(&config, &adapter, &seen).await {
            Ok(()) => {
                tracing::info!("SSE disconnected cleanly");
                retry_delay = Duration::from_secs(5); // Reset on clean disconnect
            }
            Err(e) => {
                tracing::warn!(error = %e, "SSE error");
            }
        }

        tracing::info!(delay = ?retry_delay, "Reconnecting...");
        sleep(retry_delay).await;
        retry_delay = (retry_delay * 2).min(max_delay);
    }
}

/// Connect to SSE and process messages in real-time.
async fn run_sse(
    config: &Config,
    adapter: &Arc<Box<dyn PlatformAdapter>>,
    seen: &Arc<Mutex<HashSet<String>>>,
) -> Result<()> {
    use reqwest_eventsource::{EventSource, Event};
    use futures_util::StreamExt;

    let url = format!("{}/messages/stream", config.api_base);
    let client = reqwest::Client::new();
    let request = client.get(&url)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("X-Agent-Id", &config.agent_id)
        .header("Accept", "text/event-stream")
        .header("User-Agent", format!("signaldock-runtime/0.1.0 ({})", config.agent_id));

    let mut es = EventSource::new(request)?;

    while let Some(event) = es.next().await {
        match event {
            Ok(Event::Open) => {
                tracing::info!("SSE connected");
            }
            Ok(Event::Message(msg)) => {
                if msg.event == "connected" {
                    tracing::info!(data = %msg.data, "SSE handshake complete");
                    continue;
                }
                if msg.event == "heartbeat" {
                    tracing::debug!("SSE heartbeat");
                    continue;
                }

                // Parse message
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&msg.data) {
                    let msg_id = parsed.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    let from = parsed.get("fromAgentId").and_then(|v| v.as_str()).unwrap_or("");
                    let content = parsed.get("content").and_then(|v| v.as_str()).unwrap_or("");
                    let conv_id = parsed.get("conversationId").and_then(|v| v.as_str()).unwrap_or("");

                    // Skip own messages
                    if from == config.agent_id || from.is_empty() {
                        continue;
                    }

                    // Dedup
                    {
                        let mut seen_lock = seen.lock().unwrap();
                        if seen_lock.contains(msg_id) {
                            continue;
                        }
                        seen_lock.insert(msg_id.to_string());
                    }

                    tracing::info!(from = from, id = msg_id, "SSE message received");
                    if let Err(e) = adapter.deliver(from, content, msg_id, conv_id) {
                        tracing::error!(error = %e, "Adapter delivery failed");
                    }

                    // Ack
                    ack_message(config, msg_id).await;
                    save_seen(config, &seen)?;
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "SSE stream error");
                es.close();
                return Ok(());
            }
        }
    }

    Ok(())
}

/// Poll /messages/peek for any missed messages.
async fn poll_messages(
    config: &Config,
    adapter: &Arc<Box<dyn PlatformAdapter>>,
    seen: &Arc<Mutex<HashSet<String>>>,
) -> Result<()> {
    let client = reqwest::Client::new();
    let url = format!("{}/messages/peek?limit=50", config.api_base);

    let resp = client.get(&url)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("X-Agent-Id", &config.agent_id)
        .header("User-Agent", format!("signaldock-runtime/0.1.0 ({})", config.agent_id))
        .timeout(Duration::from_secs(10))
        .send()
        .await?;

    let body: serde_json::Value = resp.json().await?;
    let messages = body.get("data").and_then(|d| d.get("messages")).and_then(|m| m.as_array());

    let mut ack_ids = Vec::new();

    if let Some(messages) = messages {
        for msg in messages {
            let msg_id = msg.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let from = msg.get("fromAgentId").and_then(|v| v.as_str()).unwrap_or("");
            let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let conv_id = msg.get("conversationId").and_then(|v| v.as_str()).unwrap_or("");

            if from == config.agent_id || from.is_empty() {
                continue;
            }

            // Dedup
            {
                let mut seen_lock = seen.lock().unwrap();
                if seen_lock.contains(msg_id) {
                    continue;
                }
                seen_lock.insert(msg_id.to_string());
            }

            tracing::info!(from = from, id = msg_id, "Poll: new message");
            if let Err(e) = adapter.deliver(from, content, msg_id, conv_id) {
                tracing::error!(error = %e, "Adapter delivery failed");
            }
            ack_ids.push(msg_id.to_string());
        }
    }

    // Batch ack
    if !ack_ids.is_empty() {
        let client = reqwest::Client::new();
        let _ = client.post(format!("{}/messages/ack", config.api_base))
            .header("Authorization", format!("Bearer {}", config.api_key))
            .header("X-Agent-Id", &config.agent_id)
            .json(&serde_json::json!({ "messageIds": ack_ids }))
            .timeout(Duration::from_secs(5))
            .send()
            .await;
        tracing::info!(count = ack_ids.len(), "Messages acknowledged");
    }

    save_seen(config, seen)?;
    Ok(())
}

async fn ack_message(config: &Config, message_id: &str) {
    let client = reqwest::Client::new();
    let _ = client.post(format!("{}/messages/ack", config.api_base))
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("X-Agent-Id", &config.agent_id)
        .json(&serde_json::json!({ "messageIds": [message_id] }))
        .timeout(Duration::from_secs(5))
        .send()
        .await;
}

fn load_seen(config: &Config) -> Result<HashSet<String>> {
    let path = config.state_dir()?.join("seen_ids.json");
    if path.exists() {
        let data = std::fs::read_to_string(&path)?;
        let ids: Vec<String> = serde_json::from_str(&data)?;
        Ok(ids.into_iter().collect())
    } else {
        Ok(HashSet::new())
    }
}

fn save_seen(config: &Config, seen: &Arc<Mutex<HashSet<String>>>) -> Result<()> {
    let path = config.state_dir()?.join("seen_ids.json");
    let seen_lock = seen.lock().unwrap();
    // Keep last 5000 IDs
    let ids: Vec<&String> = seen_lock.iter().take(5000).collect();
    std::fs::write(path, serde_json::to_string(&ids)?)?;
    Ok(())
}
