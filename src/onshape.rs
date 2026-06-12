use std::{
    collections::{BTreeMap, BTreeSet},
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

    pub fn document_id(&self) -> anyhow::Result<&str> {
        self.identity
            .path_params
            .get("did")
            .map(String::as_str)
            .ok_or_else(|| anyhow::anyhow!("canonical export request did not include document id"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartedTranslation {
    pub translation_id: String,
    pub state: String,
    pub response_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolledTranslation {
    pub state: String,
    pub final_response_json: String,
    pub poll_state_json: String,
    pub result_external_data_ids: Vec<String>,
    pub result_element_ids: Vec<String>,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadedExternalData {
    pub bytes: Vec<u8>,
    pub content_type: Option<String>,
    pub response_headers_json: String,
    pub original_filename: Option<String>,
    pub filename_source: Option<String>,
    pub etag: Option<String>,
}

impl PolledTranslation {
    pub fn single_external_data_id(&self) -> anyhow::Result<&str> {
        match self.result_external_data_ids.as_slice() {
            [external_data_id] => Ok(external_data_id),
            [] => anyhow::bail!("Onshape translation completed without external data"),
            _ => anyhow::bail!(
                "Onshape translation returned {} downloadable results; expected exactly one",
                self.result_external_data_ids.len()
            ),
        }
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
        if let Some(link_document_id) = &source.link_document_id {
            url.query_pairs_mut()
                .append_pair("linkDocumentId", link_document_id);
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
        values: &BTreeMap<String, String>,
    ) -> anyhow::Result<EncodedConfiguration> {
        anyhow::ensure!(
            self.has_credentials(),
            "Onshape credentials are not configured"
        );

        let (path, body) = configuration_encoding_request(source, values);
        let request_json = serde_json::to_string(&body)?;
        let mut url = self.base_url.clone();
        url.set_path(&path);
        url.query_pairs_mut()
            .append_pair("versionId", &source.version_id);
        if let Some(link_document_id) = &source.link_document_id {
            url.query_pairs_mut()
                .append_pair("linkDocumentId", link_document_id);
        }
        let query = url.query().unwrap_or_default().to_owned();

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

    pub async fn start_export_request(
        &self,
        request: &CanonicalExportRequest,
    ) -> anyhow::Result<StartedTranslation> {
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
        parse_started_translation(&response, &request.operation)
    }

    pub async fn poll_translation(
        &self,
        document_id: &str,
        translation_id: &str,
    ) -> anyhow::Result<PolledTranslation> {
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
            let polled = parse_polled_translation(&response)?;

            match polled.state.as_str() {
                "DONE" => return Ok(polled),
                "FAILED" => return Ok(polled),
                _ => tokio::time::sleep(Duration::from_secs(delay)).await,
            }
        }

        Err(anyhow::anyhow!(
            "Onshape translation timed out for document {document_id}"
        ))
    }

    pub async fn download_external_data(
        &self,
        document_id: &str,
        external_data_id: &str,
    ) -> anyhow::Result<DownloadedExternalData> {
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
        let response =
            onshape_response(self.client.get(url).headers(headers).send().await?).await?;
        let response_headers = response.headers().clone();
        let content_type = response_headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let etag = response_headers
            .get(header::ETAG)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let (original_filename, filename_source) = download_filename(&response_headers);
        let response_headers_json = header_map_json(&response_headers)?;
        let bytes = response.bytes().await?;
        Ok(DownloadedExternalData {
            bytes: bytes.to_vec(),
            content_type,
            response_headers_json,
            original_filename,
            filename_source,
            etag,
        })
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
    values: &BTreeMap<String, String>,
) -> (String, Value) {
    let path = format!(
        "/api/elements/d/{}/e/{}/configurationencodings",
        source.document_id, source.element_id
    );

    let parameters = values
        .keys()
        .map(|key| {
            json!({
                "parameterId": key,
                "parameterValue": values[key],
            })
        })
        .collect::<Vec<_>>();

    (path, json!({ "parameters": parameters }))
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

fn parse_started_translation(
    response: &Value,
    operation: &str,
) -> anyhow::Result<StartedTranslation> {
    let translation_id = first_string(response, &["id", "translationId"]).ok_or_else(|| {
        anyhow::anyhow!("Onshape {operation} response did not include a translation id")
    })?;
    let state =
        first_string(response, &["requestState", "state"]).unwrap_or_else(|| "ACTIVE".to_owned());
    Ok(StartedTranslation {
        translation_id,
        state,
        response_json: serde_json::to_string(response)?,
    })
}

fn parse_polled_translation(response: &Value) -> anyhow::Result<PolledTranslation> {
    let state = first_string(response, &["requestState", "state"]).ok_or_else(|| {
        anyhow::anyhow!("Onshape translation poll response did not include a state")
    })?;
    let final_response_json = serde_json::to_string(response)?;
    let result_external_data_ids = array_strings(response, "resultExternalDataIds");
    let result_element_ids = array_strings(response, "resultElementIds");
    let failure_reason = first_string(
        response,
        &["failureReason", "failureMessage", "message", "errorMessage"],
    );
    Ok(PolledTranslation {
        state,
        // The cache model persists the final response and terminal poll snapshot
        // separately even when the last poll payload matches the final response.
        poll_state_json: final_response_json.clone(),
        final_response_json,
        result_external_data_ids,
        result_element_ids,
        failure_reason,
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

fn header_map_json(headers: &HeaderMap) -> anyhow::Result<String> {
    let mut names = BTreeSet::new();
    for name in headers.keys() {
        names.insert(name.as_str().to_owned());
    }

    let mut json = BTreeMap::<String, Vec<String>>::new();
    for name in names {
        let values = headers
            .get_all(name.as_str())
            .iter()
            .filter_map(|value| value.to_str().ok().map(str::to_owned))
            .collect::<Vec<_>>();
        json.insert(name, values);
    }

    Ok(serde_json::to_string(&json)?)
}

fn download_filename(headers: &HeaderMap) -> (Option<String>, Option<String>) {
    let Some(value) = headers.get(header::CONTENT_DISPOSITION) else {
        return (None, None);
    };
    let Ok(value) = value.to_str() else {
        return (None, None);
    };

    if let Some(filename) = content_disposition_filename_star(value) {
        return (
            Some(filename),
            Some("content-disposition-filename*".to_owned()),
        );
    }
    if let Some(filename) = content_disposition_filename(value) {
        return (Some(filename), Some("content-disposition".to_owned()));
    }

    (None, None)
}

fn content_disposition_filename(value: &str) -> Option<String> {
    content_disposition_parameters(value)
        .into_iter()
        .find_map(|(name, value)| {
            (name.eq_ignore_ascii_case("filename") && !value.is_empty()).then_some(value)
        })
}

fn content_disposition_filename_star(value: &str) -> Option<String> {
    let encoded = content_disposition_parameters(value)
        .into_iter()
        .find_map(|(name, value)| {
            (name.eq_ignore_ascii_case("filename*") && !value.is_empty()).then_some(value)
        })?;
    decode_rfc8187_filename(&encoded)
}

fn content_disposition_parameters(value: &str) -> Vec<(String, String)> {
    split_header_parameters(value)
        .into_iter()
        .skip(1)
        .filter_map(|part| {
            let (name, value) = part.trim().split_once('=')?;
            Some((name.trim().to_owned(), unquote_header_value(value.trim())))
        })
        .collect()
}

fn split_header_parameters(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut escaped = false;

    for ch in value.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' if quoted => {
                current.push(ch);
                escaped = true;
            }
            '"' => {
                quoted = !quoted;
                current.push(ch);
            }
            ';' if !quoted => {
                parts.push(current.trim().to_owned());
                current.clear();
            }
            _ => current.push(ch),
        }
    }

    parts.push(current.trim().to_owned());
    parts
}

fn unquote_header_value(value: &str) -> String {
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        let mut unescaped = String::with_capacity(value.len() - 2);
        let mut escaped = false;
        for ch in value[1..value.len() - 1].chars() {
            if escaped {
                unescaped.push(ch);
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else {
                unescaped.push(ch);
            }
        }
        unescaped
    } else {
        value.to_owned()
    }
}

fn decode_rfc8187_filename(value: &str) -> Option<String> {
    let (charset, rest) = value.split_once("''")?;
    if !charset.eq_ignore_ascii_case("UTF-8") {
        return None;
    }
    percent_decode(rest)
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let high = hex_nibble(bytes[index + 1])?;
                let low = hex_nibble(bytes[index + 2])?;
                decoded.push((high << 4) | low);
                index += 3;
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(decoded).ok()
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn array_strings(value: &Value, name: &str) -> Vec<String> {
    value
        .get(name)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str().map(ToOwned::to_owned))
        .collect()
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
    use std::collections::BTreeMap;

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
    fn configuration_encoding_request_uses_sorted_parameters() {
        let source = OnshapeSource {
            document_id: "did".to_owned(),
            version_id: "vid".to_owned(),
            element_id: "eid".to_owned(),
            element_kind: ElementKind::PartStudio,
            link_document_id: Some("ldid".to_owned()),
        };
        let values = BTreeMap::from([
            ("width".to_owned(), "10 mm".to_owned()),
            ("enabled".to_owned(), "true".to_owned()),
        ]);

        let (path, body) = configuration_encoding_request(&source, &values);

        assert_eq!(path, "/api/elements/d/did/e/eid/configurationencodings");
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
    fn source_queries_are_url_encoded_before_signing() {
        let mut configuration_url = Url::parse("https://cad.onshape.com").unwrap();
        configuration_url.set_path("/api/elements/d/did/e/eid/configurationencodings");
        configuration_url
            .query_pairs_mut()
            .append_pair("versionId", "vid&= value")
            .append_pair("linkDocumentId", "ld/id?");

        assert_eq!(
            configuration_url.query(),
            Some("versionId=vid%26%3D+value&linkDocumentId=ld%2Fid%3F")
        );

        let mut version_url = Url::parse("https://cad.onshape.com").unwrap();
        version_url.set_path("/api/documents/d/did/versions/vid");
        version_url
            .query_pairs_mut()
            .append_pair("linkDocumentId", "ld/id?");

        assert_eq!(version_url.query(), Some("linkDocumentId=ld%2Fid%3F"));
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

    #[test]
    fn parse_started_translation_accepts_id_and_default_state() {
        let parsed = parse_started_translation(&json!({"id": "tid"}), "create-preview").unwrap();

        assert_eq!(parsed.translation_id, "tid");
        assert_eq!(parsed.state, "ACTIVE");
        assert_eq!(parsed.response_json, r#"{"id":"tid"}"#);
    }

    #[test]
    fn parse_started_translation_requires_translation_id() {
        assert!(parse_started_translation(&json!({"state": "ACTIVE"}), "create-preview").is_err());
    }

    #[test]
    fn parse_polled_translation_collects_results_and_failure_reason() {
        let parsed = parse_polled_translation(&json!({
            "requestState": "DONE",
            "resultExternalDataIds": ["fid"],
            "resultElementIds": ["eid"]
        }))
        .unwrap();

        assert_eq!(parsed.state, "DONE");
        assert_eq!(parsed.result_external_data_ids, ["fid"]);
        assert_eq!(parsed.result_element_ids, ["eid"]);
        assert_eq!(parsed.single_external_data_id().unwrap(), "fid");
    }

    #[test]
    fn parse_polled_translation_requires_state() {
        assert!(parse_polled_translation(&json!({"resultExternalDataIds": ["fid"]})).is_err());
    }

    #[test]
    fn polled_translation_rejects_missing_or_multiple_download_results() {
        let missing = parse_polled_translation(&json!({"requestState": "DONE"})).unwrap();
        assert!(missing.single_external_data_id().is_err());

        let multiple = parse_polled_translation(&json!({
            "requestState": "DONE",
            "resultExternalDataIds": ["a", "b"]
        }))
        .unwrap();
        assert!(multiple.single_external_data_id().is_err());
    }

    #[test]
    fn parse_polled_translation_captures_failed_reason() {
        let parsed = parse_polled_translation(&json!({
            "state": "FAILED",
            "failureReason": "bad geometry"
        }))
        .unwrap();

        assert_eq!(parsed.failure_reason.as_deref(), Some("bad geometry"));
    }

    #[test]
    fn extracts_download_filename_from_content_disposition() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_static("attachment; filename=\"model.step\""),
        );

        assert_eq!(
            download_filename(&headers),
            (
                Some("model.step".to_owned()),
                Some("content-disposition".to_owned())
            )
        );
    }

    #[test]
    fn prefers_rfc8187_download_filename() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_static(
                "attachment; filename=\"fallback.zip\"; filename*=UTF-8''model%20space.zip",
            ),
        );

        assert_eq!(
            download_filename(&headers),
            (
                Some("model space.zip".to_owned()),
                Some("content-disposition-filename*".to_owned())
            )
        );
    }

    #[test]
    fn ignores_non_utf8_rfc8187_download_filename() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_static(
                "attachment; filename=\"fallback.zip\"; filename*=ISO-8859-1''model%20space.zip",
            ),
        );

        assert_eq!(
            download_filename(&headers),
            (
                Some("fallback.zip".to_owned()),
                Some("content-disposition".to_owned())
            )
        );
    }

    #[test]
    fn serializes_response_headers_for_diagnostics() {
        let mut headers = HeaderMap::new();
        headers.append("x-test", HeaderValue::from_static("first"));
        headers.append("x-test", HeaderValue::from_static("second"));

        let json = header_map_json(&headers).unwrap();

        assert_eq!(json, r#"{"x-test":["first","second"]}"#);
    }

    #[test]
    fn preserves_quoted_semicolons_in_download_filename() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_static("attachment; filename=\"part;a.step\""),
        );

        assert_eq!(
            download_filename(&headers),
            (
                Some("part;a.step".to_owned()),
                Some("content-disposition".to_owned())
            )
        );
    }
}
