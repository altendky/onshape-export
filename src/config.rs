use std::{env, net::SocketAddr, time::Duration};

#[derive(Debug, Clone)]
pub struct Config {
    pub bind_addr: SocketAddr,
    pub database_url: String,
    pub worker_enabled: bool,
    pub worker_concurrency: usize,
    pub rebuild_interval: Option<Duration>,
    pub storage: StorageConfig,
    pub backup_storage: Option<StorageConfig>,
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
    pub force_path_style: bool,
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
        let database_url = env_or("DATABASE_URL", "sqlite://onshape-export.db?mode=rwc");
        let worker_enabled = env_bool("WORKER_ENABLED", true)?;
        let worker_concurrency = env_usize("WORKER_CONCURRENCY", 1)?;
        let rebuild_interval = env_optional_duration("REBUILD_INTERVAL_SECONDS")?;

        let storage = StorageConfig {
            bucket: env_or("TIGRIS_BUCKET", "onshape-export"),
            endpoint_url: env::var("TIGRIS_ENDPOINT_URL").ok(),
            region: env_or("AWS_REGION", "auto"),
            access_key_id: env::var("AWS_ACCESS_KEY_ID").ok(),
            secret_access_key: env::var("AWS_SECRET_ACCESS_KEY").ok(),
            public_base_url: env::var("TIGRIS_PUBLIC_BASE_URL").ok(),
            force_path_style: env_bool("TIGRIS_FORCE_PATH_STYLE", false)?,
        };
        let backup_storage = backup_storage_from_env(&storage)?;

        Ok(Self {
            bind_addr,
            database_url,
            worker_enabled,
            worker_concurrency,
            rebuild_interval,
            storage,
            backup_storage,
            onshape: OnshapeConfig {
                base_url: env_or("ONSHAPE_BASE_URL", "https://cad.onshape.com"),
                access_key: env::var("ONSHAPE_ACCESS_KEY").ok(),
                secret_key: env::var("ONSHAPE_SECRET_KEY").ok(),
            },
        })
    }
}

fn backup_storage_from_env(storage: &StorageConfig) -> anyhow::Result<Option<StorageConfig>> {
    let Ok(bucket) = env::var("BACKUP_TIGRIS_BUCKET") else {
        return Ok(None);
    };

    Ok(Some(StorageConfig {
        bucket,
        endpoint_url: env::var("BACKUP_TIGRIS_ENDPOINT_URL")
            .ok()
            .or_else(|| storage.endpoint_url.clone()),
        region: env::var("BACKUP_AWS_REGION").unwrap_or_else(|_| storage.region.clone()),
        access_key_id: env::var("BACKUP_AWS_ACCESS_KEY_ID").ok(),
        secret_access_key: env::var("BACKUP_AWS_SECRET_ACCESS_KEY").ok(),
        public_base_url: None,
        force_path_style: env_bool("BACKUP_TIGRIS_FORCE_PATH_STYLE", storage.force_path_style)?,
    }))
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

fn env_usize(name: &str, default: usize) -> anyhow::Result<usize> {
    let Ok(value) = env::var(name) else {
        return Ok(default);
    };
    parse_positive_usize(&value, name)
}

fn parse_positive_usize(value: &str, name: &str) -> anyhow::Result<usize> {
    let parsed = value.parse::<usize>()?;
    anyhow::ensure!(parsed > 0, "{name} must be greater than zero");
    Ok(parsed)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_worker_concurrency() {
        assert_eq!(parse_positive_usize("1", "WORKER_CONCURRENCY").unwrap(), 1);
        assert_eq!(parse_positive_usize("3", "WORKER_CONCURRENCY").unwrap(), 3);
        assert!(parse_positive_usize("0", "WORKER_CONCURRENCY").is_err());
        assert!(parse_positive_usize("nope", "WORKER_CONCURRENCY").is_err());
    }
}
