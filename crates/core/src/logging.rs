use std::error::Error;

use tracing_subscriber::EnvFilter;

use crate::config::LoggingConfig;

pub fn init(config: &LoggingConfig) -> Result<(), Box<dyn Error>> {
    let filter = EnvFilter::try_new(&config.rust_log).map_err(|error| {
        std::io::Error::other(format!(
            "invalid RUST_LOG filter '{}': {error}",
            config.rust_log
        ))
    })?;

    if config.log_pretty {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .pretty()
            .try_init()
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .json()
            .try_init()
    }
    .map_err(|error| std::io::Error::other(format!("failed to initialize logging: {error}")))?;

    Ok(())
}
