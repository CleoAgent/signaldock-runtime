use clap::{Parser, Subcommand};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};
use tokio::time::sleep;
use crate::{config::Config, receiver, sse_receiver, health, sender, adapters, service};

#[derive(Parser)]
#[command(name = "signaldock", version, about = "Universal agent connector for SignalDock")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Connect and start receiving messages
    Connect {
        #[arg(long)] id: String,
        #[arg(long)] key: String,
        #[arg(long, default_value = "https://api.signaldock.io")] api: String,
        /// Provider: openclaw, claude-code, codex, gemini, copilot, opencode, webhook, stdout, file
        #[arg(long)] platform: Option<String>,
        #[arg(long)] webhook: Option<String>,
        #[arg(long, default_value = "15")] interval: u64,
        /// Disable SSE and use polling only
        #[arg(long, default_value = "false")]
        no_sse: bool,
        /// Health endpoint port (0 to disable)
        #[arg(long, default_value = "4321")]
        health_port: u16,
    },
    /// Show connection status
    Status,
    /// Remove config
    Disconnect,
    /// Send a message
    Send { to: String, message: String },
    /// Check inbox
    Inbox,
    /// List available providers
    Providers,
    /// Install as system service
    InstallService {
        #[arg(long)] id: Option<String>,
        #[arg(long)] key: Option<String>,
    },
}

pub async fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Command::Connect { id, key, api, platform, webhook, interval, no_sse, health_port } => {
            let platform_name = platform.unwrap_or_else(adapters::detect_provider);
            let config = Config {
                agent_id: id.clone(), api_key: key, api_base: api,
                platform: platform_name.clone(),
                webhook_url: webhook, file_output_dir: None,
            };
            config.save()?;

            let use_sse = !no_sse;
            let mode = if use_sse { "SSE + poll fallback" } else { "polling" };
            eprintln!("[signaldock] Mode: {} | Health: {}",
                mode,
                if health_port > 0 { format!("http://127.0.0.1:{}/health", health_port) } else { "disabled".into() },
            );

            // Create shared health state
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let health_state = Arc::new(health::HealthState {
                connected: AtomicBool::new(false),
                last_heartbeat: AtomicU64::new(now),
                messages_delivered: AtomicU64::new(0),
                messages_received: AtomicU64::new(0),
                hook_deliveries: AtomicU64::new(0),
                hook_delivery_failures: AtomicU64::new(0),
                last_message_id: Arc::new(std::sync::Mutex::new(None)),
                provider_name: config.platform.clone(),
                provider_port: std::sync::atomic::AtomicU16::new(0),
                agent_id: id.clone(),
                started_at: now,
            });

            // Spawn health server if enabled
            if health_port > 0 {
                let hs = health_state.clone();
                tokio::spawn(async move {
                    health::run_health_server(hs, health_port).await;
                });
            }

            // Watchdog supervisor loop — restarts receiver on crash
            loop {
                let config_clone = config.clone();
                let provider = adapters::create_provider(&config)?;
                let hs = Some(health_state.clone());

                let result = if use_sse {
                    sse_receiver::run_sse(config_clone, provider, interval, hs).await
                } else {
                    receiver::run_poll(config_clone, provider, interval, hs).await
                };

                match result {
                    Ok(()) => break, // Normal exit
                    Err(e) => {
                        eprintln!("[signaldock] Receiver crashed: {}. Restarting in 10s...", e);
                        sleep(tokio::time::Duration::from_secs(10)).await;
                    }
                }
            }
        }

        Command::Status => {
            match Config::load() {
                Ok(c) => {
                    println!("Agent:    @{}", c.agent_id);
                    println!("API:      {}", c.api_base);
                    println!("Provider: {}", c.platform);
                    println!("Config:   {}", Config::config_path()?.display());
                    if let Ok(p) = adapters::create_provider(&c) {
                        println!("Health:   {}", p.status_line());
                    }
                }
                Err(_) => println!("Not connected. Run: signaldock connect --id <agent> --key <key>"),
            }
        }

        Command::Disconnect => {
            let p = Config::config_path()?;
            if p.exists() { std::fs::remove_file(&p)?; println!("Disconnected."); }
            else { println!("Not connected."); }
        }

        Command::Send { to, message } => {
            let config = Config::load()?;
            sender::send_message(&config, &to, &message).await?;
        }

        Command::Inbox => {
            let config = Config::load()?;
            sender::check_inbox(&config).await?;
        }

        Command::Providers => {
            println!("Available providers:");
            println!();
            println!("  {:15} {:20} Detection", "Name", "Platform");
            println!("  {:15} {:20} ---------", "----", "--------");
            println!("  {:15} {:20} ~/.openclaw/openclaw.json (hooks.enabled)", "openclaw", "OpenClaw");
            println!("  {:15} {:20} ~/.claude/ directory", "claude-code", "Claude Code");
            println!("  {:15} {:20} ~/.codex/ or `codex` in PATH", "codex", "OpenAI Codex");
            println!("  {:15} {:20} ~/.gemini/ or `gemini` in PATH", "gemini", "Google Gemini");
            println!("  {:15} {:20} ~/.config/github-copilot/", "copilot", "GitHub Copilot");
            println!("  {:15} {:20} ~/.opencode/ or `opencode` in PATH", "opencode", "OpenCode");
            println!("  {:15} {:20} --webhook URL required", "webhook", "Generic Webhook");
            println!("  {:15} {:20} Writes JSON to directory", "file", "File Output");
            println!("  {:15} {:20} Prints JSON to stdout", "stdout", "Stdout");
            println!();

            let detected = adapters::detect_provider();
            println!("Auto-detected: {}", detected);
        }

        Command::InstallService { id, key } => {
            let config = match (id, key) {
                (Some(i), Some(k)) => {
                    let c = Config {
                        agent_id: i, api_key: k,
                        api_base: "https://api.signaldock.io".into(),
                        platform: adapters::detect_provider(),
                        webhook_url: None, file_output_dir: None,
                    };
                    c.save()?;
                    c
                }
                _ => Config::load()?,
            };
            service::install_service(&config)?;
        }
    }
    Ok(())
}

// Service installation moved to service.rs (cross-platform)
