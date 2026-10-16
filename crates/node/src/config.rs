use cubecoin_core::config::LoggingConfig;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub bootstrap_ip: String,
    pub port: i32,
    #[serde(flatten)]
    pub logging: LoggingConfig,
}
