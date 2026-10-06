//! Pinned, one-attempt Onshape JSON operations for immutable selection planning.

use std::{
    fmt,
    sync::{Arc, OnceLock},
    time::Duration,
};

use reqwest::{Method, Url, header};
use serde::{
    Deserialize, Serialize,
    de::{DeserializeSeed, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Number, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio_rustls::{TlsConnector, rustls};

pub const PRODUCTION_ORIGIN: &str = "https://cad.onshape.com";
pub const API_BASELINE_SHA256: &str =
    "eadcea471568f8737f51fffe794dae51b72c35a700d4a7ac11c98fd345ef2eed";
pub const SMALL_RESPONSE_LIMIT: usize = 65_536;
pub const PARTS_RESPONSE_LIMIT: usize = 33_554_432;
pub const ASSEMBLY_RESPONSE_LIMIT: usize = 16_777_216;
pub(crate) const GEOMETRY_DOWNLOAD_LIMIT: usize = 134_217_728;
const MAX_DEPTH: usize = 32;
const MAX_ARRAY_ENTRIES: usize = 4_096;
const MAX_OBJECT_MEMBERS: usize = 256;
const MAX_TOTAL_MEMBERS: usize = 262_144;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailureKind {
    InvalidSelection,
    OperationalApiContractFailure,
    UnavailableSourceState,
    AuthenticationFailure,
    TransportFailure,
    OperationalTimeoutFailure,
    OperationalTranslationFailure,
    OperationalHttpFailure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SafeDiagnostic {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector_position: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiFailure {
    pub kind: FailureKind,
    pub diagnostics: Vec<SafeDiagnostic>,
}

impl ApiFailure {
    /// Never copy upstream text or source values into a diagnostic.
    pub fn new(kind: FailureKind, code: &str) -> Self {
        let code: String = code
            .bytes()
            .filter(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_.-".contains(byte)
            })
            .take(128)
            .map(char::from)
            .collect();
        let message = match kind {
            FailureKind::InvalidSelection => "The selection request is invalid.",
            FailureKind::OperationalApiContractFailure => {
                "The Onshape API contract could not be verified."
            }
            FailureKind::UnavailableSourceState => "The selected source state is unavailable.",
            FailureKind::AuthenticationFailure => "Onshape authentication failed.",
            FailureKind::TransportFailure => "The Onshape operation did not complete.",
            FailureKind::OperationalTimeoutFailure => "The geometry acquisition deadline expired.",
            FailureKind::OperationalTranslationFailure => "The geometry translation failed.",
            FailureKind::OperationalHttpFailure => "The geometry HTTP operation failed.",
        };
        Self {
            kind,
            diagnostics: vec![SafeDiagnostic {
                code: if code.is_empty() {
                    "failure".to_owned()
                } else {
                    code
                },
                message: message.to_owned(),
                operation: None,
                selector_position: None,
            }],
        }
    }

    pub fn operation(mut self, operation: &str) -> Self {
        let operation: String = operation
            .bytes()
            .filter(u8::is_ascii_graphic)
            .take(128)
            .map(char::from)
            .collect();
        for diagnostic in &mut self.diagnostics {
            diagnostic.operation = Some(operation.clone());
        }
        self
    }

    pub fn position(mut self, position: usize) -> Self {
        for diagnostic in &mut self.diagnostics {
            diagnostic.selector_position = Some(position);
        }
        self
    }
}

impl fmt::Display for ApiFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?}", self.kind)?;
        if let Some(diagnostic) = self.diagnostics.first().filter(|diagnostic| {
            !diagnostic.code.is_empty()
                && diagnostic.code.len() <= 128
                && diagnostic.code.bytes().all(|byte| {
                    byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_.-".contains(&byte)
                })
        }) {
            write!(formatter, " ({}", diagnostic.code)?;
            if let Some(operation) = diagnostic.operation.as_deref().filter(|operation| {
                !operation.is_empty()
                    && operation.len() <= 128
                    && operation.bytes().all(|byte| byte.is_ascii_graphic())
            }) {
                write!(formatter, " in {operation}")?;
            }
            write!(formatter, ")")?;
        }
        Ok(())
    }
}

impl std::error::Error for ApiFailure {}

fn operational(code: &str) -> ApiFailure {
    ApiFailure::new(FailureKind::OperationalApiContractFailure, code)
}

pub fn bounded_visible_ascii(value: &str) -> bool {
    !value.is_empty() && value.len() <= 4_096 && value.bytes().all(|byte| byte.is_ascii_graphic())
}

pub fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// The configured base path is deliberately excluded from trusted origin identity.
pub fn canonical_origin(base_url: &str) -> Result<String, ApiFailure> {
    let parsed = Url::parse(base_url).map_err(|_| operational("invalid_api_origin"))?;
    // An empty userinfo component is also forbidden, even if normalized away.
    let has_userinfo = base_url
        .split_once(':')
        .map(|(_, remainder)| {
            remainder
                .trim_start_matches(['/', '\\'])
                .split(['/', '\\', '?', '#'])
                .next()
                .unwrap_or_default()
                .contains('@')
        })
        .unwrap_or(false);
    if parsed.scheme() != "https"
        || parsed.host().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || has_userinfo
    {
        return Err(operational("invalid_api_origin"));
    }
    Ok(parsed.origin().ascii_serialization())
}

#[derive(Clone)]
pub struct OnshapeApi {
    tls: Option<Arc<rustls::ClientConfig>>,
    deadlines: Deadlines,
    origin: String,
    transport_origin: Url,
    access_key: Option<String>,
    secret_key: Option<String>,
}

#[derive(Debug)]
pub(crate) struct GeometryDownload {
    pub bytes: Vec<u8>,
    pub transport_media: String,
}

#[derive(Clone, Copy)]
enum ResponsePolicy {
    SelectionJson,
    GeometryJson { create: bool },
    GeometryBytes,
}

pub(crate) fn geometry_create_path(document: &str, version: &str, element: &str) -> String {
    encoded_path(&[
        "api",
        "v16",
        "partstudios",
        "d",
        document,
        "v",
        version,
        "e",
        element,
        "translations",
    ])
}

pub(crate) fn geometry_create_body(configuration: &str, part: &str) -> Value {
    serde_json::json!({
        "formatName": "3MF", "storeInDocument": false, "notifyUser": false,
        "triggerAutoDownload": false, "configuration": configuration,
        "partIds": part, "grouping": true, "resolution": "fine",
    })
}

impl OnshapeApi {
    pub fn new(
        base_url: &str,
        access_key: Option<String>,
        secret_key: Option<String>,
    ) -> Result<Self, ApiFailure> {
        let origin = canonical_origin(base_url)?;
        if origin != PRODUCTION_ORIGIN {
            return Err(operational("untrusted_api_origin"));
        }
        let transport_origin =
            Url::parse(&origin).map_err(|_| operational("invalid_api_origin"))?;
        Ok(Self {
            tls: Some(tls_config()?),
            deadlines: Deadlines::default(),
            origin,
            transport_origin,
            access_key,
            secret_key,
        })
    }

    pub fn origin(&self) -> &str {
        &self.origin
    }

    pub(crate) async fn create_geometry_translation(
        &self,
        document: &str,
        version: &str,
        element: &str,
        configuration: &str,
        part: &str,
        deadline: tokio::time::Instant,
    ) -> Result<Value, ApiFailure> {
        let body = geometry_create_body(configuration, part);
        let response = self
            .request_with_policy(
                "createPartStudioTranslation",
                Method::POST,
                &[
                    "api",
                    "v16",
                    "partstudios",
                    "d",
                    document,
                    "v",
                    version,
                    "e",
                    element,
                    "translations",
                ],
                &[],
                Some(&body),
                SMALL_RESPONSE_LIMIT,
                ResponsePolicy::GeometryJson { create: true },
                Some(deadline),
            )
            .await?;
        parse_json(&response.bytes, SMALL_RESPONSE_LIMIT)
            .map_err(|failure| failure.operation("createPartStudioTranslation"))
    }

    pub(crate) async fn poll_geometry_translation(
        &self,
        id: &str,
        deadline: tokio::time::Instant,
    ) -> Result<Value, ApiFailure> {
        let response = self
            .request_with_policy(
                "getTranslation",
                Method::GET,
                &["api", "v16", "translations", id],
                &[],
                None,
                SMALL_RESPONSE_LIMIT,
                ResponsePolicy::GeometryJson { create: false },
                Some(deadline),
            )
            .await?;
        parse_json(&response.bytes, SMALL_RESPONSE_LIMIT)
            .map_err(|failure| failure.operation("getTranslation"))
    }

    pub(crate) async fn download_geometry(
        &self,
        document: &str,
        id: &str,
        deadline: tokio::time::Instant,
    ) -> Result<GeometryDownload, ApiFailure> {
        self.request_with_policy(
            "downloadExternalData",
            Method::GET,
            &["api", "v16", "documents", "d", document, "externaldata", id],
            &[],
            None,
            GEOMETRY_DOWNLOAD_LIMIT,
            ResponsePolicy::GeometryBytes,
            Some(deadline),
        )
        .await
    }

