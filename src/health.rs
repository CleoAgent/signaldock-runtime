//! Local health check HTTP server for the SignalDock runtime.
//!
//! Runs alongside the receiver and exposes a `/health` endpoint on a configurable
//! port (default 4321). Returns JSON with connection status, heartbeat age, and
//! message delivery count. Useful for monitoring, orchestrators, and liveness probes.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};
use tokio::net::TcpListener;

/// Shared health state updated by the SSE receiver and polling loop.
pub struct HealthState {
    /// Whether the SSE stream is currently connected.
    pub connected: AtomicBool,
    /// Unix timestamp of the last heartbeat (SSE) or successful poll.
    pub last_heartbeat: AtomicU64,
    /// Total messages delivered to the configured provider since process start.
    pub messages_delivered: AtomicU64,
    /// Total inbound messages received from SignalDock since process start.
    pub messages_received: AtomicU64,
    /// Total successful hook/provider forwards since process start.
    pub hook_deliveries: AtomicU64,
    /// Total failed hook/provider forwards since process start.
    pub hook_delivery_failures: AtomicU64,
    /// Last delivered message id if known.
    pub last_message_id: Arc<std::sync::Mutex<Option<String>>>,
    /// Detected provider/platform name.
    pub provider_name: String,
    /// Hook/health target port when applicable.
    pub provider_port: AtomicU16,
    /// The agent ID this runtime is connected as.
    pub agent_id: String,
    /// Unix timestamp when the runtime started.
    pub started_at: u64,
}

/// Start the health HTTP server on `127.0.0.1:{port}`.
///
/// This function runs forever, accepting TCP connections and responding with
/// a JSON health payload. If the port is already in use, it logs an error
/// and returns (does not crash the runtime).
pub async fn run_health_server(state: Arc<HealthState>, port: u16) {
    let listener = match TcpListener::bind(format!("127.0.0.1:{}", port)).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!(
                "[signaldock] Health server failed to bind on port {}: {}",
                port, e
            );
            return;
        }
    };
    eprintln!(
        "[signaldock] Health endpoint: http://127.0.0.1:{}/health",
        port
    );

    loop {
        if let Ok((mut stream, _)) = listener.accept().await {
            let state = state.clone();
            tokio::spawn(async move {
                use tokio::io::AsyncWriteExt;

                let connected = state.connected.load(Ordering::Relaxed);
                let last_hb = state.last_heartbeat.load(Ordering::Relaxed);
                let delivered = state.messages_delivered.load(Ordering::Relaxed);
                let received = state.messages_received.load(Ordering::Relaxed);
                let hook_deliveries = state.hook_deliveries.load(Ordering::Relaxed);
                let hook_failures = state.hook_delivery_failures.load(Ordering::Relaxed);
                let provider_port = state.provider_port.load(Ordering::Relaxed);
                let last_message_id = state
                    .last_message_id
                    .lock()
                    .ok()
                    .and_then(|g| g.clone())
                    .unwrap_or_default();
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let status = if connected { "healthy" } else { "degraded" };
                let uptime = now.saturating_sub(state.started_at);

                let body = format!(
                    r#"{{"status":"{}","agentId":"{}","provider":"{}","providerPort":{},"connected":{},"lastHeartbeat":{},"messagesReceived":{},"messagesDelivered":{},"hookDeliveries":{},"hookDeliveryFailures":{},"lastMessageId":"{}","uptimeSeconds":{}}}"#,
                    status,
                    state.agent_id,
                    state.provider_name,
                    provider_port,
                    connected,
                    last_hb,
                    received,
                    delivered,
                    hook_deliveries,
                    hook_failures,
                    last_message_id,
                    uptime
                );
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    }
}
