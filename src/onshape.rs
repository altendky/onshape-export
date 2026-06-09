use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use hmac::{Hmac, Mac};
use http::{HeaderMap, HeaderValue, Method, header};
use reqwest::Url;
use serde_json::{Value, json};
use sha2::Sha256;

use crate::catalog::{DownloadFormat, DownloadOptions, ElementKind, OnshapeSource, PreviewOptions};
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

    pub async fn export_glb(
        &self,
        source: &OnshapeSource,
        configuration: &str,
        options: &PreviewOptions,
    ) -> anyhow::Result<Vec<u8>> {
        let translation_id = self
            .start_glb_export(source, configuration, options)
            .await?;
        let external_data_id = self.poll_translation(source, &translation_id).await?;
        self.download_external_data(source, &external_data_id).await
    }

    pub async fn export_download(
        &self,
        source: &OnshapeSource,
        configuration: &str,
        format: DownloadFormat,
        options: &DownloadOptions,
    ) -> anyhow::Result<Vec<u8>> {
        let translation_id = match format {
            DownloadFormat::Step => {
                self.start_step_export(source, configuration, options)
                    .await?
            }
            DownloadFormat::Stl | DownloadFormat::ThreeMf => {
                self.start_translation_export(source, configuration, format)
                    .await?
            }
        };
        let external_data_id = self.poll_translation(source, &translation_id).await?;
        self.download_external_data(source, &external_data_id).await
    }

    async fn start_glb_export(
        &self,
        source: &OnshapeSource,
        configuration: &str,
        options: &PreviewOptions,
    ) -> anyhow::Result<String> {
        anyhow::ensure!(
            self.has_credentials(),
            "Onshape credentials are not configured"
        );

        let collection = match source.element_kind {
            ElementKind::PartStudio => "partstudios",
            ElementKind::Assembly => "assemblies",
        };
        let path = format!(
            "/api/{collection}/d/{}/v/{}/e/{}/export/gltf",
            source.document_id, source.version_id, source.element_id
        );
        let body = json!({
            "advancedParams": {
                "configuration": configuration,
            },
            "meshParams": {
                "resolution": options.resolution.as_deref().unwrap_or("MEDIUM"),
            },
            "storeInDocument": false,
            "notifyUser": false,
            "triggerAutoDownload": false,
        });
        let mut url = self.base_url.clone();
        url.set_path(&path);
        url.set_query(None);

        let mut headers =
            self.signed_json_headers(Method::POST, url.path(), url.query().unwrap_or_default())?;
        headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
        let response: Value = self
            .client
            .post(url)
            .headers(headers)
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        first_string(&response, &["id", "translationId"]).ok_or_else(|| {
            anyhow::anyhow!("Onshape GLB export response did not include a translation id")
        })
    }

    async fn start_step_export(
        &self,
        source: &OnshapeSource,
        configuration: &str,
        options: &DownloadOptions,
    ) -> anyhow::Result<String> {
        anyhow::ensure!(
            self.has_credentials(),
            "Onshape credentials are not configured"
        );

        let collection = element_collection(source);
        let path = format!(
            "/api/{collection}/d/{}/v/{}/e/{}/export/step",
            source.document_id, source.version_id, source.element_id
        );
        let body = json!({
            "advancedParams": {
                "configuration": configuration,
            },
            "stepVersionString": options.step_version_string.as_deref().unwrap_or("AP242"),
            "storeInDocument": false,
            "notifyUser": false,
            "triggerAutoDownload": false,
        });
        self.start_json_translation(&path, body, "STEP").await
    }

    async fn start_translation_export(
        &self,
        source: &OnshapeSource,
        configuration: &str,
        format: DownloadFormat,
    ) -> anyhow::Result<String> {
        anyhow::ensure!(
            self.has_credentials(),
            "Onshape credentials are not configured"
        );

        let collection = element_collection(source);
        let path = format!(
            "/api/{collection}/d/{}/v/{}/e/{}/translations",
            source.document_id, source.version_id, source.element_id
        );
        let body = json!({
            "formatName": format.label(),
            "storeInDocument": false,
            "notifyUser": false,
            "triggerAutoDownload": false,
            "configuration": configuration,
        });
        self.start_json_translation(&path, body, format.label())
            .await
    }

    async fn start_json_translation(
        &self,
        path: &str,
        body: Value,
        label: &str,
    ) -> anyhow::Result<String> {
        let mut url = self.base_url.clone();
        url.set_path(path);
        url.set_query(None);

        let mut headers =
            self.signed_json_headers(Method::POST, url.path(), url.query().unwrap_or_default())?;
        headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
        let response: Value = self
            .client
            .post(url)
            .headers(headers)
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        first_string(&response, &["id", "translationId"]).ok_or_else(|| {
            anyhow::anyhow!("Onshape {label} export response did not include a translation id")
        })
    }

    async fn poll_translation(
        &self,
        source: &OnshapeSource,
        translation_id: &str,
    ) -> anyhow::Result<String> {
        let path = format!("/api/translations/{translation_id}");
        let delays = [2, 4, 8, 15, 30, 30, 30, 30];
        for delay in delays {
            let mut url = self.base_url.clone();
            url.set_path(&path);
            url.set_query(None);
            let mut headers =
                self.signed_json_headers(Method::GET, url.path(), url.query().unwrap_or_default())?;
            headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
            let response: Value = self
                .client
                .get(url)
                .headers(headers)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;

            match first_string(&response, &["requestState", "state"]).as_deref() {
                Some("DONE") => {
                    return first_array_string(&response, "resultExternalDataIds")
                        .ok_or_else(|| anyhow::anyhow!("Onshape translation completed without external data for document {}", source.document_id));
                }
                Some("FAILED") => {
                    return Err(anyhow::anyhow!(
                        "Onshape GLB translation failed: {response}"
                    ));
                }
                _ => tokio::time::sleep(Duration::from_secs(delay)).await,
            }
        }

        Err(anyhow::anyhow!("Onshape GLB translation timed out"))
    }

    async fn download_external_data(
        &self,
        source: &OnshapeSource,
        external_data_id: &str,
    ) -> anyhow::Result<Vec<u8>> {
        let path = format!(
            "/api/documents/d/{}/externaldata/{}",
            source.document_id, external_data_id
        );
        let mut url = self.base_url.clone();
        url.set_path(&path);
        url.set_query(None);
        let mut headers =
            self.signed_json_headers(Method::GET, url.path(), url.query().unwrap_or_default())?;
        headers.insert(header::ACCEPT, HeaderValue::from_static("*/*"));
        let bytes = self
            .client
            .get(url)
            .headers(headers)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        Ok(bytes.to_vec())
    }

    fn signed_json_headers(
        &self,
        method: Method,
        path: &str,
        query: &str,
    ) -> anyhow::Result<HeaderMap> {
        signed_headers(
            method,
            path,
            query,
            self.access_key.as_deref().expect("checked credentials"),
            self.secret_key.as_deref().expect("checked credentials"),
        )
    }
}

fn element_collection(source: &OnshapeSource) -> &'static str {
    match source.element_kind {
        ElementKind::PartStudio => "partstudios",
        ElementKind::Assembly => "assemblies",
    }
}

fn first_string(value: &Value, names: &[&str]) -> Option<String> {
    let object = value.as_object()?;
    names
        .iter()
        .filter_map(|name| object.get(*name))
        .find_map(|value| value.as_str().map(ToOwned::to_owned))
}

fn first_array_string(value: &Value, name: &str) -> Option<String> {
    value
        .get(name)?
        .as_array()?
        .iter()
        .find_map(|value| value.as_str().map(ToOwned::to_owned))
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
