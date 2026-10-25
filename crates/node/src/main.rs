use crate::config::Config;
use cubecoin_core::{config::load_config, logging};

mod config;

fn main() {
    let config: Config = match load_config(concat!(env!("CARGO_MANIFEST_DIR"), "/.env")) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("failed to load configuration: {error}");
            std::process::exit(1);
        }
    };

    if let Err(error) = logging::init(&config.logging) {
        eprintln!("failed to initialize logger: {error}");
        std::process::exit(1);
    }

    tracing::debug!("logging initialized");

    if let Err(error) = run() {
        tracing::error!(error = %error, "application failed");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("application started");

    Ok(())
}
