use std::path::Path;

use serde::{Deserialize, de::DeserializeOwned};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    Pretty,
    Compact,
    Json,
}

#[derive(Debug, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_filter")]
    pub rust_log: String,
    #[serde(default)]
    pub log_format: Option<LogFormat>,
}

impl LoggingConfig {
    pub fn format_or(&self, default_format: LogFormat) -> LogFormat {
        self.log_format.unwrap_or(default_format)
    }
}

fn default_log_filter() -> String {
    "info".to_owned()
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

#[cfg(test)]
mod tests {
    use serde::{
        Deserialize,
        de::value::{Error, StrDeserializer},
    };

    use super::{LogFormat, LoggingConfig};

    #[test]
    fn deserializes_log_formats() {
        for (value, expected) in [
            ("pretty", LogFormat::Pretty),
            ("compact", LogFormat::Compact),
            ("json", LogFormat::Json),
        ] {
            let format = LogFormat::deserialize(StrDeserializer::<Error>::new(value)).unwrap();
            assert_eq!(format, expected);
        }
    }

    #[test]
    fn rejects_unknown_log_format() {
        let format = LogFormat::deserialize(StrDeserializer::<Error>::new("text"));
        assert!(format.is_err());
    }

    #[test]
    fn uses_the_default_only_when_no_format_is_configured() {
        let default_config = LoggingConfig {
            rust_log: "info".to_owned(),
            log_format: None,
        };
        let configured = LoggingConfig {
            rust_log: "info".to_owned(),
            log_format: Some(LogFormat::Json),
        };

        assert_eq!(
            default_config.format_or(LogFormat::Compact),
            LogFormat::Compact
        );
        assert_eq!(configured.format_or(LogFormat::Pretty), LogFormat::Json);
    }
}
