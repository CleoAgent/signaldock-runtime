mod config;
mod receiver;
mod adapter;
mod sender;
mod cli;

use clap::Parser;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("signaldock_runtime=info".parse()?))
        .init();

    let args = cli::Cli::parse();
    cli::run(args).await
}
