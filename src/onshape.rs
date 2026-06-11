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

use crate::config::OnshapeConfig;
use crate::{
    cache_model::{EncodedConfigurationIdentity, ResolvedOnshapeSourceIdentity},
    catalog::{DownloadFormat, DownloadOptions, ElementKind, OnshapeSource, PreviewOptions},
};

static NONCE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct OnshapeClient {
    client: reqwest::Client,
    base_url: Url,
    access_key: Option<String>,
    secret_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EncodedConfiguration {
    pub identity: EncodedConfigurationIdentity,
    pub request_json: String,
    pub response_json: String,
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

        let response =
            onshape_response(self.client.get(url).headers(headers).send().await?).await?;
        Ok(response.json().await?)
    }

    pub async fn resolve_version_microversion(
        &self,
        source: &OnshapeSource,
    ) -> anyhow::Result<ResolvedOnshapeSourceIdentity> {
        anyhow::ensure!(
            self.has_credentials(),
            "Onshape credentials are not configured"
        );

        let path = format!(
            "/api/documents/d/{}/versions/{}",
            source.document_id, source.version_id
        );
        let mut url = self.base_url.clone();
        url.set_path(&path);
        match &source.link_document_id {
            Some(link_document_id) => {
                url.set_query(Some(&format!("linkDocumentId={link_document_id}")))
            }
            None => url.set_query(None),
        }

        let mut headers = signed_headers(
            Method::GET,
            url.path(),
            url.query().unwrap_or_default(),
            self.access_key.as_deref().expect("checked credentials"),
            self.secret_key.as_deref().expect("checked credentials"),
        )?;
        headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));

        let response: Value = onshape_response(self.client.get(url).headers(headers).send().await?)
            .await?
            .json()
            .await?;
        let microversion_id = first_string(&response, &["microversion", "microversionId"])
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Onshape version response did not include a microversion id: {response}"
                )
            })?;

        Ok(ResolvedOnshapeSourceIdentity {
            document_id: source.document_id.clone(),
            version_id: source.version_id.clone(),
            microversion_id,
            element_id: source.element_id.clone(),
            element_kind: source.element_kind.clone(),
            link_document_id: source.link_document_id.clone(),
        })
    }

    pub async fn encode_configuration(
        &self,
        source: &OnshapeSource,
        values: &std::collections::HashMap<String, String>,
    ) -> anyhow::Result<EncodedConfiguration> {
        anyhow::ensure!(
            self.has_credentials(),
            "Onshape credentials are not configured"
        );

        let (path, query, body) = configuration_encoding_request(source, values);
        let request_json = serde_json::to_string(&body)?;
        let mut url = self.base_url.clone();
        url.set_path(&path);
        url.set_query(Some(&query));

        let mut headers = self.signed_json_headers(Method::POST, url.path(), &query)?;
        headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
        let response: Value = onshape_response(
            self.client
                .post(url)
                .headers(headers)
                .json(&body)
                .send()
                .await?,
        )
        .await?
        .json()
        .await?;
        let response_json = serde_json::to_string(&response)?;

        Ok(EncodedConfiguration {
            identity: parse_configuration_encoding_response(&response)?,
            request_json,
            response_json,
        })
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
        let body = gltf_export_body(configuration, options);
        let mut url = self.base_url.clone();
        url.set_path(&path);
        url.set_query(None);

        let mut headers =
            self.signed_json_headers(Method::POST, url.path(), url.query().unwrap_or_default())?;
        headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
        let response = self
            .client
            .post(url)
            .headers(headers)
            .json(&body)
            .send()
            .await?;
        let response: Value = onshape_response(response).await?.json().await?;

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
        let response = self
            .client
            .post(url)
            .headers(headers)
            .json(&body)
            .send()
            .await?;
        let response: Value = onshape_response(response).await?.json().await?;

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
            let response = self.client.get(url).headers(headers).send().await?;
            let response: Value = onshape_response(response).await?.json().await?;

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
        let response = self.client.get(url).headers(headers).send().await?;
        let bytes = onshape_response(response).await?.bytes().await?;
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

fn gltf_export_body(configuration: &str, options: &PreviewOptions) -> Value {
    json!({
        "advancedParams": {
            "configuration": configuration,
        },
        "meshParams": {
            "resolution": options.resolution.as_deref().unwrap_or("MEDIUM"),
        },
        "grouping": true,
        "storeInDocument": false,
        "notifyUser": false,
        "triggerAutoDownload": false,
    })
}

fn configuration_encoding_request(
    source: &OnshapeSource,
    values: &std::collections::HashMap<String, String>,
) -> (String, String, Value) {
    let path = format!(
        "/api/elements/d/{}/e/{}/configurationencodings",
        source.document_id, source.element_id
    );

    let mut query = format!("versionId={}", source.version_id);
    if let Some(link_document_id) = &source.link_document_id {
        query.push_str("&linkDocumentId=");
        query.push_str(link_document_id);
    }

    let mut keys = values.keys().collect::<Vec<_>>();
    keys.sort();
    let parameters = keys
        .into_iter()
        .map(|key| {
            json!({
                "parameterId": key,
                "parameterValue": values[key],
            })
        })
        .collect::<Vec<_>>();

    (path, query, json!({ "parameters": parameters }))
}

fn parse_configuration_encoding_response(
    response: &Value,
) -> anyhow::Result<EncodedConfigurationIdentity> {
    let encoded_id = first_string(response, &["encodedId"])
        .ok_or_else(|| anyhow::anyhow!("Onshape encoding response did not include encodedId"))?;
    let query_param = first_string(response, &["queryParam"])
        .ok_or_else(|| anyhow::anyhow!("Onshape encoding response did not include queryParam"))?;
    Ok(EncodedConfigurationIdentity {
        encoded_id,
        query_param,
    })
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

async fn onshape_response(response: reqwest::Response) -> anyhow::Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }

    let url = response.url().clone();
    let body = response
        .text()
        .await
        .unwrap_or_else(|error| format!("<failed to read Onshape error response body: {error}>"));
    anyhow::bail!("Onshape request failed: {status} {url}: {body}")
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::ElementKind;
    use std::collections::HashMap;

    #[test]
    fn gltf_export_body_requests_grouped_preview() {
        let body = gltf_export_body(
            "width=10 mm",
            &PreviewOptions {
                resolution: Some("FINE".to_owned()),
            },
        );

        assert_eq!(body["advancedParams"]["configuration"], "width=10 mm");
        assert_eq!(body["meshParams"]["resolution"], "FINE");
        assert_eq!(body["grouping"], true);
        assert_eq!(body["storeInDocument"], false);
        assert_eq!(body["notifyUser"], false);
        assert_eq!(body["triggerAutoDownload"], false);
    }

    #[test]
    fn configuration_encoding_request_uses_version_query_and_sorted_parameters() {
        let source = OnshapeSource {
            document_id: "did".to_owned(),
            version_id: "vid".to_owned(),
            element_id: "eid".to_owned(),
            element_kind: ElementKind::PartStudio,
            link_document_id: Some("ldid".to_owned()),
        };
        let values = HashMap::from([
            ("width".to_owned(), "10 mm".to_owned()),
            ("enabled".to_owned(), "true".to_owned()),
        ]);

        let (path, query, body) = configuration_encoding_request(&source, &values);

        assert_eq!(path, "/api/elements/d/did/e/eid/configurationencodings");
        assert_eq!(query, "versionId=vid&linkDocumentId=ldid");
        assert_eq!(
            body,
            json!({
                "parameters": [
                    {
                        "parameterId": "enabled",
                        "parameterValue": "true",
                    },
                    {
                        "parameterId": "width",
                        "parameterValue": "10 mm",
                    },
                ]
            })
        );
    }

    #[test]
    fn parse_configuration_encoding_response_requires_expected_fields() {
        let parsed = parse_configuration_encoding_response(&json!({
            "encodedId": "enc-123",
            "queryParam": "configuration=enc-123",
        }))
        .unwrap();

        assert_eq!(parsed.encoded_id, "enc-123");
        assert_eq!(parsed.query_param, "configuration=enc-123");
        assert!(parse_configuration_encoding_response(&json!({"encodedId": "enc"})).is_err());
    }
}
