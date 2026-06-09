use http::{HeaderMap, HeaderValue, header};
use reqwest::Url;

use crate::config::OnshapeConfig;

#[derive(Debug, Clone)]
pub struct OnshapeClient {
    client: reqwest::Client,
    base_url: Url,
    has_credentials: bool,
}

impl OnshapeClient {
    pub fn new(config: OnshapeConfig) -> anyhow::Result<Self> {
        let base_url = Url::parse(&config.base_url)?;
        let has_credentials = config.access_key.is_some() && config.secret_key.is_some();

        Ok(Self {
            client: reqwest::Client::builder()
                .default_headers(default_headers())
                .build()?,
            base_url,
            has_credentials,
        })
    }

    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    pub fn has_credentials(&self) -> bool {
        self.has_credentials
    }

    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }
}

fn default_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
    headers
}
