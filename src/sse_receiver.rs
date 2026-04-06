//! SSE (Server-Sent Events) receiver for real-time message delivery.
//!
//! Connects to the SignalDock SSE stream at `GET {api_base}/messages/stream`.
//! On connection failure or stream end, falls back to a single poll cycle
//! before reconnecting. This provides both low-latency delivery (SSE) and
//! reliability (poll fallback).

use anyhow::Result;
use crate::config::Config;
use crate::adapters::providers::provider::{Provider, Message, DeliveryResult};
use crate::health::HealthState;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::time::{sleep, Duration};

/// Run the SSE receiver loop with automatic reconnection and poll fallback.
///
/// This function never returns under normal operation. On SSE disconnection,
/// it runs one poll cycle as a fallback, waits 5 seconds, then reconnects.
pub async fn run_sse(
    config: Config,
    provider: Box<dyn Provider>,
    _poll_interval: u64,
    health: Option<Arc<HealthState>>,
) -> Result<()> {
    let config = Arc::new(config);
    let provider: Arc<Box<dyn Provider>> = Arc::new(provider);

    loop {
        eprintln!("[signaldock] Connecting SSE stream...");
        match sse_connect(&config, &provider, &health).await {
            Ok(()) => eprintln!("[signaldock] SSE stream ended normally"),
            Err(e) => eprintln!("[signaldock] SSE error: {}, falling back to polling", e),
        }

        // Mark disconnected
        if let Some(h) = health.as_ref() {
            h.connected.store(false, Ordering::Relaxed);
        }

        // Fallback: run one poll cycle while SSE reconnects
        eprintln!("[signaldock] Running poll fallback...");
        if let Err(e) = crate::receiver::poll_once_standalone(&config, &provider, &health).await {
            eprintln!("[signaldock] Poll fallback error: {}", e);
        }

        // Wait before reconnecting SSE
        sleep(Duration::from_secs(5)).await;
    }
}

/// Establish an SSE connection and process events until the stream ends.
async fn sse_connect(
    config: &Arc<Config>,
    provider: &Arc<Box<dyn Provider>>,
    health: &Option<Arc<HealthState>>,
) -> Result<()> {
    let url = format!("{}/messages/stream", config.api_base);
    let client = reqwest::Client::new();
    let mut response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("X-Agent-Id", &config.agent_id)
        .header("Accept", "text/event-stream")
        .header(
            "User-Agent",
            format!(
                "signaldock-runtime/{} ({})",
                env!("CARGO_PKG_VERSION"),
                config.agent_id,
            ),
        )
        .send()
        .await?;

    if !response.status().is_success() {
        anyhow::bail!("SSE connection failed: {}", response.status());
    }

    eprintln!("[signaldock] SSE connected");
    if let Some(h) = health {
        h.connected.store(true, Ordering::Relaxed);
    }

    let mut event_type = String::new();

    // Read the stream chunk by chunk
    while let Some(chunk) = response.chunk().await? {
        let text = String::from_utf8_lossy(&chunk);
        for line in text.lines() {
            if let Some(ev) = line.strip_prefix("event: ") {
                event_type = ev.to_string();
            } else if let Some(data) = line.strip_prefix("data: ") {
                // Process complete event
                handle_sse_event(&event_type, data, config, provider, health)?;
                event_type.clear();
            }
            // Blank lines and comments are ignored per SSE spec
        }
    }

    Ok(())
}

/// Handle a single SSE event by type.
fn handle_sse_event(
    event: &str,
    data: &str,
    config: &Config,
    provider: &Arc<Box<dyn Provider>>,
    health: &Option<Arc<HealthState>>,
) -> Result<()> {
    match event {
        "connected" => eprintln!("[signaldock] SSE: connected event received"),
        "heartbeat" => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            if let Some(h) = health {
                h.last_heartbeat.store(now, Ordering::Relaxed);
            }
        }
        "message" | "" => {
            // Parse message and deliver
            if let Ok(raw) = serde_json::from_str::<serde_json::Value>(data) {
                let msg_id = raw.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let from = raw
                    .get("fromAgentId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if from == config.agent_id || from.is_empty() {
                    return Ok(());
                }

                let content = raw
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let msg = Message {
                    id: msg_id.to_string(),
                    from: from.to_string(),
                    from_name: raw
                        .get("fromAgentName")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    content: content.clone(),
                    conversation_id: raw
                        .get("conversationId")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    content_type: raw
                        .get("contentType")
                        .and_then(|v| v.as_str())
                        .unwrap_or("text")
                        .to_string(),
                    created_at: raw
                        .get("createdAt")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    metadata: raw
                        .get("metadata")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                };

                let preview_len = content.len().min(80);
                eprintln!(
                    "[signaldock] SSE message from @{} msg={} conv={} type={}: {}...",
                    msg.from,
                    msg.id,
                    msg.conversation_id,
                    msg.content_type,
                    &content[..preview_len]
                );

                if let Some(h) = health {
                    h.messages_received.fetch_add(1, Ordering::Relaxed);
                    if let Ok(mut last) = h.last_message_id.lock() {
                        *last = Some(msg.id.clone());
                    }
                }

                match provider.deliver(&msg) {
                    Ok(DeliveryResult::Delivered) => {
                        if let Some(h) = health {
                            h.messages_delivered.fetch_add(1, Ordering::Relaxed);
                            h.hook_deliveries.fetch_add(1, Ordering::Relaxed);
                        }
                        // Auto-ack via API (fire and forget)
                        let api_base = config.api_base.clone();
                        let api_key = config.api_key.clone();
                        let agent_id = config.agent_id.clone();
                        let id = msg_id.to_string();
                        tokio::spawn(async move {
                            let _ = reqwest::Client::new()
                                .post(format!("{}/messages/ack", api_base))
                                .header("Authorization", format!("Bearer {}", api_key))
                                .header("X-Agent-Id", &agent_id)
                                .json(&serde_json::json!({ "messageIds": [id] }))
                                .timeout(Duration::from_secs(5))
                                .send()
                                .await;
                        });
                    }
                    Ok(DeliveryResult::Retry(r)) => eprintln!("[signaldock] Retry msg={} conv={}: {}", msg.id, msg.conversation_id, r),
                    Ok(DeliveryResult::Failed(r)) => {
                        if let Some(h) = health {
                            h.hook_delivery_failures.fetch_add(1, Ordering::Relaxed);
                        }
                        eprintln!("[signaldock] Failed msg={} conv={}: {}", msg.id, msg.conversation_id, r)
                    }
                    Err(e) => {
                        if let Some(h) = health {
                            h.hook_delivery_failures.fetch_add(1, Ordering::Relaxed);
                        }
                        eprintln!("[signaldock] Error msg={} conv={}: {}", msg.id, msg.conversation_id, e)
                    },
                }
            }
        }
        _ => {}
    }
    Ok(())
}
