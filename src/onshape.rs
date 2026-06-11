use std::{
    collections::BTreeMap,
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
    cache_model::{EncodedConfigurationIdentity, RequestIdentity, ResolvedOnshapeSourceIdentity},
    catalog::{DownloadFormat, DownloadOptions, ElementKind, OnshapeSource, PreviewOptions},
};

static NONCE_COUNTER: AtomicU64 = AtomicU64::new(0);
const API_SPEC_VERSION: Option<&str> = None;
const PREVIEW_GLTF_DEFAULTS_POLICY_VERSION: &str = "preview-gltf-defaults-v1";
const PREVIEW_GLTF_REQUEST_BUILDER_VERSION: &str = "preview-gltf-request-v1";
const STEP_EXPORT_DEFAULTS_POLICY_VERSION: &str = "step-export-defaults-v1";
const STEP_EXPORT_REQUEST_BUILDER_VERSION: &str = "step-export-request-v1";
const TRANSLATION_EXPORT_DEFAULTS_POLICY_VERSION: &str = "translation-export-defaults-v1";
const TRANSLATION_EXPORT_REQUEST_BUILDER_VERSION: &str = "translation-export-request-v1";

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

type CanonicalRequestPathParams = BTreeMap<String, String>;
type CanonicalRequestQuery = BTreeMap<String, String>;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalExportRequest {
    pub operation: String,
    pub method: String,
    pub path: String,
    pub identity: RequestIdentity<CanonicalRequestPathParams, CanonicalRequestQuery, Value>,
}

