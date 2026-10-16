use serde::Deserialize;

const ENV_FILES: &[&str] = &[".env", ".env.local"];

#[derive(Debug, Deserialize)]
pub struct Config {
    bootstrap_ip: String,
    port: i32,
}

pub fn load_config() -> Result<Config, Box<dyn std::error::Error>> {
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
    let config: Config = envy::from_env().map_err(|error| {
        std::io::Error::other(format!(
            "failed to parse environment configuration: {error}"
        ))
    })?;

    Ok(config)
}
