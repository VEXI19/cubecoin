use std::error::Error;

use tracing_subscriber::EnvFilter;

pub fn init(filter: &str, pretty: bool) -> Result<(), Box<dyn Error>> {
    let filter = EnvFilter::try_new(filter).map_err(|error| {
        std::io::Error::other(format!("invalid RUST_LOG filter '{filter}': {error}"))
    })?;

    if pretty {
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