    /// This socket override cannot be enabled in production and never changes trust.
    #[cfg(test)]
    pub(crate) fn for_test(server_origin: &str) -> Self {
        let transport_origin = Url::parse(server_origin).unwrap();
        assert_eq!(transport_origin.scheme(), "http");
        let address: std::net::IpAddr = transport_origin.host_str().unwrap().parse().unwrap();
        assert!(address.is_loopback());
        Self {
            tls: None,
            deadlines: Deadlines::default(),
            origin: PRODUCTION_ORIGIN.to_owned(),
            transport_origin,
            access_key: Some("synthetic-access-key".to_owned()),
            secret_key: Some("synthetic-secret-key".to_owned()),
        }
    }

    pub async fn resolve_version(
        &self,
        document_id: &str,
        version_id: &str,
    ) -> Result<String, ApiFailure> {
        let operation = "getVersion";
        let value = self
            .request(
                operation,
                Method::GET,
                &[
                    "api",
                    "v16",
                    "documents",
                    "d",
                    document_id,
                    "versions",
                    version_id,
                ],
                &[("parents", "false")],
                None,
                SMALL_RESPONSE_LIMIT,
            )
            .await?;
        let object = value
            .as_object()
            .ok_or_else(|| operational("invalid_version").operation(operation))?;
        let document =
            required_ascii(object, "documentId").map_err(|failure| failure.operation(operation))?;
        let version =
            required_ascii(object, "id").map_err(|failure| failure.operation(operation))?;
        let microversion = required_ascii(object, "microversion")
            .map_err(|failure| failure.operation(operation))?;
        if document != document_id || version != version_id {
            return Err(operational("version_resource_mismatch").operation(operation));
        }
        Ok(microversion.to_owned())
    }

    pub async fn parts(
        &self,
        document: &str,
        microversion: &str,
        element: &str,
        configuration: &str,
    ) -> Result<Value, ApiFailure> {
        self.request(
            "getPartsWMVE",
            Method::GET,
            &[
                "api",
                "v16",
                "parts",
                "d",
                document,
                "m",
                microversion,
                "e",
                element,
            ],
            &[
                ("configuration", configuration),
                ("withThumbnails", "false"),
                ("includePropertyDefaults", "false"),
                ("includeFlatParts", "false"),
            ],
            None,
            PARTS_RESPONSE_LIMIT,
        )
        .await
    }

    pub async fn assembly(
        &self,
        document: &str,
        microversion: &str,
        element: &str,
        configuration: &str,
    ) -> Result<Value, ApiFailure> {
        self.request(
            "getAssemblyDefinition",
            Method::GET,
            &[
                "api",
                "v16",
                "assemblies",
                "d",
                document,
                "m",
                microversion,
                "e",
                element,
            ],
            &[
                ("configuration", configuration),
                ("includeMateFeatures", "false"),
                ("includeNonSolids", "false"),
                ("includeMateConnectors", "false"),
                ("excludeSuppressed", "false"),
            ],
            None,
            ASSEMBLY_RESPONSE_LIMIT,
        )
        .await
    }

    pub async fn encode(
        &self,
        document: &str,
        version: &str,
        element: &str,
        body: &Value,
    ) -> Result<Value, ApiFailure> {
        self.request(
            "encodeConfigurationMap",
            Method::POST,
            &[
                "api",
                "v16",
                "elements",
                "d",
                document,
                "e",
                element,
                "configurationencodings",
            ],
            &[("versionId", version)],
            Some(body),
            SMALL_RESPONSE_LIMIT,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn request(
        &self,
        operation: &str,
        method: Method,
        segments: &[&str],
        query: &[(&str, &str)],
        body: Option<&Value>,
        limit: usize,
    ) -> Result<Value, ApiFailure> {
        let response = self
            .request_with_policy(
                operation,
                method,
                segments,
                query,
                body,
                limit,
                ResponsePolicy::SelectionJson,
                None,
            )
            .await?;
        parse_json(&response.bytes, limit).map_err(|failure| failure.operation(operation))
    }

    #[allow(clippy::too_many_arguments)]
    async fn request_with_policy(
        &self,
        operation: &str,
        method: Method,
        segments: &[&str],
        query: &[(&str, &str)],
        body: Option<&Value>,
        limit: usize,
        policy: ResponsePolicy,
        deadline: Option<tokio::time::Instant>,
    ) -> Result<GeometryDownload, ApiFailure> {
        let now = tokio::time::Instant::now();
        let operation_deadline = now + self.deadlines.total;
        let clipped = deadline.is_some_and(|deadline| deadline <= operation_deadline);
        let deadline = deadline.map_or(operation_deadline, |deadline| {
            deadline.min(operation_deadline)
        });
        if deadline <= now {
            return Err(ApiFailure::new(
                FailureKind::OperationalTimeoutFailure,
                "acquisition_deadline",
            )
            .operation(operation));
        }
        let (Some(access_key), Some(secret_key)) = (&self.access_key, &self.secret_key) else {
            return Err(
                ApiFailure::new(FailureKind::AuthenticationFailure, "missing_credentials")
                    .operation(operation),
            );
        };
        let mut url = self.transport_origin.clone();
        // URL setters erase dot-only segments; sign and send their exact raw
        // encoded resource path without passing it through URL normalization.
        let path = encoded_path(segments);
        url.set_query(None);
        url.set_fragment(None);
        url.query_pairs_mut().extend_pairs(query.iter().copied());
        let mut headers = crate::onshape::signed_headers(
            method.clone(),
            &path,
            url.query().unwrap_or_default(),
            access_key,
            secret_key,
        )
        .map_err(|_| operational("request_signing_failed").operation(operation))?;
        headers.insert(
            header::ACCEPT,
            header::HeaderValue::from_static(if matches!(policy, ResponsePolicy::GeometryBytes) {
                "application/octet-stream"
            } else {
                "application/json"
            }),
        );
        headers.insert(
            header::ACCEPT_ENCODING,
            header::HeaderValue::from_static("identity"),
        );
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );
        let body = body
            .map(crate::cache_key::canonical_json_bytes)
            .transpose()
            .map_err(|_| operational("invalid_request_body").operation(operation))?;
        let timeout_failure = || {
            ApiFailure::new(
                if clipped {
                    FailureKind::OperationalTimeoutFailure
                } else {
                    FailureKind::TransportFailure
                },
                "operation_deadline",
            )
            .operation(operation)
        };
        if tokio::time::Instant::now() >= deadline {
            return Err(timeout_failure());
        }
        let response = tokio::time::timeout_at(
            deadline,
            self.exchange(method, &url, &path, headers, body.as_deref(), limit, policy),
        )
        .await
        .map_err(|_| timeout_failure())?
        .map_err(|failure| failure.operation(operation))?;
        if tokio::time::Instant::now() >= deadline {
            return Err(timeout_failure());
        }
        Ok(response)
    }
}

#[derive(Clone, Copy)]
struct Deadlines {
    connect: Duration,
    read: Duration,
    total: Duration,
}

impl Default for Deadlines {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(10),
            read: Duration::from_secs(10),
            total: Duration::from_secs(60),
        }
    }
}

fn tls_config() -> Result<Arc<rustls::ClientConfig>, ApiFailure> {
    static CONFIG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    cached_tls_config(&CONFIG, || {
        build_tls_config(rustls_native_certs::load_native_certs().certs)
    })
}

fn cached_tls_config(
    cache: &OnceLock<Arc<rustls::ClientConfig>>,
    build: impl FnOnce() -> Result<Arc<rustls::ClientConfig>, ApiFailure>,
) -> Result<Arc<rustls::ClientConfig>, ApiFailure> {
    if let Some(config) = cache.get() {
        return Ok(Arc::clone(config));
    }
    // Publish only a successful initialization; a transient failure can recover.
    let config = build()?;
    Ok(Arc::clone(cache.get_or_init(|| config)))
}

fn build_tls_config(
    certificates: Vec<rustls::pki_types::CertificateDer<'static>>,
) -> Result<Arc<rustls::ClientConfig>, ApiFailure> {
    let mut roots = rustls::RootCertStore::empty();
    roots.add_parsable_certificates(certificates);
    if roots.is_empty() {
        return Err(operational("missing_native_certificates"));
    }
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|_| operational("tls_configuration_failed"))?
    .with_root_certificates(roots)
    .with_no_client_auth();
    Ok(Arc::new(config))
}

fn encoded_path(segments: &[&str]) -> String {
    let mut path = String::new();
    for segment in segments {
        path.push('/');
        for byte in segment.bytes() {
            if byte.is_ascii_alphanumeric()
                || b"-_~".contains(&byte)
                || (byte == b'.' && !matches!(*segment, "." | ".."))
            {
                path.push(char::from(byte));
            } else {
                const HEX: &[u8; 16] = b"0123456789ABCDEF";
                path.push('%');
                path.push(char::from(HEX[usize::from(byte >> 4)]));
                path.push(char::from(HEX[usize::from(byte & 15)]));
            }
        }
    }
    path
}

