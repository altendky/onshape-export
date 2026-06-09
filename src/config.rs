use std::{env, net::SocketAddr, path::PathBuf, time::Duration};

#[derive(Debug, Clone)]
pub struct Config {
    pub bind_addr: SocketAddr,
    pub catalog_path: PathBuf,
    pub database_url: String,
    pub worker_enabled: bool,
    pub rebuild_interval: Option<Duration>,
    pub storage: StorageConfig,
    pub onshape: OnshapeConfig,
}

#[derive(Debug, Clone)]
pub struct StorageConfig {
    pub bucket: String,
    pub endpoint_url: Option<String>,
    pub region: String,
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
    pub public_base_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OnshapeConfig {
    pub base_url: String,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let bind_addr = env_or("BIND_ADDR", "0.0.0.0:3000").parse()?;
        let catalog_path = PathBuf::from(env_or("CATALOG_PATH", "catalog/models.json"));
        let database_url = env_or("DATABASE_URL", "sqlite://onshape-export.db?mode=rwc");
        let worker_enabled = env_bool("WORKER_ENABLED", true)?;
        let rebuild_interval = env_optional_duration("REBUILD_INTERVAL_SECONDS")?;

        Ok(Self {
            bind_addr,
            catalog_path,
            database_url,
            worker_enabled,
            rebuild_interval,
            storage: StorageConfig {
                bucket: env_or("TIGRIS_BUCKET", "onshape-export"),
                endpoint_url: env::var("TIGRIS_ENDPOINT_URL").ok(),
                region: env_or("AWS_REGION", "auto"),
                access_key_id: env::var("AWS_ACCESS_KEY_ID").ok(),
                secret_access_key: env::var("AWS_SECRET_ACCESS_KEY").ok(),
                public_base_url: env::var("TIGRIS_PUBLIC_BASE_URL").ok(),
            },
            onshape: OnshapeConfig {
                base_url: env_or("ONSHAPE_BASE_URL", "https://cad.onshape.com"),
                access_key: env::var("ONSHAPE_ACCESS_KEY").ok(),
                secret_key: env::var("ONSHAPE_SECRET_KEY").ok(),
            },
        })
    }
}

fn env_or(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_owned())
}

fn env_bool(name: &str, default: bool) -> anyhow::Result<bool> {
    let Ok(value) = env::var(name) else {
        return Ok(default);
    };

    match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => anyhow::bail!("{name} must be a boolean value"),
    }
}

fn env_optional_duration(name: &str) -> anyhow::Result<Option<Duration>> {
    let Ok(value) = env::var(name) else {
        return Ok(None);
    };
    let seconds = value.parse::<u64>()?;
    if seconds == 0 {
        Ok(None)
    } else {
        Ok(Some(Duration::from_secs(seconds)))
    }
}
