use std::{env, net::SocketAddr, path::PathBuf};

#[derive(Debug, Clone)]
pub struct Config {
    pub bind_addr: SocketAddr,
    pub catalog_path: PathBuf,
    pub database_url: String,
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

        Ok(Self {
            bind_addr,
            catalog_path,
            database_url,
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