pub fn required_ascii<'a>(
    object: &'a Map<String, Value>,
    field: &str,
) -> Result<&'a str, ApiFailure> {
    object
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| bounded_visible_ascii(value))
        .ok_or_else(|| operational("invalid_required_source_value"))
}

trait ApiStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> ApiStream for T {}
type ResponseReader = BufReader<Box<dyn ApiStream>>;
const MAX_HEADER_BYTES: usize = 65_536;

fn transport(code: &str) -> ApiFailure {
    ApiFailure::new(FailureKind::TransportFailure, code)
}

impl OnshapeApi {
    #[allow(clippy::too_many_arguments)]
    async fn exchange(
        &self,
        method: Method,
        url: &Url,
        path: &str,
        mut headers: header::HeaderMap,
        body: Option<&[u8]>,
        limit: usize,
        policy: ResponsePolicy,
    ) -> Result<GeometryDownload, ApiFailure> {
        let host = url
            .host_str()
            .ok_or_else(|| operational("invalid_api_host"))?;
        let port = url
            .port_or_known_default()
            .ok_or_else(|| operational("invalid_api_port"))?;
        let stream: Box<dyn ApiStream> = tokio::time::timeout(self.deadlines.connect, async {
            let tcp = tokio::net::TcpStream::connect((host, port))
                .await
                .map_err(|_| transport("connection_failed"))?;
            if url.scheme() == "https" {
                let config = self
                    .tls
                    .clone()
                    .ok_or_else(|| operational("missing_tls_configuration"))?;
                let name = rustls::pki_types::ServerName::try_from(host.to_owned())
                    .map_err(|_| operational("invalid_tls_server_name"))?;
                let tls = TlsConnector::from(config)
                    .connect(name, tcp)
                    .await
                    .map_err(|_| transport("tls_connection_failed"))?;
                Ok::<Box<dyn ApiStream>, ApiFailure>(Box::new(tls))
            } else {
                #[cfg(test)]
                {
                    Ok::<Box<dyn ApiStream>, ApiFailure>(Box::new(tcp))
                }
                #[cfg(not(test))]
                {
                    Err(operational("non_https_transport"))
                }
            }
        })
        .await
        .map_err(|_| transport("connect_deadline"))??;
        let host_header = match url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_owned(),
        };
        headers.insert(
            header::HOST,
            host_header
                .parse()
                .map_err(|_| operational("invalid_host_header"))?,
        );
        headers.insert(
            header::CONNECTION,
            header::HeaderValue::from_static("close"),
        );
        if let Some(body) = body {
            headers.insert(
                header::CONTENT_LENGTH,
                body.len()
                    .to_string()
                    .parse()
                    .map_err(|_| operational("invalid_request_length"))?,
            );
        }
        let query = url
            .query()
            .filter(|query| !query.is_empty())
            .map(|query| format!("?{query}"))
            .unwrap_or_default();
        let mut request =
            format!("{} {}{} HTTP/1.1\r\n", method.as_str(), path, query).into_bytes();
        for (name, value) in &headers {
            request.extend_from_slice(name.as_str().as_bytes());
            request.extend_from_slice(b": ");
            request.extend_from_slice(value.as_bytes());
            request.extend_from_slice(b"\r\n");
        }
        request.extend_from_slice(b"\r\n");
        let mut reader = BufReader::new(stream);
        reader
            .get_mut()
            .write_all(&request)
            .await
            .map_err(|_| transport("request_write_failed"))?;
        if let Some(body) = body {
            reader
                .get_mut()
                .write_all(body)
                .await
                .map_err(|_| transport("request_write_failed"))?;
        }
        reader
            .get_mut()
            .flush()
            .await
            .map_err(|_| transport("request_write_failed"))?;
        let first_line = read_line(&mut reader, self.deadlines.read, MAX_HEADER_BYTES).await?;
        let status = status_from_line(&first_line)?;
        // Observe the status before inspecting Content-Length or diagnostics.
        // General HTTP clients lose this information when rejecting framing.
        if status != 200
            && !(status == 201 && matches!(policy, ResponsePolicy::GeometryJson { create: true }))
        {
            let kind = if matches!(status, 401 | 403) {
                FailureKind::AuthenticationFailure
            } else if !matches!(policy, ResponsePolicy::SelectionJson) {
                FailureKind::OperationalHttpFailure
            } else {
                FailureKind::OperationalApiContractFailure
            };
            return Err(ApiFailure::new(kind, "http_status"));
        }
        let mut head = first_line;
        loop {
            let line = read_line(
                &mut reader,
                self.deadlines.read,
                MAX_HEADER_BYTES - head.len(),
            )
            .await?;
            let complete = line == b"\r\n";
            head.extend_from_slice(&line);
            if complete {
                break;
            }
        }
        let headers = parse_header_block(&head)?;
        let transport_media = if matches!(policy, ResponsePolicy::GeometryBytes) {
            validate_geometry_headers(&headers)?
        } else {
            validate_json_headers(&headers)?;
            String::new()
        };
        let declared = content_length(&headers, limit)?;
        let mut encodings = headers.get_all(header::TRANSFER_ENCODING).iter();
        let chunked = if let Some(encoding) = encodings.next() {
            if encodings.next().is_some()
                || !encoding.as_bytes().eq_ignore_ascii_case(b"chunked")
                || declared.is_some()
            {
                return Err(operational("invalid_transfer_encoding"));
            }
            true
        } else {
            false
        };
        let bytes = if chunked {
            collect_chunked(&mut reader, self.deadlines.read, limit).await?
        } else if let Some(declared) = declared {
            let mut bytes = Vec::with_capacity((declared as usize).min(8_192));
            read_exact_body(
                &mut reader,
                self.deadlines.read,
                declared as usize,
                &mut bytes,
                limit,
            )
            .await?;
            require_completed_close(&mut reader, self.deadlines.read).await?;
            bytes
        } else {
            collect_to_eof(&mut reader, self.deadlines.read, limit).await?
        };
        if bytes.is_empty() && matches!(policy, ResponsePolicy::GeometryBytes) {
            return Err(operational("empty_geometry_payload"));
        }
        Ok(GeometryDownload {
            bytes,
            transport_media,
        })
    }
}

fn status_from_line(line: &[u8]) -> Result<u16, ApiFailure> {
    let mut parts = line
        .strip_suffix(b"\r\n")
        .ok_or_else(|| transport("invalid_http_status"))?
        .splitn(3, |byte| *byte == b' ');
    let protocol = parts.next().unwrap_or_default();
    let code = parts.next().unwrap_or_default();
    if !matches!(protocol, b"HTTP/1.1" | b"HTTP/1.0")
        || code.len() != 3
        || !code.iter().all(u8::is_ascii_digit)
    {
        return Err(transport("invalid_http_status"));
    }
    let status = u16::from(code[0] - b'0') * 100
        + u16::from(code[1] - b'0') * 10
        + u16::from(code[2] - b'0');
    if status < 100 {
        return Err(transport("invalid_http_status"));
    }
    Ok(status)
}

fn parse_header_block(head: &[u8]) -> Result<header::HeaderMap, ApiFailure> {
    let mut slots = [httparse::EMPTY_HEADER; 256];
    let mut parsed = httparse::Response::new(&mut slots);
    if parsed
        .parse(head)
        .map_err(|_| operational("invalid_http_headers"))?
        != httparse::Status::Complete(head.len())
    {
        return Err(operational("invalid_http_headers"));
    }
    let mut headers = header::HeaderMap::new();
    for raw in parsed.headers {
        let name = header::HeaderName::from_bytes(raw.name.as_bytes())
            .map_err(|_| operational("invalid_http_headers"))?;
        let value = header::HeaderValue::from_bytes(raw.value)
            .map_err(|_| operational("invalid_http_headers"))?;
        headers.append(name, value);
    }
    Ok(headers)
}

async fn read_line(
    reader: &mut ResponseReader,
    idle: Duration,
    limit: usize,
) -> Result<Vec<u8>, ApiFailure> {
    let mut line = Vec::new();
    loop {
        let available = tokio::time::timeout(idle, reader.fill_buf())
            .await
            .map_err(|_| transport("read_idle_deadline"))?
            .map_err(|_| transport("response_read_failed"))?;
        if available.is_empty() {
            return Err(transport("premature_eof"));
        }
        let count = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|index| index + 1)
            .unwrap_or(available.len());
        if count > limit - line.len() {
            return Err(operational("http_header_byte_limit"));
        }
        let complete = available[count - 1] == b'\n';
        line.extend_from_slice(&available[..count]);
        reader.consume(count);
        if complete {
            if !line.ends_with(b"\r\n") {
                return Err(transport("invalid_http_line_framing"));
            }
            return Ok(line);
        }
    }
}

async fn timed_read(
    reader: &mut ResponseReader,
    idle: Duration,
    bytes: &mut [u8],
) -> Result<usize, ApiFailure> {
    tokio::time::timeout(idle, reader.read(bytes))
        .await
        .map_err(|_| transport("read_idle_deadline"))?
        .map_err(|_| transport("response_read_failed"))
}

