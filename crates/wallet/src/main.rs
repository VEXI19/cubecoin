use crate::config::Config;
use cubecoin_core::{
    config::{LogFormat, load_config},
    logging,
};
use std::io::Write;

mod config;
mod identity;
#[cfg(target_os = "linux")]
mod keystore;

#[cfg(not(target_os = "linux"))]
compile_error!("cubecoin-wallet persistent identity storage currently supports Linux only");

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
    let keystore = keystore::Keystore::resolve_default()?;

    if keystore.uses_development_override() {
        tracing::warn!("using the debug-only development wallet path override");
    }
    if keystore.ignored_relative_xdg_path() {
        tracing::warn!("ignoring relative XDG_DATA_HOME while resolving the wallet path");
    }

    let opened = keystore.load_or_create()?;
    if opened.created {
        tracing::info!("wallet identity created");
    } else {
        tracing::info!("wallet identity unlocked");
    }

    let address = opened.identity.address();

    let mut stdout = std::io::stdout().lock();
    match write_address(&mut stdout, address) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => return Ok(()),
        Err(error) => return Err(error.into()),
    }

    Ok(())
}

fn write_address(
    output: &mut dyn Write,
    address: cubecoin_core::identity::Address,
) -> std::io::Result<()> {
    writeln!(output, "{address}")
}

#[cfg(test)]
mod tests {
    use cubecoin_core::identity::Address;

    use super::write_address;

    #[test]
    fn writes_only_the_address_to_stdout() {
        let address = Address::from_ed25519_public_key(&[0x42; 32]);
        let mut output = Vec::new();

        write_address(&mut output, address).expect("write address");

        assert_eq!(output, format!("{address}\n").into_bytes());
    }
}
