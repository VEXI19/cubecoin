use std::error::Error;

use tracing_subscriber::EnvFilter;

use crate::config::{LogFormat, LoggingConfig};

pub fn init(config: &LoggingConfig, default_format: LogFormat) -> Result<(), Box<dyn Error>> {
    let filter = EnvFilter::try_new(&config.rust_log).map_err(|error| {
        std::io::Error::other(format!(
            "invalid RUST_LOG filter '{}': {error}",
            config.rust_log
        ))
    })?;

    match config.format_or(default_format) {
        LogFormat::Pretty => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .pretty()
            .try_init(),
        LogFormat::Compact => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .compact()
            .try_init(),
        LogFormat::Json => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .json()
            .try_init(),
    }
    .map_err(|error| std::io::Error::other(format!("failed to initialize logging: {error}")))?;

    Ok(())
}