/// Connection: close makes completion observable across socket fragments. An
/// unclean TLS EOF after an already complete explicit HTTP frame is closure;
/// before frame completion it remains a transport failure.
async fn require_completed_close(
    reader: &mut ResponseReader,
    idle: Duration,
) -> Result<(), ApiFailure> {
    let mut extra = [0; 1];
    match tokio::time::timeout(idle, reader.read(&mut extra))
        .await
        .map_err(|_| transport("read_idle_deadline"))?
    {
        Ok(0) => Ok(()),
        Ok(_) => Err(operational("response_length_mismatch")),
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => Ok(()),
        Err(_) => Err(transport("response_read_failed")),
    }
}

async fn read_exact_body(
    reader: &mut ResponseReader,
    idle: Duration,
    length: usize,
    bytes: &mut Vec<u8>,
    limit: usize,
) -> Result<(), ApiFailure> {
    let mut remaining = length;
    while remaining > 0 {
        let mut buffer = [0; 8_192];
        let maximum = remaining
            .min(buffer.len())
            .min((limit - bytes.len()).saturating_add(1));
        let read = timed_read(reader, idle, &mut buffer[..maximum]).await?;
        if read == 0 {
            return Err(transport("premature_eof"));
        }
        if read > limit - bytes.len() {
            return Err(operational("response_byte_limit"));
        }
        bytes.extend_from_slice(&buffer[..read]);
        remaining -= read;
    }
    Ok(())
}

async fn collect_to_eof(
    reader: &mut ResponseReader,
    idle: Duration,
    limit: usize,
) -> Result<Vec<u8>, ApiFailure> {
    let mut bytes = Vec::new();
    loop {
        let mut buffer = [0; 8_192];
        let maximum = (limit - bytes.len()).saturating_add(1).min(buffer.len());
        let read = timed_read(reader, idle, &mut buffer[..maximum]).await?;
        if read == 0 {
            return Ok(bytes);
        }
        if read > limit - bytes.len() {
            return Err(operational("response_byte_limit"));
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
}

async fn collect_chunked(
    reader: &mut ResponseReader,
    idle: Duration,
    limit: usize,
) -> Result<Vec<u8>, ApiFailure> {
    let mut bytes = Vec::new();
    loop {
        let line = read_line(reader, idle, MAX_HEADER_BYTES).await?;
        let line = &line[..line.len() - 2];
        let size_length = line
            .iter()
            .take_while(|byte| byte.is_ascii_hexdigit())
            .count();
        let size = &line[..size_length];
        if size.is_empty() || !valid_chunk_extensions(&line[size_length..]) {
            return Err(transport("invalid_chunk_framing"));
        }
        let mut length = 0usize;
        for byte in size {
            length = length
                .checked_mul(16)
                .and_then(|value| {
                    value.checked_add(usize::from(match byte {
                        b'0'..=b'9' => byte - b'0',
                        b'a'..=b'f' => byte - b'a' + 10,
                        _ => byte - b'A' + 10,
                    }))
                })
                .ok_or_else(|| transport("invalid_chunk_framing"))?;
        }
        if length == 0 {
            let mut trailers = b"HTTP/1.1 200 OK\r\n".to_vec();
            loop {
                let line = read_line(reader, idle, MAX_HEADER_BYTES - trailers.len()).await?;
                let complete = line == b"\r\n";
                trailers.extend_from_slice(&line);
                if complete {
                    break;
                }
            }
            let trailers =
                parse_header_block(&trailers).map_err(|_| transport("invalid_chunk_trailers"))?;
            if [
                header::CONTENT_LENGTH,
                header::TRANSFER_ENCODING,
                header::CONTENT_TYPE,
                header::CONTENT_ENCODING,
            ]
            .iter()
            .any(|name| trailers.contains_key(name))
            {
                return Err(operational("invalid_response_trailers"));
            }
            require_completed_close(reader, idle).await?;
            return Ok(bytes);
        }
        read_exact_body(reader, idle, length, &mut bytes, limit).await?;
        let mut ending = Vec::new();
        read_exact_body(reader, idle, 2, &mut ending, 2).await?;
        if ending != b"\r\n" {
            return Err(transport("invalid_chunk_framing"));
        }
    }
}

fn valid_chunk_extensions(mut bytes: &[u8]) -> bool {
    loop {
        trim_ows(&mut bytes);
        if bytes.is_empty() {
            return true;
        }
        if !consume(&mut bytes, b';') {
            return false;
        }
        trim_ows(&mut bytes);
        if take_token(&mut bytes).is_none() {
            return false;
        }
        trim_ows(&mut bytes);
        if consume(&mut bytes, b'=') {
            trim_ows(&mut bytes);
            if consume(&mut bytes, b'"') {
                let mut ended = false;
                while let Some((&byte, rest)) = bytes.split_first() {
                    bytes = rest;
                    match byte {
                        b'"' => {
                            ended = true;
                            break;
                        }
                        b'\\' => {
                            let Some((&escaped, rest)) = bytes.split_first() else {
                                return false;
                            };
                            if !(escaped == b'\t'
                                || (0x20..=0x7e).contains(&escaped)
                                || escaped >= 0x80)
                            {
                                return false;
                            }
                            bytes = rest;
                        }
                        b'\t' | 0x20..=0x7e | 0x80..=0xff => {}
                        _ => return false,
                    }
                }
                if !ended {
                    return false;
                }
            } else if take_token(&mut bytes).is_none() {
                return false;
            }
        }
    }
}

fn content_length(headers: &header::HeaderMap, limit: usize) -> Result<Option<u64>, ApiFailure> {
    let mut values = headers.get_all(header::CONTENT_LENGTH).iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(operational("invalid_content_length"));
    }
    let value = value
        .to_str()
        .map_err(|_| operational("invalid_content_length"))?;
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(operational("invalid_content_length"));
    }
    let value: u64 = value
        .parse()
        .map_err(|_| operational("invalid_content_length"))?;
    if value > limit as u64 {
        return Err(operational("response_byte_limit"));
    }
    Ok(Some(value))
}

fn validate_json_headers(headers: &header::HeaderMap) -> Result<(), ApiFailure> {
    let mut encodings = headers.get_all(header::CONTENT_ENCODING).iter();
    if let Some(encoding) = encodings.next()
        && (encodings.next().is_some() || encoding.as_bytes() != b"identity")
    {
        return Err(operational("unsupported_content_encoding"));
    }
    let mut types = headers.get_all(header::CONTENT_TYPE).iter();
    let value = types
        .next()
        .ok_or_else(|| operational("invalid_content_type"))?;
    if types.next().is_some() {
        return Err(operational("invalid_content_type"));
    }
    if !valid_json_media_type(value.as_bytes()) {
        return Err(operational("invalid_content_type"));
    }
    Ok(())
}

fn validate_geometry_headers(headers: &header::HeaderMap) -> Result<String, ApiFailure> {
    let mut encodings = headers.get_all(header::CONTENT_ENCODING).iter();
    if let Some(encoding) = encodings.next()
        && (encodings.next().is_some() || !encoding.as_bytes().eq_ignore_ascii_case(b"identity"))
    {
        return Err(operational("unsupported_content_encoding"));
    }
    let mut types = headers.get_all(header::CONTENT_TYPE).iter();
    let value = types
        .next()
        .ok_or_else(|| operational("invalid_content_type"))?;
    if types.next().is_some() || !valid_geometry_media_type(value.as_bytes()) {
        return Err(operational("invalid_content_type"));
    }
    value
        .to_str()
        .map(str::to_owned)
        .map_err(|_| operational("invalid_content_type"))
}

fn valid_geometry_media_type(mut bytes: &[u8]) -> bool {
    trim_ows(&mut bytes);
    let Some(base) = take_token(&mut bytes) else {
        return false;
    };
    if !base.eq_ignore_ascii_case(b"application") || !consume(&mut bytes, b'/') {
        return false;
    }
    let Some(subtype) = take_token(&mut bytes) else {
        return false;
    };
    if !subtype.eq_ignore_ascii_case(b"octet-stream") {
        return false;
    }
    trim_ows(&mut bytes);
    if bytes.is_empty() {
        return true;
    }
    if !consume(&mut bytes, b';') {
        return false;
    }
    trim_ows(&mut bytes);
    let Some(parameter) = take_token(&mut bytes) else {
        return false;
    };
    if !parameter.eq_ignore_ascii_case(b"charset") {
        return false;
    }
    trim_ows(&mut bytes);
    if !consume(&mut bytes, b'=') {
        return false;
    }
    trim_ows(&mut bytes);
    let quoted = consume(&mut bytes, b'"');
    let Some(charset) = take_token(&mut bytes) else {
        return false;
    };
    if !charset.eq_ignore_ascii_case(b"utf-8") || (quoted && !consume(&mut bytes, b'"')) {
        return false;
    }
    trim_ows(&mut bytes);
    bytes.is_empty()
}