impl CanonicalExportRequest {
    pub fn request_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(&self.identity)?)
    }

    fn document_id(&self) -> anyhow::Result<&str> {
        self.identity
            .path_params
            .get("did")
            .map(String::as_str)
            .ok_or_else(|| anyhow::anyhow!("canonical export request did not include document id"))
    }
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
        let request = self.build_preview_glb_export_request(
            source,
            &EncodedConfigurationIdentity {
                encoded_id: configuration.to_owned(),
                query_param: format!("configuration={configuration}"),
            },
            options,
        );
        self.execute_export_request(&request).await
    }

    pub async fn export_download(
        &self,
        source: &OnshapeSource,
        configuration: &str,
        format: DownloadFormat,
        options: &DownloadOptions,
    ) -> anyhow::Result<Vec<u8>> {
        let request = self.build_download_export_request(
            source,
            &EncodedConfigurationIdentity {
                encoded_id: configuration.to_owned(),
                query_param: format!("configuration={configuration}"),
            },
            format,
            options,
        );
        self.execute_export_request(&request).await
    }

    pub fn build_preview_glb_export_request(
        &self,
        source: &OnshapeSource,
        configuration: &EncodedConfigurationIdentity,
        options: &PreviewOptions,
    ) -> CanonicalExportRequest {
        let collection = element_collection(source);
        let path = format!(
            "/api/{collection}/d/{}/v/{}/e/{}/export/gltf",
            source.document_id, source.version_id, source.element_id
        );
        let operation = format!("create-{}-gltf-export", source.element_kind.key());
        let body = gltf_export_body(&configuration.encoded_id, options);

        CanonicalExportRequest {
            operation: operation.clone(),
            method: Method::POST.as_str().to_owned(),
            path,
            identity: RequestIdentity {
                api_host_class: api_host_class(&self.base_url),
                api_spec_version: API_SPEC_VERSION.map(str::to_owned),
                operation,
                method: Method::POST.as_str().to_owned(),
                path_template: "/api/{collection}/d/{did}/v/{vid}/e/{eid}/export/gltf".to_owned(),
                path_params: export_path_params(collection, source),
                query: CanonicalRequestQuery::new(),
                body,
                encoded_configuration: Some(configuration.clone()),
                defaults_policy_version: PREVIEW_GLTF_DEFAULTS_POLICY_VERSION.to_owned(),
                request_builder_version: PREVIEW_GLTF_REQUEST_BUILDER_VERSION.to_owned(),
            },
        }
    }

    pub fn build_download_export_request(
        &self,
        source: &OnshapeSource,
        configuration: &EncodedConfigurationIdentity,
        format: DownloadFormat,
        options: &DownloadOptions,
    ) -> CanonicalExportRequest {
        match format {
            DownloadFormat::Step => self.build_step_export_request(source, configuration, options),
            DownloadFormat::Stl | DownloadFormat::ThreeMf => {
                self.build_translation_export_request(source, configuration, format)
            }
        }
    }

    pub async fn execute_export_request(
        &self,
        request: &CanonicalExportRequest,
    ) -> anyhow::Result<Vec<u8>> {
        let translation_id = self.start_export_request(request).await?;
        let external_data_id = self
            .poll_translation(request.document_id()?, &translation_id)
            .await?;
        self.download_external_data(request.document_id()?, &external_data_id)
            .await
    }

    fn build_step_export_request(
        &self,
        source: &OnshapeSource,
        configuration: &EncodedConfigurationIdentity,
        options: &DownloadOptions,
    ) -> CanonicalExportRequest {
        let collection = element_collection(source);
        let path = format!(
            "/api/{collection}/d/{}/v/{}/e/{}/export/step",
            source.document_id, source.version_id, source.element_id
        );
        let body = json!({
            "advancedParams": {
                "configuration": configuration.encoded_id,
            },
            "stepVersionString": options.step_version_string.as_deref().unwrap_or("AP242"),
            "storeInDocument": false,
            "notifyUser": false,
            "triggerAutoDownload": false,
        });

        CanonicalExportRequest {
            operation: "create-step-export".to_owned(),
            method: Method::POST.as_str().to_owned(),
            path,
            identity: RequestIdentity {
                api_host_class: api_host_class(&self.base_url),
                api_spec_version: API_SPEC_VERSION.map(str::to_owned),
                operation: "create-step-export".to_owned(),
                method: Method::POST.as_str().to_owned(),
                path_template: "/api/{collection}/d/{did}/v/{vid}/e/{eid}/export/step".to_owned(),
                path_params: export_path_params(collection, source),
                query: CanonicalRequestQuery::new(),
                body,
                encoded_configuration: Some(configuration.clone()),
                defaults_policy_version: STEP_EXPORT_DEFAULTS_POLICY_VERSION.to_owned(),
                request_builder_version: STEP_EXPORT_REQUEST_BUILDER_VERSION.to_owned(),
            },
        }
    }

    fn build_translation_export_request(
        &self,
        source: &OnshapeSource,
        configuration: &EncodedConfigurationIdentity,
        format: DownloadFormat,
    ) -> CanonicalExportRequest {
        let collection = element_collection(source);
        let path = format!(
            "/api/{collection}/d/{}/v/{}/e/{}/translations",
            source.document_id, source.version_id, source.element_id
        );
        let format_label = format.label().to_owned();
        let body = json!({
            "formatName": format_label,
            "storeInDocument": false,
            "notifyUser": false,
            "triggerAutoDownload": false,
            "configuration": configuration.encoded_id,
        });

        CanonicalExportRequest {
            operation: format!("create-{}-translation", format.slug()),
            method: Method::POST.as_str().to_owned(),
            path,
            identity: RequestIdentity {
                api_host_class: api_host_class(&self.base_url),
                api_spec_version: API_SPEC_VERSION.map(str::to_owned),
                operation: format!("create-{}-translation", format.slug()),
                method: Method::POST.as_str().to_owned(),
                path_template: "/api/{collection}/d/{did}/v/{vid}/e/{eid}/translations".to_owned(),
                path_params: export_path_params(collection, source),
                query: CanonicalRequestQuery::new(),
                body,
                encoded_configuration: Some(configuration.clone()),
                defaults_policy_version: TRANSLATION_EXPORT_DEFAULTS_POLICY_VERSION.to_owned(),
                request_builder_version: TRANSLATION_EXPORT_REQUEST_BUILDER_VERSION.to_owned(),
            },
        }
    }

    async fn start_export_request(
        &self,
        request: &CanonicalExportRequest,
    ) -> anyhow::Result<String> {
        anyhow::ensure!(
            self.has_credentials(),
            "Onshape credentials are not configured"
        );
        let method = Method::from_bytes(request.method.as_bytes())?;
        let mut url = self.base_url.clone();
        url.set_path(&request.path);
        if request.identity.query.is_empty() {
            url.set_query(None);
        } else {
            let mut query = url.query_pairs_mut();
            for (key, value) in &request.identity.query {
                query.append_pair(key, value);
            }
        }

        let mut headers =
            self.signed_json_headers(method.clone(), url.path(), url.query().unwrap_or_default())?;
        headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
        let response = self
            .client
            .request(method, url)
            .headers(headers)
            .json(&request.identity.body)
            .send()
            .await?;
        let response: Value = onshape_response(response).await?.json().await?;

        first_string(&response, &["id", "translationId"]).ok_or_else(|| {
            anyhow::anyhow!(
                "Onshape {} response did not include a translation id",
                request.operation
            )
        })
    }

    async fn poll_translation(
        &self,
        document_id: &str,
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
                        .ok_or_else(|| anyhow::anyhow!("Onshape translation completed without external data for document {document_id}"));
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
        document_id: &str,
        external_data_id: &str,
    ) -> anyhow::Result<Vec<u8>> {
        let path = format!(
            "/api/documents/d/{}/externaldata/{}",
            document_id, external_data_id
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

fn export_path_params(
    source_collection: &str,
    source: &OnshapeSource,
) -> CanonicalRequestPathParams {
    BTreeMap::from([
        ("collection".to_owned(), source_collection.to_owned()),
        ("did".to_owned(), source.document_id.clone()),
        ("vid".to_owned(), source.version_id.clone()),
        ("eid".to_owned(), source.element_id.clone()),
    ])
}

fn api_host_class(base_url: &Url) -> String {
    base_url.host_str().unwrap_or_default().to_owned()
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
    fn preview_export_request_uses_encoded_configuration_and_versions() {
        let client = OnshapeClient::new(OnshapeConfig {
            base_url: "https://cad.onshape.com".to_owned(),
            access_key: None,
            secret_key: None,
        })
        .unwrap();
        let source = OnshapeSource {
            document_id: "did".to_owned(),
            version_id: "vid".to_owned(),
            element_id: "eid".to_owned(),
            element_kind: ElementKind::PartStudio,
            link_document_id: None,
        };
        let request = client.build_preview_glb_export_request(
            &source,
            &EncodedConfigurationIdentity {
                encoded_id: "enc-123".to_owned(),
                query_param: "configuration=enc-123".to_owned(),
            },
            &PreviewOptions {
                resolution: Some("FINE".to_owned()),
            },
        );

        assert_eq!(request.operation, "create-part_studio-gltf-export");
        assert_eq!(
            request.path,
            "/api/partstudios/d/did/v/vid/e/eid/export/gltf"
        );
        assert_eq!(
            request.identity.path_template,
            "/api/{collection}/d/{did}/v/{vid}/e/{eid}/export/gltf"
        );
        assert_eq!(request.identity.path_params["collection"], "partstudios");
        assert_eq!(
            request.identity.body["advancedParams"]["configuration"],
            "enc-123"
        );
        assert_eq!(request.identity.body["meshParams"]["resolution"], "FINE");
        assert_eq!(
            request.identity.defaults_policy_version,
            PREVIEW_GLTF_DEFAULTS_POLICY_VERSION
        );
        assert_eq!(
            request.identity.request_builder_version,
            PREVIEW_GLTF_REQUEST_BUILDER_VERSION
        );
    }

    #[test]
    fn step_export_request_uses_explicit_default_step_version() {
        let client = OnshapeClient::new(OnshapeConfig {
            base_url: "https://cad.onshape.com".to_owned(),
            access_key: None,
            secret_key: None,
        })
        .unwrap();
        let source = OnshapeSource {
            document_id: "did".to_owned(),
            version_id: "vid".to_owned(),
            element_id: "eid".to_owned(),
            element_kind: ElementKind::PartStudio,
            link_document_id: None,
        };
        let request = client.build_download_export_request(
            &source,
            &EncodedConfigurationIdentity {
                encoded_id: "enc-123".to_owned(),
                query_param: "configuration=enc-123".to_owned(),
            },
            DownloadFormat::Step,
            &DownloadOptions::default(),
        );

        assert_eq!(request.operation, "create-step-export");
        assert_eq!(
            request.path,
            "/api/partstudios/d/did/v/vid/e/eid/export/step"
        );
        assert_eq!(
            request.identity.body["advancedParams"]["configuration"],
            "enc-123"
        );
        assert_eq!(request.identity.body["stepVersionString"], "AP242");
        assert_eq!(
            request.identity.defaults_policy_version,
            STEP_EXPORT_DEFAULTS_POLICY_VERSION
        );
        assert_eq!(
            request.identity.request_builder_version,
            STEP_EXPORT_REQUEST_BUILDER_VERSION
        );
    }

    #[test]
    fn translation_export_request_uses_format_specific_body() {
        let client = OnshapeClient::new(OnshapeConfig {
            base_url: "https://cad.onshape.com".to_owned(),
            access_key: None,
            secret_key: None,
        })
        .unwrap();
        let source = OnshapeSource {
            document_id: "did".to_owned(),
            version_id: "vid".to_owned(),
            element_id: "eid".to_owned(),
            element_kind: ElementKind::Assembly,
            link_document_id: None,
        };
        let request = client.build_download_export_request(
            &source,
            &EncodedConfigurationIdentity {
                encoded_id: "enc-123".to_owned(),
                query_param: "configuration=enc-123".to_owned(),
            },
            DownloadFormat::ThreeMf,
            &DownloadOptions::default(),
        );

        assert_eq!(request.operation, "create-3mf-translation");
        assert_eq!(
            request.path,
            "/api/assemblies/d/did/v/vid/e/eid/translations"
        );
        assert_eq!(request.identity.body["formatName"], "3MF");
        assert_eq!(request.identity.body["configuration"], "enc-123");
        assert_eq!(
            request.identity.defaults_policy_version,
            TRANSLATION_EXPORT_DEFAULTS_POLICY_VERSION
        );
        assert_eq!(
            request.identity.request_builder_version,
            TRANSLATION_EXPORT_REQUEST_BUILDER_VERSION
        );
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
