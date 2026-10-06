//! Trusted configuration provenance. Planning can validate a handoff but cannot encode.

use std::collections::BTreeMap;

use num_bigint::BigInt;
use num_rational::BigRational;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    cache_key,
    cache_model::{self, EncodedConfigurationIdentity, ResolvedOnshapeSourceIdentity},
    catalog::OnshapeSource,
    db::{ConfigurationEncodingInsert, ConfigurationEncodingRecord, Database},
    onshape_api::{self, API_BASELINE_SHA256, ApiFailure, FailureKind, OnshapeApi},
    parameters::{
        CanonicalParameterValue, QuantityDimension, SCHEMA_VERSION, encoding_request_values,
    },
};

const CONTEXT_DOMAIN: &str = "onshape-export-configuration-encoding-context-v1";
const EVIDENCE_LIMIT: usize = 65_536;

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EncodingHandoff {
    pub source_hash: String,
    pub config_hash: String,
    pub encoding_context_hash: String,
    pub encoded_id: String,
}

impl EncodingHandoff {
    pub fn validate_shape(&self) -> Result<(), ApiFailure> {
        if !onshape_api::is_sha256(&self.source_hash)
            || !onshape_api::is_sha256(&self.config_hash)
            || !onshape_api::is_sha256(&self.encoding_context_hash)
            || !onshape_api::bounded_visible_ascii(&self.encoded_id)
        {
            return Err(ApiFailure::new(
                FailureKind::InvalidSelection,
                "invalid_encoding_handoff",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct EncodingResolution {
    pub handoff: EncodingHandoff,
    pub identity: EncodedConfigurationIdentity,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EncodingContext {
    context_schema_version: u32,
    api_origin: String,
    api_major_version: u32,
    open_api_sha256: String,
    operation_id: String,
    method: String,
    path_template: String,
    document_id: String,
    version_id: String,
    resolved_microversion_id: String,
    element_id: String,
    element_kind: String,
    link_document_id: Option<String>,
    query: EncodingQuery,
    config_canonicalization_version: u32,
    parameter_schema_version: u32,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EncodingQuery {
    version_id: String,
}

fn failure(code: &str) -> ApiFailure {
    ApiFailure::new(FailureKind::OperationalApiContractFailure, code)
        .operation("encodingValidation")
}

fn validate_source(source: &ResolvedOnshapeSourceIdentity) -> Result<(), ApiFailure> {
    if source.link_document_id.is_some()
        || ![
            &source.document_id,
            &source.version_id,
            &source.microversion_id,
            &source.element_id,
        ]
        .into_iter()
        .all(|value| onshape_api::bounded_visible_ascii(value))
    {
        return Err(failure("invalid_encoding_source"));
    }
    Ok(())
}

pub fn context(
    origin: &str,
    source: &ResolvedOnshapeSourceIdentity,
) -> Result<EncodingContext, ApiFailure> {
    validate_source(source)?;
    if onshape_api::canonical_origin(origin)? != origin {
        return Err(failure("noncanonical_encoding_origin"));
    }
    Ok(EncodingContext {
        context_schema_version: 1,
        api_origin: origin.to_owned(),
        api_major_version: 16,
        open_api_sha256: API_BASELINE_SHA256.to_owned(),
        operation_id: "encodeConfigurationMap".to_owned(),
        method: "POST".to_owned(),
        path_template: "/api/v16/elements/d/{did}/e/{eid}/configurationencodings".to_owned(),
        document_id: source.document_id.clone(),
        version_id: source.version_id.clone(),
        resolved_microversion_id: source.microversion_id.clone(),
        element_id: source.element_id.clone(),
        element_kind: source.element_kind.key().to_owned(),
        link_document_id: None,
        query: EncodingQuery {
            version_id: source.version_id.clone(),
        },
        config_canonicalization_version: cache_key::CANONICALIZATION_VERSION,
        parameter_schema_version: SCHEMA_VERSION,
    })
}

pub fn context_hash(
    origin: &str,
    source: &ResolvedOnshapeSourceIdentity,
) -> Result<String, ApiFailure> {
    cache_key::hash_json(CONTEXT_DOMAIN, &context(origin, source)?)
        .map_err(|_| failure("encoding_context_canonicalization_failed"))
}

fn canonical_string(value: &impl Serialize) -> Result<String, ApiFailure> {
    let bytes = cache_key::canonical_json_bytes(value)
        .map_err(|_| failure("encoding_evidence_canonicalization_failed"))?;
    String::from_utf8(bytes).map_err(|_| failure("encoding_evidence_canonicalization_failed"))
}

pub fn request_body(typed: &BTreeMap<String, CanonicalParameterValue>) -> Value {
    let parameters = encoding_request_values(typed)
        .into_iter()
        .map(|(parameter_id, parameter_value)| {
            json!({"parameterId":parameter_id,"parameterValue":parameter_value})
        })
        .collect::<Vec<_>>();
    json!({"parameters":parameters})
}

struct Expected {
    source_hash: String,
    config_hash: String,
    context_hash: String,
    context: EncodingContext,
    request: Value,
}

fn expected(
    api: &OnshapeApi,
    source: &ResolvedOnshapeSourceIdentity,
    typed: &BTreeMap<String, CanonicalParameterValue>,
) -> Result<Expected, ApiFailure> {
    validate_typed_values(typed)?;
    onshape_api::parse_json(canonical_string(typed)?.as_bytes(), EVIDENCE_LIMIT)?;
    let context = context(api.origin(), source)?;
    let source_hash =
        cache_model::source_hash(source).map_err(|_| failure("encoding_source_hash_failed"))?;
    let config_hash = cache_model::config_hash(&source_hash, SCHEMA_VERSION, typed)
        .map_err(|_| failure("encoding_config_hash_failed"))?;
    let context_hash = cache_key::hash_json(CONTEXT_DOMAIN, &context)
        .map_err(|_| failure("encoding_context_hash_failed"))?;
    let request = request_body(typed);
    // The same JSON bounds apply to locally generated and persisted request evidence.
    onshape_api::parse_json(canonical_string(&request)?.as_bytes(), EVIDENCE_LIMIT)?;
    Ok(Expected {
        source_hash,
        config_hash,
        context_hash,
        context,
        request,
    })
}

fn validate_typed_values(
    typed: &BTreeMap<String, CanonicalParameterValue>,
) -> Result<(), ApiFailure> {
    for value in typed.values() {
        let (numerator, denominator) = match value {
            CanonicalParameterValue::Number {
                numerator,
                denominator,
            } => (numerator, denominator),
            CanonicalParameterValue::Quantity {
                dimension,
                numerator,
                denominator,
                unit,
            } => {
                let valid_unit = matches!(
                    (dimension, unit.as_str()),
                    (QuantityDimension::Length, "m") | (QuantityDimension::Angle, "deg" | "rad")
                );
                if !valid_unit {
                    return Err(failure("invalid_encoding_quantity_unit"));
                }
                (numerator, denominator)
            }
            _ => continue,
        };
        let numerator_value: BigInt = numerator
            .parse()
            .map_err(|_| failure("invalid_encoding_rational"))?;
        let denominator_value: BigInt = denominator
            .parse()
            .map_err(|_| failure("invalid_encoding_rational"))?;
        if denominator_value <= BigInt::from(0) {
            return Err(failure("invalid_encoding_rational"));
        }
        let canonical = BigRational::new(numerator_value, denominator_value);
        if canonical.numer().to_string() != *numerator
            || canonical.denom().to_string() != *denominator
        {
            return Err(failure("noncanonical_encoding_rational"));
        }
    }
    Ok(())
}

fn response_identity(
    response: &Value,
    require_query: bool,
) -> Result<EncodedConfigurationIdentity, ApiFailure> {
    let object = response
        .as_object()
        .ok_or_else(|| failure("invalid_encoding_response"))?;
    let encoded_id = object
        .get("encodedId")
        .and_then(Value::as_str)
        .filter(|value| onshape_api::bounded_visible_ascii(value))
        .ok_or_else(|| failure("invalid_encoding_encoded_id"))?;
    let query_param = object
        .get("queryParam")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_graphic()));
    if require_query && query_param.is_none() {
        return Err(failure("invalid_encoding_query_param"));
    }
    let query_param = query_param.unwrap_or_default();
    Ok(EncodedConfigurationIdentity {
        encoded_id: encoded_id.to_owned(),
        query_param: query_param.to_owned(),
    })
}

fn validate_record(
    record: &ConfigurationEncodingRecord,
    expected: &Expected,
    require_query: bool,
) -> Result<EncodingResolution, ApiFailure> {
    let stored_context =
        onshape_api::parse_json(record.encoding_context_json.as_bytes(), EVIDENCE_LIMIT)?;
    let decoded: EncodingContext = serde_json::from_value(stored_context.clone())
        .map_err(|_| failure("invalid_encoding_context"))?;
    // Equality of the entire JSON also rejects omitted nullable fields.
    if decoded != expected.context
        || stored_context
            != serde_json::to_value(&expected.context)
                .map_err(|_| failure("invalid_encoding_context"))?
        || cache_key::hash_json(CONTEXT_DOMAIN, &decoded)
            .map_err(|_| failure("encoding_context_hash_failed"))?
            != record.encoding_context_hash
        || record.source_hash != expected.source_hash
        || record.config_hash != expected.config_hash
        || record.encoding_context_hash != expected.context_hash
    {
        return Err(failure("encoding_context_mismatch"));
    }
    let request = onshape_api::parse_json(record.request_json.as_bytes(), EVIDENCE_LIMIT)?;
    if request != expected.request {
        return Err(failure("encoding_request_mismatch"));
    }
    let response = onshape_api::parse_json(record.response_json.as_bytes(), EVIDENCE_LIMIT)?;
    let identity = response_identity(&response, require_query)?;
    if record.encoded_id != identity.encoded_id
        || (require_query && record.query_param != identity.query_param)
    {
        return Err(failure("encoding_response_mismatch"));
    }
    Ok(EncodingResolution {
        handoff: EncodingHandoff {
            source_hash: expected.source_hash.clone(),
            config_hash: expected.config_hash.clone(),
            encoding_context_hash: expected.context_hash.clone(),
            encoded_id: identity.encoded_id.clone(),
        },
        identity,
    })
}

/// The coordinator always resolves the caller version. Only absence of an active row permits encoding.
pub async fn resolve(
    db: &Database,
    api: &OnshapeApi,
    source: &OnshapeSource,
    typed: &BTreeMap<String, CanonicalParameterValue>,
) -> Result<EncodingHandoff, ApiFailure> {
    resolve_inner(db, api, source, typed, None, false)
        .await
        .map(|resolution| resolution.handoff)
}

pub(crate) async fn resolve_for_hashes(
    db: &Database,
    api: &OnshapeApi,
    source: &OnshapeSource,
    typed: &BTreeMap<String, CanonicalParameterValue>,
    source_hash: &str,
    config_hash: &str,
) -> Result<EncodingResolution, ApiFailure> {
    resolve_inner(
        db,
        api,
        source,
        typed,
        Some((source_hash, config_hash)),
        true,
    )
    .await
}

async fn resolve_inner(
    db: &Database,
    api: &OnshapeApi,
    source: &OnshapeSource,
    typed: &BTreeMap<String, CanonicalParameterValue>,
    bound: Option<(&str, &str)>,
    require_query: bool,
) -> Result<EncodingResolution, ApiFailure> {
    validate_typed_values(typed)?;
    onshape_api::parse_json(canonical_string(typed)?.as_bytes(), EVIDENCE_LIMIT)?;
    if source.link_document_id.is_some()
        || ![&source.document_id, &source.version_id, &source.element_id]
            .into_iter()
            .all(|value| onshape_api::bounded_visible_ascii(value))
    {
        return Err(failure("invalid_encoding_source"));
    }
    let resolved = ResolvedOnshapeSourceIdentity {
        document_id: source.document_id.clone(),
        version_id: source.version_id.clone(),
        microversion_id: api
            .resolve_version(&source.document_id, &source.version_id)
            .await?,
        element_id: source.element_id.clone(),
        element_kind: source.element_kind.clone(),
        link_document_id: None,
    };
    let expected = expected(api, &resolved, typed)?;
    if let Some((source_hash, config_hash)) = bound
        && (expected.source_hash != source_hash || expected.config_hash != config_hash)
    {
        return Err(failure("encoding_coordinator_binding_mismatch"));
    }
    if let Some(record) = db
        .configuration_encoding(
            &expected.source_hash,
            &expected.config_hash,
            &expected.context_hash,
        )
        .await
        .map_err(|_| failure("encoding_cache_read_failed"))?
    {
        let validated = validate_record(&record, &expected, require_query)?;
        retain_typed_selection(db, &expected, typed).await?;
        return Ok(validated);
    }
    let response = api
        .encode(
            &source.document_id,
            &source.version_id,
            &source.element_id,
            &expected.request,
        )
        .await?;
    let response_json = canonical_string(&response)?;
    // Fresh evidence is checked through the exact same validator before any insert.
    let record = ConfigurationEncodingRecord {
        source_hash: expected.source_hash.clone(),
        config_hash: expected.config_hash.clone(),
        encoding_context_hash: expected.context_hash.clone(),
        encoding_context_json: canonical_string(&expected.context)?,
        encoded_id: response_identity(&response, require_query)?.encoded_id,
        query_param: response_identity(&response, require_query)?.query_param,
        request_json: canonical_string(&expected.request)?,
        response_json,
        created_at: String::new(),
        updated_at: String::new(),
    };
    let validated = validate_record(&record, &expected, require_query)?;
    retain_typed_selection(db, &expected, typed).await?;
    let inserted = db
        .insert_configuration_encoding_if_absent(ConfigurationEncodingInsert {
            source_hash: &record.source_hash,
            config_hash: &record.config_hash,
            encoding_context_hash: &record.encoding_context_hash,
            encoding_context_json: &record.encoding_context_json,
            encoded_id: &record.encoded_id,
            query_param: &record.query_param,
            request_json: &record.request_json,
            response_json: &record.response_json,
        })
        .await
        .map_err(|_| failure("encoding_cache_insert_failed"))?;
    if inserted {
        return Ok(validated);
    }
    let winner = db
        .configuration_encoding(
            &expected.source_hash,
            &expected.config_hash,
            &expected.context_hash,
        )
        .await
        .map_err(|_| failure("encoding_cache_read_failed"))?
        .ok_or_else(|| failure("encoding_cache_conflict_missing"))?;
    let winner_resolution = validate_record(&winner, &expected, require_query)?;
    if winner_resolution.handoff.encoded_id != validated.handoff.encoded_id
        || (require_query
            && winner_resolution.identity.query_param != validated.identity.query_param)
    {
        return Err(failure("encoding_concurrent_evidence_conflict"));
    }
    Ok(winner_resolution)
}

async fn retain_typed_selection(
    db: &Database,
    expected: &Expected,
    typed: &BTreeMap<String, CanonicalParameterValue>,
) -> Result<(), ApiFailure> {
    db.insert_configuration_selection_if_absent(crate::db::ConfigurationSelectionUpsert {
        source_hash: &expected.source_hash,
        config_hash: &expected.config_hash,
        values_json: &canonical_string(typed)?,
        validation_json: &canonical_string(&json!({
            "parameterSchemaVersion":SCHEMA_VERSION,
            "requestValues":encoding_request_values(typed),
        }))?,
    })
    .await
    .map_err(|_| failure("encoding_selection_insert_failed"))?;
    if stored_typed_values(db, &expected.source_hash, &expected.config_hash).await? != *typed {
        return Err(failure("encoding_selection_evidence_conflict"));
    }
    Ok(())
}

async fn stored_typed_values(
    db: &Database,
    source_hash: &str,
    config_hash: &str,
) -> Result<BTreeMap<String, CanonicalParameterValue>, ApiFailure> {
    let selection = db
        .configuration_selection(source_hash, config_hash)
        .await
        .map_err(|_| failure("encoding_selection_read_failed"))?
        .ok_or_else(|| failure("encoding_selection_missing"))?;
    let raw = onshape_api::parse_json(selection.values_json.as_bytes(), EVIDENCE_LIMIT)?;
    let typed: BTreeMap<String, CanonicalParameterValue> = serde_json::from_value(raw.clone())
        .map_err(|_| failure("invalid_encoding_typed_values"))?;
    if serde_json::to_value(&typed).map_err(|_| failure("invalid_encoding_typed_values"))? != raw {
        return Err(failure("invalid_encoding_typed_values"));
    }
    Ok(typed)
}

/// Read-only cache lookup used by export status callers. Never repairs or encodes.
pub(crate) async fn lookup(
    db: &Database,
    api: &OnshapeApi,
    source: &ResolvedOnshapeSourceIdentity,
    source_hash: &str,
    config_hash: &str,
) -> Result<Option<EncodingResolution>, ApiFailure> {
    if !onshape_api::is_sha256(source_hash)
        || !onshape_api::is_sha256(config_hash)
        || cache_model::source_hash(source).map_err(|_| failure("encoding_source_hash_failed"))?
            != source_hash
    {
        return Err(failure("encoding_lookup_binding_mismatch"));
    }
    let context_hash = context_hash(api.origin(), source)?;
    let Some(record) = db
        .configuration_encoding(source_hash, config_hash, &context_hash)
        .await
        .map_err(|_| failure("encoding_cache_read_failed"))?
    else {
        return Ok(None);
    };
    let typed = stored_typed_values(db, source_hash, config_hash).await?;
    let expected = expected(api, source, &typed)?;
    validate_record(&record, &expected, true).map(Some)
}

/// Planning revalidates the existing handoff against its independently resolved root.
/// This path has no reference to the encoding operation and cannot fill a missing row.
pub async fn validate_handoff(
    db: &Database,
    api: &OnshapeApi,
    source: &ResolvedOnshapeSourceIdentity,
    handoff: &EncodingHandoff,
) -> Result<(), ApiFailure> {
    handoff.validate_shape()?;
    let typed = stored_typed_values(db, &handoff.source_hash, &handoff.config_hash).await?;
    let expected = expected(api, source, &typed)?;
    if handoff.source_hash != expected.source_hash
        || handoff.config_hash != expected.config_hash
        || handoff.encoding_context_hash != expected.context_hash
    {
        return Err(failure("encoding_handoff_binding_mismatch"));
    }
    let record = db
        .configuration_encoding(
            &expected.source_hash,
            &expected.config_hash,
            &expected.context_hash,
        )
        .await
        .map_err(|_| failure("encoding_cache_read_failed"))?
        .ok_or_else(|| failure("encoding_handoff_cache_missing"))?;
    let validated = validate_record(&record, &expected, false)?;
    if validated.handoff != *handoff {
        return Err(failure("encoding_handoff_encoded_id_mismatch"));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) async fn seed(
    db: &Database,
    api: &OnshapeApi,
    source: &ResolvedOnshapeSourceIdentity,
    typed: &BTreeMap<String, CanonicalParameterValue>,
    encoded_id: &str,
    query_param: &str,
) -> EncodingHandoff {
    let expected = expected(api, source, typed).unwrap();
    db.upsert_configuration_selection(crate::db::ConfigurationSelectionUpsert {
        source_hash: &expected.source_hash,
        config_hash: &expected.config_hash,
        values_json: &canonical_string(typed).unwrap(),
        validation_json: &canonical_string(&json!({
            "parameterSchemaVersion":SCHEMA_VERSION,
            "requestValues":encoding_request_values(typed),
        }))
        .unwrap(),
    })
    .await
    .unwrap();
    db.insert_configuration_encoding_if_absent(ConfigurationEncodingInsert {
        source_hash: &expected.source_hash,
        config_hash: &expected.config_hash,
        encoding_context_hash: &expected.context_hash,
        encoding_context_json: &canonical_string(&expected.context).unwrap(),
        encoded_id,
        query_param,
        request_json: &canonical_string(&expected.request).unwrap(),
        response_json: &canonical_string(&json!({"encodedId":encoded_id,"queryParam":query_param}))
            .unwrap(),
    })
    .await
    .unwrap();
    EncodingHandoff {
        source_hash: expected.source_hash,
        config_hash: expected.config_hash,
        encoding_context_hash: expected.context_hash,
        encoded_id: encoded_id.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::Duration,
    };

    use super::*;
    use crate::catalog::ElementKind;

    fn source() -> ResolvedOnshapeSourceIdentity {
        ResolvedOnshapeSourceIdentity {
            document_id: "d".to_owned(),
            version_id: "v".to_owned(),
            microversion_id: "m".to_owned(),
            element_id: "e".to_owned(),
            element_kind: ElementKind::PartStudio,
            link_document_id: None,
        }
    }

    fn root() -> OnshapeSource {
        OnshapeSource {
            document_id: "d".to_owned(),
            version_id: "v".to_owned(),
            element_id: "e".to_owned(),
            element_kind: ElementKind::PartStudio,
            link_document_id: None,
        }
    }

    fn typed() -> BTreeMap<String, CanonicalParameterValue> {
        BTreeMap::from([(
            "flag".to_owned(),
            CanonicalParameterValue::Boolean { value: true },
        )])
    }

    fn fixture(responses: Vec<Value>) -> (OnshapeApi, thread::JoinHandle<Vec<String>>) {
        fixture_with_hook(responses, |_| {})
    }

    fn fixture_with_hook(
        responses: Vec<Value>,
        mut before_response: impl FnMut(usize) + Send + 'static,
    ) -> (OnshapeApi, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let api = OnshapeApi::for_test(&format!("http://{}", listener.local_addr().unwrap()));
        let server = thread::spawn(move || {
            listener.set_nonblocking(true).unwrap();
            let mut requests = Vec::new();
            for (index, response) in responses.into_iter().enumerate() {
                let started = std::time::Instant::now();
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(
                                started.elapsed() < Duration::from_secs(5),
                                "missing synthetic request"
                            );
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("synthetic listener failure: {error}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut byte = [0_u8];
                while !bytes.ends_with(b"\r\n\r\n") {
                    stream.read_exact(&mut byte).unwrap();
                    bytes.push(byte[0]);
                }
                let headers = String::from_utf8(bytes.clone()).unwrap();
                let body_length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .and_then(|value| value.parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                let mut body = vec![0; body_length];
                stream.read_exact(&mut body).unwrap();
                bytes.extend(body);
                requests.push(String::from_utf8(bytes).unwrap());
                before_response(index);
                let body = response.to_string();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            requests
        });
        (api, server)
    }

    fn version() -> Value {
        json!({"documentId":"d","id":"v","microversion":"m"})
    }

    #[test]
    fn context_has_exact_golden_hash_and_keeps_version_out_of_source_and_config() {
        let first = source();
        assert_eq!(
            context_hash(onshape_api::PRODUCTION_ORIGIN, &first).unwrap(),
            "fcd5993bb20fcb91dfede6ed40fdd2c7ab8bd154f5607d432d806a3bbad2c596"
        );
        let mut second = first.clone();
        second.version_id = "other-version".to_owned();
        let first_source = cache_model::source_hash(&first).unwrap();
        let second_source = cache_model::source_hash(&second).unwrap();
        assert_eq!(first_source, second_source);
        assert_eq!(
            cache_model::config_hash(&first_source, SCHEMA_VERSION, &typed()).unwrap(),
            cache_model::config_hash(&second_source, SCHEMA_VERSION, &typed()).unwrap()
        );
        assert_ne!(
            context_hash(onshape_api::PRODUCTION_ORIGIN, &first).unwrap(),
            context_hash(onshape_api::PRODUCTION_ORIGIN, &second).unwrap()
        );
        let expected = context(onshape_api::PRODUCTION_ORIGIN, &first).unwrap();
        let baseline = cache_key::hash_json(CONTEXT_DOMAIN, &expected).unwrap();
        for field in [
            "apiOrigin",
            "openApiSha256",
            "operationId",
            "method",
            "pathTemplate",
            "documentId",
            "versionId",
            "resolvedMicroversionId",
            "elementId",
            "elementKind",
        ] {
            let mut changed = serde_json::to_value(&expected).unwrap();
            changed[field] = json!("changed");
            assert_ne!(
                cache_key::hash_json(CONTEXT_DOMAIN, &changed).unwrap(),
                baseline,
                "{field}"
            );
        }
        for field in [
            "contextSchemaVersion",
            "apiMajorVersion",
            "configCanonicalizationVersion",
            "parameterSchemaVersion",
        ] {
            let mut changed = serde_json::to_value(&expected).unwrap();
            changed[field] = json!(99);
            assert_ne!(
                cache_key::hash_json(CONTEXT_DOMAIN, &changed).unwrap(),
                baseline,
                "{field}"
            );
        }
        let mut linked = first;
        linked.link_document_id = Some("linked".to_owned());
        assert!(context(onshape_api::PRODUCTION_ORIGIN, &linked).is_err());
    }

    #[test]
    fn canonical_typed_numbers_and_quantity_units_fail_closed() {
        for (numerator, denominator) in [
            ("1", "0"),
            ("1", "-1"),
            ("2", "4"),
            ("+1", "1"),
            ("-0", "1"),
            ("01", "1"),
            ("invalid", "1"),
        ] {
            let values = BTreeMap::from([(
                "n".to_owned(),
                CanonicalParameterValue::Number {
                    numerator: numerator.to_owned(),
                    denominator: denominator.to_owned(),
                },
            )]);
            assert!(
                validate_typed_values(&values).is_err(),
                "{numerator}/{denominator}"
            );
        }
        for (dimension, unit) in [
            (QuantityDimension::Length, "mm"),
            (QuantityDimension::Length, "deg"),
            (QuantityDimension::Angle, "m"),
        ] {
            let values = BTreeMap::from([(
                "n".to_owned(),
                CanonicalParameterValue::Quantity {
                    dimension,
                    unit: unit.to_owned(),
                    numerator: "1".to_owned(),
                    denominator: "1".to_owned(),
                },
            )]);
            assert!(validate_typed_values(&values).is_err());
        }
    }

    #[tokio::test]
    async fn true_miss_encodes_once_then_hit_only_resolves_version() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let (api, server) = fixture(vec![
            version(),
            json!({"encodedId":"enc&=exact","queryParam":"configuration=enc&=exact"}),
            version(),
        ]);
        let first = resolve(&db, &api, &root(), &typed()).await.unwrap();
        validate_handoff(&db, &api, &source(), &first)
            .await
            .unwrap();
        let second = resolve(&db, &api, &root(), &typed()).await.unwrap();
        assert_eq!(first, second);
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 3);
        assert!(requests[1].starts_with(
            "POST /api/v16/elements/d/d/e/e/configurationencodings?versionId=v HTTP/1.1\r\n"
        ));
        assert_eq!(
            requests[1].split("\r\n\r\n").nth(1).unwrap(),
            canonical_string(&request_body(&typed())).unwrap()
        );
        assert!(
            requests[0]
                .starts_with("GET /api/v16/documents/d/d/versions/v?parents=false HTTP/1.1\r\n")
        );
        assert!(
            requests[2]
                .starts_with("GET /api/v16/documents/d/d/versions/v?parents=false HTTP/1.1\r\n")
        );
    }

    #[tokio::test]
    async fn maximum_encoded_id_can_have_longer_valid_query_parameter_evidence() {
        let encoded_id = "x".repeat(4_096);
        let query_param = format!("configuration={encoded_id}");
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let (api, server) = fixture(vec![
            version(),
            json!({"encodedId":encoded_id,"queryParam":query_param}),
            version(),
        ]);
        let handoff = resolve(&db, &api, &root(), &typed()).await.unwrap();
        assert_eq!(handoff.encoded_id, encoded_id);
        validate_handoff(&db, &api, &source(), &handoff)
            .await
            .unwrap();
        let legacy = resolve_for_hashes(
            &db,
            &api,
            &root(),
            &typed(),
            &handoff.source_hash,
            &handoff.config_hash,
        )
        .await
        .unwrap();
        assert_eq!(legacy.identity.query_param, query_param);
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 3);
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.starts_with("POST "))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn planning_encoding_needs_only_encoded_id_and_legacy_hit_requires_query_evidence() {
        for response in [
            json!({"encodedId":"enc-only"}),
            json!({"encodedId":"enc-only","queryParam":null}),
        ] {
            let db = Database::connect("sqlite::memory:").await.unwrap();
            let (api, server) = fixture(vec![version(), response, version(), version()]);
            let first = resolve(&db, &api, &root(), &typed()).await.unwrap();
            assert_eq!(first.encoded_id, "enc-only");
            validate_handoff(&db, &api, &source(), &first)
                .await
                .unwrap();
            assert_eq!(resolve(&db, &api, &root(), &typed()).await.unwrap(), first);
            assert_eq!(
                resolve_for_hashes(
                    &db,
                    &api,
                    &root(),
                    &typed(),
                    &first.source_hash,
                    &first.config_hash
                )
                .await
                .unwrap_err()
                .kind,
                FailureKind::OperationalApiContractFailure
            );
            let record = db
                .configuration_encoding(
                    &first.source_hash,
                    &first.config_hash,
                    &first.encoding_context_hash,
                )
                .await
                .unwrap()
                .unwrap();
            assert_eq!(record.encoded_id, "enc-only");
            assert!(record.query_param.is_empty());
            let requests = server.join().unwrap();
            assert_eq!(requests.len(), 4);
            assert_eq!(
                requests
                    .iter()
                    .filter(|request| request.starts_with("POST "))
                    .count(),
                1
            );
        }
    }

    #[tokio::test]
    async fn strict_evidence_rejects_mutation_and_handoff_tampering_without_calls() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let api = OnshapeApi::for_test("http://127.0.0.1:9");
        let handoff = seed(&db, &api, &source(), &typed(), "enc", "configuration=enc").await;
        let record = db
            .configuration_encoding(
                &handoff.source_hash,
                &handoff.config_hash,
                &handoff.encoding_context_hash,
            )
            .await
            .unwrap()
            .unwrap();
        let expected = expected(&api, &source(), &typed()).unwrap();
        assert!(validate_record(&record, &expected, true).is_ok());
        for (field, value) in [
            ("context", "{}"),
            (
                "context",
                "{\"contextSchemaVersion\":1,\"contextSchemaVersion\":1}",
            ),
            ("request", "{\"parameters\":[]}"),
            ("request", "{\"parameters\":[],\"parameters\":[]}"),
            (
                "response",
                "{\"encodedId\":\"wrong\",\"queryParam\":\"configuration=enc\"}",
            ),
            ("response", "{\"encodedId\":\"enc\",\"encodedId\":\"enc\"}"),
            (
                "response",
                "{\"encodedId\":\"\",\"queryParam\":\"configuration=enc\"}",
            ),
        ] {
            let mut changed = record.clone();
            match field {
                "context" => changed.encoding_context_json = value.to_owned(),
                "request" => changed.request_json = value.to_owned(),
                _ => changed.response_json = value.to_owned(),
            }
            assert_eq!(
                validate_record(&changed, &expected, true).unwrap_err().kind,
                FailureKind::OperationalApiContractFailure
            );
        }
        let mut changed = handoff.clone();
        changed.encoded_id = "forged".to_owned();
        assert!(
            validate_handoff(&db, &api, &source(), &changed)
                .await
                .is_err()
        );
        let mut other_version = source();
        other_version.version_id = "other".to_owned();
        assert!(
            validate_handoff(&db, &api, &other_version, &handoff)
                .await
                .is_err()
        );
        let mut wire = serde_json::to_value(&handoff).unwrap();
        wire["queryParam"] = json!("forbidden");
        assert!(serde_json::from_value::<EncodingHandoff>(wire).is_err());
        let invalid_typed = BTreeMap::from([(
            "number".to_owned(),
            CanonicalParameterValue::Number {
                numerator: "1".to_owned(),
                denominator: "0".to_owned(),
            },
        )]);
        assert_eq!(
            resolve(&db, &api, &root(), &invalid_typed)
                .await
                .unwrap_err()
                .kind,
            FailureKind::OperationalApiContractFailure
        );
    }

    #[tokio::test]
    async fn invalid_active_row_never_authorizes_encoding_and_fresh_invalid_response_is_not_inserted()
     {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let (api, server) = fixture(vec![version()]);
        let expected = expected(&api, &source(), &typed()).unwrap();
        db.insert_configuration_encoding_if_absent(ConfigurationEncodingInsert {
            source_hash: &expected.source_hash,
            config_hash: &expected.config_hash,
            encoding_context_hash: &expected.context_hash,
            encoding_context_json: "{}",
            encoded_id: "enc",
            query_param: "configuration=enc",
            request_json: "{}",
            response_json: "{}",
        })
        .await
        .unwrap();
        assert_eq!(
            resolve(&db, &api, &root(), &typed())
                .await
                .unwrap_err()
                .kind,
            FailureKind::OperationalApiContractFailure
        );
        assert_eq!(server.join().unwrap().len(), 1);
        let other_db = Database::connect("sqlite::memory:").await.unwrap();
        let (api, server) = fixture(vec![version(), json!({"encodedId":""})]);
        assert_eq!(
            resolve(&other_db, &api, &root(), &typed())
                .await
                .unwrap_err()
                .kind,
            FailureKind::OperationalApiContractFailure
        );
        assert!(
            other_db
                .configuration_encoding(
                    &expected.source_hash,
                    &expected.config_hash,
                    &expected.context_hash
                )
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(server.join().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn concurrent_insert_winner_is_revalidated_without_overwrite() {
        for conflicting in [false, true] {
            let db = Database::connect("sqlite::memory:").await.unwrap();
            let (reached_sender, reached_receiver) = tokio::sync::oneshot::channel();
            let mut reached_sender = Some(reached_sender);
            let (continue_sender, continue_receiver) = std::sync::mpsc::channel();
            let (api, server) = fixture_with_hook(
                vec![
                    version(),
                    json!({"encodedId":"enc","queryParam":"configuration=enc","futureUnconsumedCounter":2}),
                ],
                move |index| {
                    if index == 1 {
                        reached_sender.take().unwrap().send(()).unwrap();
                        continue_receiver
                            .recv_timeout(Duration::from_secs(5))
                            .unwrap();
                    }
                },
            );
            let resolving_db = db.clone();
            let resolving_api = api.clone();
            let resolving = tokio::spawn(async move {
                resolve(&resolving_db, &resolving_api, &root(), &typed()).await
            });
            reached_receiver.await.unwrap();
            let winner_id = if conflicting { "other-enc" } else { "enc" };
            let handoff = seed(
                &db,
                &api,
                &source(),
                &typed(),
                winner_id,
                &format!("configuration={winner_id}"),
            )
            .await;
            continue_sender.send(()).unwrap();
            let result = resolving.await.unwrap();
            if conflicting {
                assert_eq!(
                    result.unwrap_err().kind,
                    FailureKind::OperationalApiContractFailure
                );
            } else {
                assert_eq!(result.unwrap(), handoff);
            }
            let winner = db
                .configuration_encoding(
                    &handoff.source_hash,
                    &handoff.config_hash,
                    &handoff.encoding_context_hash,
                )
                .await
                .unwrap()
                .unwrap();
            assert_eq!(winner.encoded_id, winner_id);
            assert_eq!(server.join().unwrap().len(), 2);
        }
    }
}