// Parse MIME tokens and quoted parameters rather than matching a prefix. Unknown
// valid parameters are allowed; every supplied charset must be UTF-8.
fn valid_json_media_type(mut bytes: &[u8]) -> bool {
    trim_ows(&mut bytes);
    let Some(base) = take_token(&mut bytes) else {
        return false;
    };
    if !base.eq_ignore_ascii_case(b"application") || !consume(&mut bytes, b'/') {
        return false;
    }
    let Some(subtype) = take_token(&mut bytes) else {
        return false;
    };
    if !subtype.eq_ignore_ascii_case(b"json") {
        return false;
    }
    loop {
        trim_ows(&mut bytes);
        if bytes.is_empty() {
            return true;
        }
        if !consume(&mut bytes, b';') {
            return false;
        }
        trim_ows(&mut bytes);
        let Some(name) = take_token(&mut bytes) else {
            return false;
        };
        trim_ows(&mut bytes);
        if !consume(&mut bytes, b'=') {
            return false;
        }
        trim_ows(&mut bytes);
        let parameter = if consume(&mut bytes, b'"') {
            let mut value = Vec::new();
            let mut ended = false;
            while let Some((&byte, rest)) = bytes.split_first() {
                bytes = rest;
                match byte {
                    b'"' => {
                        ended = true;
                        break;
                    }
                    b'\\' => {
                        let Some((&escaped, rest)) = bytes.split_first() else {
                            return false;
                        };
                        if !(escaped == b'\t'
                            || (0x20..=0x7e).contains(&escaped)
                            || escaped >= 0x80)
                        {
                            return false;
                        }
                        bytes = rest;
                        value.push(escaped);
                    }
                    b'\t' | 0x20..=0x7e | 0x80..=0xff => value.push(byte),
                    _ => return false,
                }
            }
            if !ended {
                return false;
            }
            value
        } else {
            let Some(token) = take_token(&mut bytes) else {
                return false;
            };
            token.to_vec()
        };
        if name.eq_ignore_ascii_case(b"charset") && !parameter.eq_ignore_ascii_case(b"utf-8") {
            return false;
        }
    }
}

fn trim_ows(bytes: &mut &[u8]) {
    while matches!(bytes.first(), Some(b' ' | b'\t')) {
        *bytes = &bytes[1..];
    }
}

fn consume(bytes: &mut &[u8], expected: u8) -> bool {
    if bytes.first() != Some(&expected) {
        return false;
    }
    *bytes = &bytes[1..];
    true
}

fn take_token<'a>(bytes: &mut &'a [u8]) -> Option<&'a [u8]> {
    let length = bytes
        .iter()
        .take_while(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(byte))
        .count();
    if length == 0 {
        return None;
    }
    let token = &bytes[..length];
    *bytes = &bytes[length..];
    Some(token)
}

pub fn parse_json(bytes: &[u8], limit: usize) -> Result<Value, ApiFailure> {
    if bytes.len() > limit {
        return Err(operational("response_byte_limit"));
    }
    std::str::from_utf8(bytes).map_err(|_| operational("invalid_json_utf8"))?;
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let mut budget = JsonBudget { members: 0 };
    let value = BoundedValue {
        budget: &mut budget,
        depth: 0,
    }
    .deserialize(&mut deserializer)
    .map_err(|_| operational("invalid_bounded_json"))?;
    deserializer
        .end()
        .map_err(|_| operational("invalid_bounded_json"))?;
    Ok(value)
}

struct JsonBudget {
    members: usize,
}
struct BoundedValue<'a> {
    budget: &'a mut JsonBudget,
    depth: usize,
}

