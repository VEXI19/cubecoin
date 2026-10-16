use serde::{Deserialize, Deserializer, de::DeserializeOwned};

const ENV_FILES: &[&str] = &[".env", ".env.node", ".env.client"];

#[derive(Debug, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_filter")]
    pub rust_log: String,
    #[serde(default, deserialize_with = "deserialize_bool")]
    pub log_pretty: bool,
}

fn default_log_filter() -> String {
    "info".to_owned()
}

fn deserialize_bool<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    value.parse().map_err(serde::de::Error::custom)
}

pub fn load_config<T: DeserializeOwned>() -> Result<T, Box<dyn std::error::Error>> {
    let env_file = ENV_FILES
        .iter()
        .copied()
        .find(|file| std::path::Path::new(file).is_file())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "no environment file found; checked: {}",
                    ENV_FILES.join(", ")
                ),
            )
        })?;

    dotenvy::from_filename(env_file)
        .map_err(|error| std::io::Error::other(format!("failed to load {env_file}: {error}")))?;

    envy::from_env().map_err(|error| {
        std::io::Error::other(format!(
            "failed to parse environment configuration: {error}"
        ))
        .into()
    })
}
