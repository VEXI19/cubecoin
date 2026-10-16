use crate::config::load_config;

mod config;
mod logging;

fn main() {
    let config = match load_config() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("failed to load configuration: {error}");
            std::process::exit(1);
        }
    };

    if let Err(error) = logging::init(&config.rust_log, config.log_pretty) {
        eprintln!("failed to initialize logger: {error}");
        std::process::exit(1);
    }

    tracing::info!("logging initialized");

    if let Err(error) = run() {
        tracing::error!(error = %error, "application failed");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("application started");

    Ok(())
}