impl<'de> DeserializeSeed<'de> for BoundedValue<'_> {
    type Value = Value;
    fn deserialize<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for BoundedValue<'_> {
    type Value = Value;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("bounded JSON without duplicate members")
    }
    fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }
    fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }
    fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }
    fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Value, E> {
        Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom("non-finite number"))
    }
    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Value, E> {
        Ok(Value::String(value.to_owned()))
    }
    fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Value, E> {
        Ok(Value::String(value))
    }
    fn visit_unit<E: serde::de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_none<E: serde::de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Value, A::Error> {
        if self.depth >= MAX_DEPTH {
            return Err(serde::de::Error::custom("container depth limit"));
        }
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(BoundedValue {
            budget: self.budget,
            depth: self.depth + 1,
        })? {
            if values.len() == MAX_ARRAY_ENTRIES {
                return Err(serde::de::Error::custom("array entry limit"));
            }
            values.push(value);
        }
        Ok(Value::Array(values))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut object: A) -> Result<Value, A::Error> {
        if self.depth >= MAX_DEPTH {
            return Err(serde::de::Error::custom("container depth limit"));
        }
        let mut values = Map::new();
        while let Some(key) = object.next_key::<String>()? {
            if values.len() == MAX_OBJECT_MEMBERS || self.budget.members == MAX_TOTAL_MEMBERS {
                return Err(serde::de::Error::custom("object member limit"));
            }
            if values.contains_key(&key) {
                return Err(serde::de::Error::custom("duplicate object member"));
            }
            self.budget.members += 1;
            let value = object.next_value_seed(BoundedValue {
                budget: self.budget,
                depth: self.depth + 1,
            })?;
            values.insert(key, value);
        }
        Ok(Value::Object(values))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc,
        thread,
    };

    #[test]
    fn origins_are_canonical_and_production_trust_is_static() {
        for input in [
            "https://CAD.Onshape.com:443/base/path",
            "https://cad.onshape.com",
        ] {
            assert_eq!(canonical_origin(input).unwrap(), PRODUCTION_ORIGIN);
            OnshapeApi::new(input, None, None).unwrap();
        }
        assert_eq!(
            canonical_origin("https://EXAMPLE.test:8443/base").unwrap(),
            "https://example.test:8443"
        );
        assert_eq!(
            canonical_origin("https://[::1]:443/path").unwrap(),
            "https://[::1]"
        );
        for input in [
            "http://cad.onshape.com",
            "https://user@cad.onshape.com",
            "https://@cad.onshape.com",
            "https:///@cad.onshape.com",
            "https:\\\\@cad.onshape.com",
            "https://cad.onshape.com?",
            "https://cad.onshape.com#",
            "/relative",
            "https://",
        ] {
            assert!(canonical_origin(input).is_err(), "{input}");
        }
        assert_eq!(
            canonical_origin("https://cad.onshape.com/path/@legitimate").unwrap(),
            PRODUCTION_ORIGIN
        );
        assert!(OnshapeApi::new("https://untrusted.test", None, None).is_err());
    }

    #[test]
    fn parser_rejects_invalid_unknown_structure_and_duplicate_members() {
        for invalid in [
            b"{\"unknown\":{\"a\":1,\"a\":2}}".as_slice(),
            b"{\"a\":1,\"\\u0061\":2}",
            b"[] true",
            b"{\"a\":1e400}",
            b"\xff",
            b"\"\\ud800\"",
        ] {
            assert!(parse_json(invalid, 1_024).is_err());
        }
        assert_eq!(
            parse_json(br#"{"unknown":[null,true,3.5]}"#, 1_024).unwrap()["unknown"][2],
            3.5
        );
        assert!(parse_json(b"[]", 1).is_err());
        assert!(parse_json(b"[]", 2).is_ok());
    }

    #[test]
    fn parser_enforces_inclusive_container_and_member_bounds() {
        for (depth, accepted) in [(32, true), (33, false)] {
            let document = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
            assert_eq!(
                parse_json(document.as_bytes(), usize::MAX).is_ok(),
                accepted
            );
        }
        for (entries, accepted) in [(4_096, true), (4_097, false)] {
            let document = serde_json::to_vec(&vec![0; entries]).unwrap();
            assert_eq!(parse_json(&document, usize::MAX).is_ok(), accepted);
        }
        for (members, accepted) in [(256, true), (257, false)] {
            let object: Map<_, _> = (0..members)
                .map(|index| (format!("k{index}"), Value::Null))
                .collect();
            let document = serde_json::to_vec(&object).unwrap();
            assert_eq!(parse_json(&document, usize::MAX).is_ok(), accepted);
        }
        let object: Map<_, _> = (0..256)
            .map(|index| (format!("k{index}"), Value::Null))
            .collect();
        let mut values = vec![Value::Object(object); 1_024];
        let exact = serde_json::to_vec(&values).unwrap();
        assert!(parse_json(&exact, usize::MAX).is_ok());
        values.push(serde_json::json!({"one":0}));
        assert!(parse_json(&serde_json::to_vec(&values).unwrap(), usize::MAX).is_err());
    }

    #[test]
    fn media_types_validate_parameters_and_utf8_charset() {
        for value in [
            "application/json",
            "Application/JSON; charset=UTF-8",
            "application/json; charset=\"utf-8\"; profile=\"x;y\"",
            "application/json; profile=a",
            "application/json; profile=\"é\"",
        ] {
            assert!(valid_json_media_type(value.as_bytes()), "{value}");
        }
        for value in [
            "application/jsonx",
            "text/json",
            "application/json; charset=latin-1",
            "application/json; charset=utf-8; charset=ascii",
            "application/json; garbage",
            "application/json; a=\"unterminated",
            "application/json; a=b c",
            "application/json, text/json",
            "application/json; charset=\"é\"",
        ] {
            assert!(!valid_json_media_type(value.as_bytes()), "{value}");
        }
    }

    #[test]
    fn content_length_headers_are_strict_and_bounded() {
        let mut headers = header::HeaderMap::new();
        assert_eq!(content_length(&headers, 10).unwrap(), None);
        for value in ["0", "10", "00010"] {
            headers.insert(header::CONTENT_LENGTH, value.parse().unwrap());
            assert!(content_length(&headers, 10).is_ok());
        }
        for value in ["", "+1", "-1", "1,1", "1x", "11", "18446744073709551616"] {
            headers.insert(header::CONTENT_LENGTH, value.parse().unwrap());
            assert!(content_length(&headers, 10).is_err(), "{value}");
        }
        headers.insert(header::CONTENT_LENGTH, "1".parse().unwrap());
        headers.append(header::CONTENT_LENGTH, "1".parse().unwrap());
        assert!(content_length(&headers, 10).is_err());
    }

    fn synthetic_root() -> rustls::pki_types::CertificateDer<'static> {
        // Locally generated, self-signed public CA; its private key was discarded.
        rustls::pki_types::CertificateDer::from(
            include_bytes!("test_data/onshape-test-root.der").as_slice(),
        )
    }

    #[test]
    fn tls_native_roots_keep_valid_certificates_and_reject_empty_stores() {
        let invalid = rustls::pki_types::CertificateDer::from(b"invalid DER".as_slice());
        assert!(build_tls_config(vec![invalid.clone(), synthetic_root()]).is_ok());
        for certificates in [Vec::new(), vec![invalid]] {
            let failure = build_tls_config(certificates).unwrap_err();
            assert_eq!(failure.kind, FailureKind::OperationalApiContractFailure);
            assert_eq!(failure.diagnostics[0].code, "missing_native_certificates");
        }
    }

    #[test]
    fn tls_cache_reuses_successes_and_retries_failed_initialization() {
        let cache = OnceLock::new();
        let first = cached_tls_config(&cache, || build_tls_config(Vec::new()));
        assert!(first.is_err());
        assert!(cache.get().is_none());
        let second =
            cached_tls_config(&cache, || build_tls_config(vec![synthetic_root()])).unwrap();
        let third =
            cached_tls_config(&cache, || panic!("successful TLS config must be reused")).unwrap();
        assert!(Arc::ptr_eq(&second, &third));
    }

    #[test]
    fn display_preserves_safe_diagnostics_through_anyhow() {
        let mut failure = operational("encoding_handoff_binding_mismatch");
        assert_eq!(
            failure.to_string(),
            "OperationalApiContractFailure (encoding_handoff_binding_mismatch)"
        );
        failure = failure.operation("encodingValidation");
        failure.diagnostics[0].message = "synthetic upstream response".to_owned();
        failure.diagnostics.push(SafeDiagnostic {
            code: "later_diagnostic".to_owned(),
            message: "another response".to_owned(),
            operation: None,
            selector_position: None,
        });
        let expected = "OperationalApiContractFailure (encoding_handoff_binding_mismatch in encodingValidation)";
        assert_eq!(failure.to_string(), expected);
        assert_eq!(anyhow::Error::new(failure.clone()).to_string(), expected);
        failure.diagnostics.clear();
        assert_eq!(failure.to_string(), "OperationalApiContractFailure");
    }

    #[test]
    fn display_omits_diagnostic_values_outside_sanitized_bounds() {
        for code in ["".to_owned(), "bad\ncode".to_owned(), "x".repeat(129)] {
            let mut failure = transport("tls_connection_failed").operation("getVersion");
            failure.diagnostics[0].code = code;
            assert_eq!(failure.to_string(), "TransportFailure");
        }
        for operation in ["".to_owned(), "bad\noperation".to_owned(), "x".repeat(129)] {
            let mut failure = transport("tls_connection_failed");
            failure.diagnostics[0].operation = Some(operation);
            assert_eq!(
                failure.to_string(),
                "TransportFailure (tls_connection_failed)"
            );
        }
        let failure = transport(&"x".repeat(128)).operation(&"y".repeat(128));
        assert_eq!(
            failure.to_string(),
            format!(
                "TransportFailure ({} in {})",
                "x".repeat(128),
                "y".repeat(128)
            )
        );
    }

    #[test]
    fn diagnostics_never_contain_upstream_text() {
        let failure = operational(&"A".repeat(1_000))
            .operation("getPartsWMVE")
            .position(255);
        assert_eq!(failure.diagnostics.len(), 1);
        assert_eq!(failure.diagnostics[0].code, "failure");
        assert!(failure.diagnostics[0].message.len() < 2_048);
        assert_eq!(failure.diagnostics[0].selector_position, Some(255));
        assert!(bounded_visible_ascii(&"a".repeat(4_096)));
        assert!(!bounded_visible_ascii(&"a".repeat(4_097)));
        assert!(!bounded_visible_ascii(" "));
        assert!(is_sha256(&"a".repeat(64)));
        assert!(!is_sha256(&"A".repeat(64)));
    }

    fn fixture(
        response: Vec<u8>,
    ) -> (
        OnshapeApi,
        mpsc::Receiver<String>,
        thread::JoinHandle<usize>,
    ) {
        fixture_with_delay(response, Duration::ZERO)
    }

    fn fixture_with_delay(
        response: Vec<u8>,
        delay: Duration,
    ) -> (
        OnshapeApi,
        mpsc::Receiver<String>,
        thread::JoinHandle<usize>,
    ) {
        fixture_fragments(response, delay, Vec::new())
    }

    fn fixture_fragments(
        response: Vec<u8>,
        delay: Duration,
        tail: Vec<u8>,
    ) -> (
        OnshapeApi,
        mpsc::Receiver<String>,
        thread::JoinHandle<usize>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (sender, receiver) = mpsc::channel();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            loop {
                let mut chunk = [0; 8_192];
                let read = stream.read(&mut chunk).unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..read]);
                if let Some(end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|value| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            sender.send(String::from_utf8(request).unwrap()).unwrap();
            thread::sleep(delay);
            let _ = stream.write_all(&response);
            if !tail.is_empty() {
                stream.flush().unwrap();
                thread::sleep(Duration::from_millis(30));
                let _ = stream.write_all(&tail);
            }
            drop(stream);
            listener.set_nonblocking(true).unwrap();
            let started = std::time::Instant::now();
            let mut count = 1;
            while started.elapsed() < Duration::from_millis(100) {
                if let Ok((mut stream, _)) = listener.accept() {
                    count += 1;
                    let _ = stream.write_all(&response);
                }
                thread::sleep(Duration::from_millis(5));
            }
            count
        });
        (
            OnshapeApi::for_test(&format!("http://{address}")),
            receiver,
            handle,
        )
    }

    fn json_response(body: &str) -> Vec<u8> {
        format!("HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).into_bytes()
    }

    #[tokio::test]
    async fn exact_operations_encode_path_and_configuration_once() {
        let (api, request, server) = fixture(json_response(
            r#"{"documentId":"d","id":"v","microversion":"m"}"#,
        ));
        assert_eq!(api.resolve_version("d", "v").await.unwrap(), "m");
        assert!(
            request
                .recv()
                .unwrap()
                .starts_with("GET /api/v16/documents/d/d/versions/v?parents=false HTTP/1.1\r\n")
        );
        assert_eq!(server.join().unwrap(), 1);

        let (api, request, server) = fixture(json_response("[]"));
        api.parts("d/one", "m", "e", "configuration=a%2Fb&x=1+2")
            .await
            .unwrap();
        let request = request.recv().unwrap();
        assert!(request.starts_with("GET /api/v16/parts/d/d%2Fone/m/m/e/e?configuration=configuration%3Da%252Fb%26x%3D1%2B2&withThumbnails=false&includePropertyDefaults=false&includeFlatParts=false HTTP/1.1\r\n"), "{request}");
        assert!(
            request
                .to_ascii_lowercase()
                .contains("accept-encoding: identity\r\n")
        );
        assert!(!request.contains("linkDocumentId"));
        assert_eq!(server.join().unwrap(), 1);

        let (api, request, server) = fixture(json_response("{}"));
        api.assembly("d", "m", "e", "c").await.unwrap();
        assert!(request.recv().unwrap().starts_with("GET /api/v16/assemblies/d/d/m/m/e/e?configuration=c&includeMateFeatures=false&includeNonSolids=false&includeMateConnectors=false&excludeSuppressed=false HTTP/1.1\r\n"));
        assert_eq!(server.join().unwrap(), 1);

        let (api, request, server) = fixture(json_response(r#"{"encodedId":"c"}"#));
        api.encode("d", "v", "e", &serde_json::json!({"parameters":[]}))
            .await
            .unwrap();
        let request = request.recv().unwrap();
        assert!(request.starts_with(
            "POST /api/v16/elements/d/d/e/e/configurationencodings?versionId=v HTTP/1.1\r\n"
        ));
        assert!(request.ends_with(r#"{"parameters":[]}"#));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("content-type: application/json\r\n")
        );
        assert_eq!(server.join().unwrap(), 1);
    }

    #[tokio::test]
    async fn failures_do_not_retry_redirect_or_expose_diagnostic_bodies() {
        for (response, expected) in [
            (b"HTTP/1.1 401 Unauthorized\r\nContent-Type: application/octet-stream\r\nContent-Length: 900000\r\nConnection: close\r\n\r\n\xffsecret".to_vec(), FailureKind::AuthenticationFailure),
            (b"HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\ninvalid diagnostic".to_vec(), FailureKind::AuthenticationFailure),
            (b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/redirect\r\nConnection: close\r\n\r\n".to_vec(), FailureKind::OperationalApiContractFailure),
            (b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 0\r\nConnection: close\r\n\r\n".to_vec(), FailureKind::OperationalApiContractFailure),
            (b"HTTP/1.1 503 Unavailable\r\nConnection: close\r\n\r\n".to_vec(), FailureKind::OperationalApiContractFailure),
            (json_response(r#"{"documentId":"d","id":"v","microversionId":"m"}"#), FailureKind::OperationalApiContractFailure),
            (json_response(r#"{"documentId":"other","id":"v","microversion":"m"}"#), FailureKind::OperationalApiContractFailure),
        ] {
            let (api, request, server) = fixture(response);
            let failure = api.resolve_version("d", "v").await.unwrap_err();
            assert_eq!(failure.kind, expected);
            assert_eq!(failure.diagnostics[0].operation.as_deref(), Some("getVersion"));
            assert!(!serde_json::to_string(&failure).unwrap().contains("secret"));
            request.recv().unwrap();
            assert_eq!(server.join().unwrap(), 1);
        }
    }

    #[tokio::test]
    async fn success_streams_are_bounded_and_truncation_remains_transport_failure() {
        for (response, expected) in [
            (b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n[]".to_vec(), FailureKind::OperationalApiContractFailure),
            (b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Encoding: gzip\r\nConnection: close\r\n\r\n[]".to_vec(), FailureKind::OperationalApiContractFailure),
            (b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 33554433\r\nConnection: close\r\n\r\n[]".to_vec(), FailureKind::OperationalApiContractFailure),
            (b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 10\r\nConnection: close\r\n\r\n[]".to_vec(), FailureKind::TransportFailure),
            (json_response("[1,1] trailing"), FailureKind::OperationalApiContractFailure),
        ] {
            let (api, request, server) = fixture(response);
            assert_eq!(api.parts("d", "m", "e", "c").await.unwrap_err().kind, expected);
            request.recv().unwrap();
            assert_eq!(server.join().unwrap(), 1);
        }
        let (api, request, server) = fixture(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n[]"
                .to_vec(),
        );
        assert_eq!(
            api.parts("d", "m", "e", "c").await.unwrap(),
            serde_json::json!([])
        );
        request.recv().unwrap();
        assert_eq!(server.join().unwrap(), 1);
    }

    #[tokio::test]
    async fn actual_byte_bound_applies_without_declared_length_and_to_chunked_transfer() {
        for response in [
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n[] ".to_vec(),
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n2\r\n[]\r\n1\r\n \r\n0\r\n\r\n".to_vec(),
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 3\r\nConnection: close\r\n\r\n[] ".to_vec(),
        ] {
            let (api, request, server) = fixture(response);
            let failure = api.request("test", Method::GET, &["api", "v16"], &[], None, 2).await.unwrap_err();
            assert_eq!(failure.kind, FailureKind::OperationalApiContractFailure);
            request.recv().unwrap();
            assert_eq!(server.join().unwrap(), 1);
        }
        let (api, request, server) = fixture(json_response("[]"));
        assert_eq!(
            api.request("test", Method::GET, &["api", "v16"], &[], None, 2)
                .await
                .unwrap(),
            serde_json::json!([])
        );
        request.recv().unwrap();
        assert_eq!(server.join().unwrap(), 1);
    }

    #[tokio::test]
    async fn read_idle_and_total_timeout_each_stop_after_one_attempt() {
        for (read, total) in [(30, 1_000), (1_000, 30)] {
            let (mut api, request, server) =
                fixture_with_delay(json_response("[]"), Duration::from_millis(100));
            api.deadlines = Deadlines {
                connect: Duration::from_secs(10),
                read: Duration::from_millis(read),
                total: Duration::from_millis(total),
            };
            assert_eq!(
                api.parts("d", "m", "e", "c").await.unwrap_err().kind,
                FailureKind::TransportFailure
            );
            request.recv().unwrap();
            assert_eq!(server.join().unwrap(), 1);
        }
    }

    #[tokio::test]
    async fn missing_credentials_never_contact_transport() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut api = OnshapeApi::for_test(&format!("http://{}", listener.local_addr().unwrap()));
        api.access_key = None;
        assert_eq!(
            api.resolve_version("d", "v").await.unwrap_err().kind,
            FailureKind::AuthenticationFailure
        );
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    #[tokio::test]
    async fn dot_segments_are_exact_encoded_resources_without_path_normalization() {
        let (api, request, server) = fixture(json_response(
            r#"{"documentId":".","id":"..","microversion":"m"}"#,
        ));
        assert_eq!(api.resolve_version(".", "..").await.unwrap(), "m");
        assert!(request.recv().unwrap().starts_with(
            "GET /api/v16/documents/d/%2E/versions/%2E%2E?parents=false HTTP/1.1\r\n"
        ));
        assert_eq!(server.join().unwrap(), 1);
    }

    #[tokio::test]
    async fn malformed_length_preserves_status_and_never_retries() {
        for status in ["200 OK", "401 Unauthorized", "403 Forbidden"] {
            for declaration in [
                "Content-Length: +1",
                "Content-Length: 1,1",
                "Content-Length: 18446744073709551616",
                "Content-Length: 2\r\nContent-Length: 2",
                "Content-Length: 2\r\nContent-Length: 3",
            ] {
                let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{declaration}\r\nConnection: close\r\n\r\n[]").into_bytes();
                let (api, request, server) = fixture(response);
                let expected = if status.starts_with("200") {
                    FailureKind::OperationalApiContractFailure
                } else {
                    FailureKind::AuthenticationFailure
                };
                assert_eq!(
                    api.parts("d", "m", "e", "c").await.unwrap_err().kind,
                    expected,
                    "{status} {declaration}"
                );
                request.recv().unwrap();
                assert_eq!(server.join().unwrap(), 1);
            }
        }
    }

    #[tokio::test]
    async fn split_excess_after_declared_or_chunked_body_is_not_accepted() {
        for response in [json_response("[]"), b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n2\r\n[]\r\n0\r\n\r\n".to_vec()] {
            let (api, request, server) = fixture_fragments(response, Duration::ZERO, b"excess".to_vec());
            assert_eq!(api.parts("d", "m", "e", "c").await.unwrap_err().kind, FailureKind::OperationalApiContractFailure);
            request.recv().unwrap();
            assert_eq!(server.join().unwrap(), 1);
        }
    }

    #[tokio::test]
    async fn huge_truncated_chunk_is_transport_and_significant_trailers_fail() {
        let prefix = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n";
        let (api, request, server) = fixture(format!("{prefix}ffffffff\r\nx").into_bytes());
        assert_eq!(
            api.request("test", Method::GET, &["api", "v16"], &[], None, 2)
                .await
                .unwrap_err()
                .kind,
            FailureKind::TransportFailure
        );
        request.recv().unwrap();
        assert_eq!(server.join().unwrap(), 1);
        for trailer in [
            "Content-Length: +1",
            "Content-Length: 2\r\nContent-Length: 3",
            "Content-Type: text/plain",
            "Content-Encoding: gzip",
            "Transfer-Encoding: chunked",
        ] {
            let (api, request, server) =
                fixture(format!("{prefix}2\r\n[]\r\n0\r\n{trailer}\r\n\r\n").into_bytes());
            assert_eq!(
                api.parts("d", "m", "e", "c").await.unwrap_err().kind,
                FailureKind::OperationalApiContractFailure
            );
            request.recv().unwrap();
            assert_eq!(server.join().unwrap(), 1);
        }
    }

    #[tokio::test]
    async fn chunked_framing_extensions_trailers_and_length_contradiction() {
        for (wire_body, accepted) in [
            (
                "2;foo=\"a;b\"\r\n[]\r\n0\r\nX-Synthetic: value\r\n\r\n",
                true,
            ),
            ("2;=bad\r\n[]\r\n0\r\n\r\n", false),
            ("2\r\n[\r\n", false),
            ("2\r\n[]xx0\r\n\r\n", false),
        ] {
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{wire_body}").into_bytes();
            let (api, request, server) = fixture(response);
            assert_eq!(api.parts("d", "m", "e", "c").await.is_ok(), accepted);
            request.recv().unwrap();
            assert_eq!(server.join().unwrap(), 1);
        }
        let (api, request, server) = fixture(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[] ".to_vec());
        assert_eq!(
            api.parts("d", "m", "e", "c").await.unwrap_err().kind,
            FailureKind::OperationalApiContractFailure
        );
        request.recv().unwrap();
        assert_eq!(server.join().unwrap(), 1);
    }

    #[test]
    fn geometry_media_contract_is_closed_and_case_insensitive() {
        for media in [
            "application/octet-stream",
            "Application/Octet-Stream; Charset=UTF-8",
            " application/octet-stream \t; charset = \"utf-8\" \t",
        ] {
            assert!(valid_geometry_media_type(media.as_bytes()), "{media}");
        }
        for media in [
            "application/json",
            "application/octet-stream-extra",
            "application/octet-stream;charset=ascii",
            "application/octet-stream;foo=bar",
            "application/octet-stream;charset=utf-8;charset=utf-8",
            "application/octet-stream;",
            "application/octet-stream;charset=\"utf-8",
            "application/octet-stream;charset=utf-8;foo=bar",
        ] {
            assert!(!valid_geometry_media_type(media.as_bytes()), "{media}");
        }
    }

    #[tokio::test]
    async fn geometry_create_request_is_pinned_and_preserves_configuration_string() {
        let configuration = "configuration=a%2Fb&text=\"synthetic\"\\value";
        for status in ["200 OK", "201 Created"] {
            let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}").into_bytes();
            let (api, request, server) = fixture(response);
            assert_eq!(
                api.create_geometry_translation(
                    "d/one",
                    ".",
                    "e?one",
                    configuration,
                    "part/one",
                    tokio::time::Instant::now() + Duration::from_secs(1)
                )
                .await
                .unwrap(),
                serde_json::json!({})
            );
            let request = request.recv().unwrap();
            let path = geometry_create_path("d/one", ".", "e?one");
            assert_eq!(
                path,
                "/api/v16/partstudios/d/d%2Fone/v/%2E/e/e%3Fone/translations"
            );
            assert!(request.starts_with(&format!("POST {path} HTTP/1.1\r\n")));
            assert!(request.contains("accept: application/json\r\n"));
            assert!(request.contains("content-type: application/json\r\n"));
            assert!(request.contains("accept-encoding: identity\r\n"));
            let body: Value =
                serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert_eq!(body, geometry_create_body(configuration, "part/one"));
            assert_eq!(body.as_object().unwrap().len(), 8);
            assert_eq!(body["configuration"], configuration);
            assert_eq!(body["partIds"], "part/one");
            assert_eq!(server.join().unwrap(), 1);
        }
    }

    #[tokio::test]
    async fn geometry_poll_path_and_status_failures_preserve_authentication() {
        let (api, request, server) = fixture(json_response("{}"));
        api.poll_geometry_translation(
            "../one",
            tokio::time::Instant::now() + Duration::from_secs(1),
        )
        .await
        .unwrap();
        assert!(
            request
                .recv()
                .unwrap()
                .starts_with("GET /api/v16/translations/..%2Fone HTTP/1.1\r\n")
        );
        assert_eq!(server.join().unwrap(), 1);
        for (status, kind) in [
            ("201 Created", FailureKind::OperationalHttpFailure),
            ("302 Found", FailureKind::OperationalHttpFailure),
            ("401 Unauthorized", FailureKind::AuthenticationFailure),
            ("403 Forbidden", FailureKind::AuthenticationFailure),
            ("429 Too Many Requests", FailureKind::OperationalHttpFailure),
            ("500 Server Error", FailureKind::OperationalHttpFailure),
        ] {
            let (api, request, server) = fixture(
                format!(
                    "HTTP/1.1 {status}\r\nContent-Length: invalid\r\n\r\nprivate upstream text"
                )
                .into_bytes(),
            );
            let failure = api
                .poll_geometry_translation(
                    "id",
                    tokio::time::Instant::now() + Duration::from_secs(1),
                )
                .await
                .unwrap_err();
            assert_eq!(failure.kind, kind, "{status}");
            assert!(!failure.to_string().contains("private"));
            request.recv().unwrap();
            assert_eq!(server.join().unwrap(), 1);
        }
    }

    #[tokio::test]
    async fn geometry_download_preserves_opaque_bytes_media_and_exact_resource_path() {
        let payload = [0, 255, 1, 128];
        let mut response = b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream;charset=utf-8\r\nContent-Length: 4\r\nConnection: close\r\n\r\n".to_vec();
        response.extend_from_slice(&payload);
        let (api, request, server) = fixture(response);
        let download = api
            .download_geometry(
                "d/one",
                "..",
                tokio::time::Instant::now() + Duration::from_secs(1),
            )
            .await
            .unwrap();
        assert_eq!(download.bytes, payload);
        assert_eq!(
            download.transport_media,
            "application/octet-stream;charset=utf-8"
        );
        let request = request.recv().unwrap();
        assert!(
            request
                .starts_with("GET /api/v16/documents/d/d%2Fone/externaldata/%2E%2E HTTP/1.1\r\n")
        );
        assert!(request.contains("accept: application/octet-stream\r\n"));
        assert!(request.contains("accept-encoding: identity\r\n"));
        assert!(!request.to_ascii_lowercase().contains("if-none-match"));
        assert_eq!(server.join().unwrap(), 1);
    }

    #[tokio::test]
    async fn geometry_download_rejects_header_defects_and_empty_payloads() {
        for declarations in [
            "Content-Length: 0\r\nContent-Type: application/octet-stream",
            "Content-Length: 1",
            "Content-Length: 1\r\nContent-Type: application/json",
            "Content-Length: 1\r\nContent-Type: application/octet-stream\r\nContent-Type: application/octet-stream",
            "Content-Length: 1\r\nContent-Type: application/octet-stream\r\nContent-Encoding: gzip",
            "Content-Length: 1\r\nContent-Type: application/octet-stream\r\nContent-Encoding: identity\r\nContent-Encoding: identity",
            "Content-Length: 134217729\r\nContent-Type: application/octet-stream",
        ] {
            let body = if declarations.starts_with("Content-Length: 0") {
                ""
            } else {
                "x"
            };
            let (api, request, server) = fixture(
                format!("HTTP/1.1 200 OK\r\n{declarations}\r\nConnection: close\r\n\r\n{body}")
                    .into_bytes(),
            );
            assert_eq!(
                api.download_geometry(
                    "d",
                    "id",
                    tokio::time::Instant::now() + Duration::from_secs(1)
                )
                .await
                .unwrap_err()
                .kind,
                FailureKind::OperationalApiContractFailure,
                "{declarations}"
            );
            request.recv().unwrap();
            assert_eq!(server.join().unwrap(), 1);
        }
    }

    #[tokio::test]
    async fn geometry_framing_enforces_actual_and_declared_byte_bounds() {
        for (framing, body, expected) in [
            ("Content-Length: 2", "xy", None),
            (
                "Content-Length: 3",
                "xyz",
                Some(FailureKind::OperationalApiContractFailure),
            ),
            ("", "xyz", Some(FailureKind::OperationalApiContractFailure)),
            (
                "Content-Length: 2",
                "x",
                Some(FailureKind::TransportFailure),
            ),
            (
                "Content-Length: 2",
                "xyz",
                Some(FailureKind::OperationalApiContractFailure),
            ),
            ("Transfer-Encoding: chunked", "2\r\nxy\r\n0\r\n\r\n", None),
            (
                "Transfer-Encoding: chunked",
                "3\r\nxyz\r\n0\r\n\r\n",
                Some(FailureKind::OperationalApiContractFailure),
            ),
            (
                "Transfer-Encoding: chunked",
                "2\r\nx",
                Some(FailureKind::TransportFailure),
            ),
        ] {
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\n{framing}\r\nConnection: close\r\n\r\n{body}").replace("\r\n\r\nConnection", "\r\nConnection").into_bytes();
            let (api, request, server) = fixture(response);
            let result = api
                .request_with_policy(
                    "downloadExternalData",
                    Method::GET,
                    &["api", "v16"],
                    &[],
                    None,
                    2,
                    ResponsePolicy::GeometryBytes,
                    Some(tokio::time::Instant::now() + Duration::from_secs(1)),
                )
                .await;
            match expected {
                None => assert_eq!(result.unwrap().bytes, b"xy"),
                Some(kind) => assert_eq!(result.unwrap_err().kind, kind, "{framing} {body}"),
            }
            request.recv().unwrap();
            assert_eq!(server.join().unwrap(), 1);
        }
    }

    #[tokio::test]
    async fn geometry_absolute_deadline_prevents_calls_and_clips_response_wait() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let api = OnshapeApi::for_test(&format!("http://{}", listener.local_addr().unwrap()));
        let expired = tokio::time::Instant::now();
        assert_eq!(
            api.poll_geometry_translation("id", expired)
                .await
                .unwrap_err()
                .kind,
            FailureKind::OperationalTimeoutFailure
        );
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );

        let (api, request, server) =
            fixture_with_delay(json_response("{}"), Duration::from_millis(100));
        assert_eq!(
            api.poll_geometry_translation(
                "id",
                tokio::time::Instant::now() + Duration::from_millis(20)
            )
            .await
            .unwrap_err()
            .kind,
            FailureKind::OperationalTimeoutFailure
        );
        request.recv().unwrap();
        assert_eq!(server.join().unwrap(), 1);
    }
}
