use cubecoin_core::config::LoggingConfig;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(flatten)]
    pub logging: LoggingConfig,
}
