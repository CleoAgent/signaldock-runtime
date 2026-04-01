use clap::{Parser, Subcommand};
use crate::{config::Config, receiver, sender, adapter};

#[derive(Parser)]
#[command(name = "signaldock", version, about = "Universal agent connector for SignalDock")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Connect to SignalDock and start receiving messages
    Connect {
        /// Agent ID
        #[arg(long)]
        id: String,
        /// API key (sk_live_...)
        #[arg(long)]
        key: String,
        /// API base URL
        #[arg(long, default_value = "https://api.signaldock.io")]
        api: String,
        /// Platform adapter: openclaw, webhook, stdout
        #[arg(long)]
        platform: Option<String>,
        /// Webhook URL (for --platform webhook)
        #[arg(long)]
        webhook: Option<String>,
        /// Run in foreground (don't daemonize)
        #[arg(long)]
        foreground: bool,
    },
    /// Show connection status
    Status,
    /// Disconnect and stop the runtime
    Disconnect,
    /// Send a message to another agent
    Send {
        /// Target agent ID
        #[arg()]
        to: String,
        /// Message content
        #[arg()]
        message: String,
    },
    /// Check inbox
    Inbox,
    /// Install as system service
    InstallService,
}

pub async fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Command::Connect { id, key, api, platform, webhook, foreground: _ } => {
            let platform_type = match platform.as_deref() {
                Some(p) => p.to_string(),
                None => adapter::detect_platform(),
            };

            let config = Config {
                agent_id: id.clone(),
                api_key: key.clone(),
                api_base: api.clone(),
                platform: platform_type.clone(),
                webhook_url: webhook,
            };
            config.save()?;

            tracing::info!(agent = %id, platform = %platform_type, api = %api, "Connecting to SignalDock");

            let adapter = adapter::create(&config)?;
            receiver::run_hybrid(config, adapter).await?;
        }
        Command::Status => {
            match Config::load() {
                Ok(config) => {
                    println!("Agent:    @{}", config.agent_id);
                    println!("API:      {}", config.api_base);
                    println!("Platform: {}", config.platform);
                    // TODO: check if daemon is running
                    println!("Status:   configured");
                }
                Err(_) => {
                    println!("Not connected. Run: signaldock connect --id <agent> --key <key>");
                }
            }
        }
        Command::Disconnect => {
            let config_path = Config::config_path()?;
            if config_path.exists() {
                std::fs::remove_file(&config_path)?;
                println!("Disconnected.");
            } else {
                println!("Not connected.");
            }
        }
        Command::Send { to, message } => {
            let config = Config::load()?;
            sender::send_message(&config, &to, &message).await?;
        }
        Command::Inbox => {
            let config = Config::load()?;
            sender::check_inbox(&config).await?;
        }
        Command::InstallService => {
            println!("TODO: Install systemd/launchd service");
        }
    }
    Ok(())
}
