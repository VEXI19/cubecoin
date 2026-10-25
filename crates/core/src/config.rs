use std::path::Path;

use serde::{Deserialize, Deserializer, de::DeserializeOwned};

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

pub fn load_config<T: DeserializeOwned>(
    env_file: impl AsRef<Path>,
) -> Result<T, Box<dyn std::error::Error>> {
    let env_file = env_file.as_ref();

    dotenvy::from_path(env_file).map_err(|error| dotenv_error(env_file, error))?;

    envy::from_env().map_err(|error| {
        let message = match error {
            envy::Error::MissingValue(field) => {
                format!("missing required environment variable: {field}")
            }
            envy::Error::Custom(_) => "invalid environment configuration".to_owned(),
        };

        std::io::Error::other(message).into()
    })
}

fn dotenv_error(env_file: &Path, error: dotenvy::Error) -> std::io::Error {
    let message = match error {
        dotenvy::Error::LineParse(_, index) => {
            format!(
                "failed to parse environment file {}: invalid syntax at character index {index}",
                env_file.display()
            )
        }
        dotenvy::Error::Io(error) => format!(
            "failed to load environment file {}: {:?}",
            env_file.display(),
            error.kind()
        ),
        dotenvy::Error::EnvVar(_) => format!(
            "failed to load environment file {}: invalid environment variable",
            env_file.display()
        ),
        _ => format!("failed to load environment file {}", env_file.display()),
    };

    std::io::Error::other(message)
}
