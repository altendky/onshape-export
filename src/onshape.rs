use std::sync::atomic::{AtomicU64, Ordering};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use hmac::{Hmac, Mac};
use http::{HeaderMap, HeaderValue, Method, header};
use reqwest::Url;
use serde_json::Value;
use sha2::Sha256;

use crate::catalog::OnshapeSource;
use crate::config::OnshapeConfig;

static NONCE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct OnshapeClient {
    client: reqwest::Client,
    base_url: Url,
    access_key: Option<String>,
    secret_key: Option<String>,
}

impl OnshapeClient {
    pub fn new(config: OnshapeConfig) -> anyhow::Result<Self> {
        let base_url = Url::parse(&config.base_url)?;
        Ok(Self {
            client: reqwest::Client::builder()
                .default_headers(default_headers())
                .build()?,
            base_url,
            access_key: config.access_key,
            secret_key: config.secret_key,
        })
    }

    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    pub fn has_credentials(&self) -> bool {
        self.access_key.is_some() && self.secret_key.is_some()
    }

    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }

    pub async fn fetch_configuration(&self, source: &OnshapeSource) -> anyhow::Result<Value> {
        anyhow::ensure!(
            self.has_credentials(),
            "Onshape credentials are not configured"
        );

        let path = format!(
            "/api/elements/d/{}/v/{}/e/{}/configuration",
            source.document_id, source.version_id, source.element_id
        );
        let mut url = self.base_url.clone();
        url.set_path(&path);
        url.set_query(None);

        let mut headers = signed_headers(
            Method::GET,
            url.path(),
            url.query().unwrap_or_default(),
            self.access_key.as_deref().expect("checked credentials"),
            self.secret_key.as_deref().expect("checked credentials"),
        )?;
        headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));

        let response = self.client.get(url).headers(headers).send().await?;
        let response = response.error_for_status()?;
        Ok(response.json().await?)
    }
}

fn default_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
    headers
}

fn signed_headers(
    method: Method,
    path: &str,
    query: &str,
    access_key: &str,
    secret_key: &str,
) -> anyhow::Result<HeaderMap> {
    let date = httpdate::fmt_http_date(std::time::SystemTime::now());
    let nonce = nonce();
    let content_type = "application/json";
    let signature_input = format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n",
        method.as_str(),
        nonce,
        date,
        content_type,
        path,
        query
    )
    .to_ascii_lowercase();

    let mut mac = Hmac::<Sha256>::new_from_slice(secret_key.as_bytes())?;
    mac.update(signature_input.as_bytes());
    let signature = BASE64.encode(mac.finalize().into_bytes());

    let mut headers = HeaderMap::new();
    headers.insert(header::DATE, HeaderValue::from_str(&date)?);
    headers.insert("On-Nonce", HeaderValue::from_str(&nonce)?);
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_str(&format!("On {access_key}:HmacSHA256:{signature}"))?,
    );
    Ok(headers)
}

fn nonce() -> String {
    let counter = NONCE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();

    format!("{nanos:x}{counter:x}")
}
