use crate::config::Config;
use cubecoin_core::{
    config::{LogFormat, load_config},
    logging,
};

mod config;

#[tokio::main]
async fn main() {
    let config: Config = match load_config(concat!(env!("CARGO_MANIFEST_DIR"), "/.env")) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("failed to load configuration: {error}");
            std::process::exit(1);
        }
    };

    if let Err(error) = logging::init(&config.logging, LogFormat::Pretty) {
        eprintln!("failed to initialize logger: {error}");
        std::process::exit(1);
    }

    tracing::debug!("configuration loaded");
    tracing::debug!("logging initialized");

    let node_config = match config.try_into() {
        Ok(config) => config,
        Err(error) => {
            tracing::error!(error = %error, "invalid P2P configuration");
            std::process::exit(1);
        }
    };

    if let Err(error) = run(node_config).await {
        tracing::error!(error = %error, "application failed");
        std::process::exit(1);
    }
}

async fn run(config: cubecoin_p2p::NodeConfig) -> Result<(), Box<dyn std::error::Error>> {
    let node = cubecoin_p2p::NodeHandle::start(config).await?;
    tracing::info!(event = "node.started", node_id = %node.node_id(), address = %node.address(), "node started");
    wait_for_shutdown_signal().await?;
    tracing::info!(
        event = "node.shutdown_requested",
        "shutdown signal received"
    );
    node.shutdown();

    Ok(())
}

#[cfg(unix)]
async fn wait_for_shutdown_signal() -> Result<(), std::io::Error> {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        result = tokio::signal::ctrl_c() => result,
        _ = terminate.recv() => Ok(()),
    }
}

#[cfg(not(unix))]
async fn wait_for_shutdown_signal() -> Result<(), std::io::Error> {
    tokio::signal::ctrl_c().await
}
