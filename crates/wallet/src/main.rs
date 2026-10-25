use crate::config::Config;
use crate::identity::WalletIdentity;
use cubecoin_core::{
    config::{LogFormat, load_config},
    logging,
};
use std::io::Write;

mod config;
mod identity;

fn main() {
    let config: Config = match load_config(concat!(env!("CARGO_MANIFEST_DIR"), "/.env")) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("failed to load configuration: {error}");
            std::process::exit(1);
        }
    };

    if let Err(error) = logging::init(&config.logging, LogFormat::Compact) {
        eprintln!("failed to initialize logger: {error}");
        std::process::exit(1);
    }

    tracing::debug!("configuration loaded");
    tracing::debug!("logging initialized");

    if let Err(error) = run() {
        tracing::error!(error = %error, "wallet failed");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("wallet started");
    tracing::info!("generating ephemeral wallet identity");

    let address = WalletIdentity::generate()
        .map_err(std::io::Error::other)?
        .address();

    let mut stdout = std::io::stdout().lock();
    match writeln!(stdout, "{address}") {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => return Ok(()),
        Err(error) => return Err(error.into()),
    }

    tracing::warn!("wallet identity is ephemeral and is not persisted; do not send funds to it");

    Ok(())
}
