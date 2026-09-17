use std::collections::{BTreeMap, HashMap};

use serde::Serialize;

use crate::{
    cache_key, catalog,
    parameters::{CanonicalParameterValue, ParameterSchema},
};

pub const RESPONSE_SHAPE_VERSION: u32 = 1;
pub const ARTIFACT_SET_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedOnshapeSourceIdentity {
    pub document_id: String,
    pub version_id: String,
    pub microversion_id: String,
    pub element_id: String,
    pub element_kind: catalog::ElementKind,
    pub link_document_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EncodedConfigurationIdentity {
    pub encoded_id: String,
    pub query_param: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestIdentity<TPathParams, TQuery, TBody>
where
    TPathParams: Serialize,
    TQuery: Serialize,
    TBody: Serialize,
{
    pub api_host_class: String,
    pub api_spec_version: Option<String>,
    pub operation: String,
    pub method: String,
    pub path_template: String,
    pub path_params: TPathParams,
    pub query: TQuery,
    pub body: TBody,
    pub encoded_configuration: Option<EncodedConfigurationIdentity>,
    pub defaults_policy_version: String,
    pub request_builder_version: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseIdentity<TStartResponse, TFinalResponse, TPollState>
where
    TStartResponse: Serialize,
    TFinalResponse: Serialize,
    TPollState: Serialize,
{
    pub translation_id: String,
    pub start_response: TStartResponse,
    pub final_response: TFinalResponse,
    pub poll_state: TPollState,
    pub response_shape_version: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostprocessIdentity<TPolicy>
where
    TPolicy: Serialize,
{
    pub raw_payload_hash: String,
    pub processor_name: String,
    pub processor_version: String,
    pub policy: TPolicy,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactSetIdentity {
    pub artifact_set_schema_version: u32,
    pub output_kind: String,
    pub format: String,
    pub source_hash: String,
    pub config_hash: String,
    pub options_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_payload_hash: Option<String>,
    pub postprocess_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generator_processing_hash: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceHashPayload<'a> {
    document_id: &'a str,
    microversion_id: &'a str,
    element_id: &'a str,
    element_kind: &'a str,
    link_document_id: Option<&'a str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfigHashPayload<'a> {
    canonicalization_version: u32,
    source_hash: &'a str,
    parameter_schema_version: u32,
    typed_values: &'a BTreeMap<String, CanonicalParameterValue>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OptionsHashPayload<'a, T> {
    options_version: &'static str,
    format: &'a str,
    options: &'a T,
}

pub fn canonical_values(values: &HashMap<String, String>) -> BTreeMap<String, String> {
    values
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

pub fn source_hash(identity: &ResolvedOnshapeSourceIdentity) -> anyhow::Result<String> {
    cache_key::hash_json(
        "source-v2",
        &SourceHashPayload {
            document_id: &identity.document_id,
            microversion_id: &identity.microversion_id,
            element_id: &identity.element_id,
            element_kind: identity.element_kind.key(),
            link_document_id: identity.link_document_id.as_deref(),
        },
    )
}

pub fn config_hash(
    source_hash: &str,
    parameter_schema_version: u32,
    typed_values: &BTreeMap<String, CanonicalParameterValue>,
) -> anyhow::Result<String> {
    cache_key::hash_json(
        "config-v2",
        &ConfigHashPayload {
            canonicalization_version: cache_key::CANONICALIZATION_VERSION,
            source_hash,
            parameter_schema_version,
            typed_values,
        },
    )
}

pub fn parameter_schema_hash(schema: &ParameterSchema) -> anyhow::Result<String> {
    cache_key::hash_json("parameter-schema-v2", schema)
}

pub fn options_hash<T>(
    format: &str,
    options_version: &'static str,
    options: &T,
) -> anyhow::Result<String>
where
    T: Serialize,
{
    cache_key::hash_json(
        "options-v2",
        &OptionsHashPayload {
            options_version,
            format,
            options,
        },
    )
}

pub fn request_hash<TPathParams, TQuery, TBody>(
    identity: &RequestIdentity<TPathParams, TQuery, TBody>,
) -> anyhow::Result<String>
where
    TPathParams: Serialize,
    TQuery: Serialize,
    TBody: Serialize,
{
    cache_key::hash_json("request-v2", identity)
}

pub fn response_hash<TStartResponse, TFinalResponse, TPollState>(
    identity: &ResponseIdentity<TStartResponse, TFinalResponse, TPollState>,
) -> anyhow::Result<String>
where
    TStartResponse: Serialize,
    TFinalResponse: Serialize,
    TPollState: Serialize,
{
    cache_key::hash_json("response-v2", identity)
}

pub fn postprocess_hash<TPolicy>(identity: &PostprocessIdentity<TPolicy>) -> anyhow::Result<String>
where
    TPolicy: Serialize,
{
    cache_key::hash_json("postprocess-v2", identity)
}

pub fn artifact_set_hash(identity: &ArtifactSetIdentity) -> anyhow::Result<String> {
    cache_key::hash_json("artifact-set-v2", identity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::ElementKind;
    use crate::parameters::CanonicalParameterValue;
    use serde_json::json;

    #[test]
    fn source_hash_ignores_version_id_once_microversion_is_resolved() {
        let first = ResolvedOnshapeSourceIdentity {
            document_id: "did".to_owned(),
            version_id: "vid-1".to_owned(),
            microversion_id: "mid".to_owned(),
            element_id: "eid".to_owned(),
            element_kind: ElementKind::PartStudio,
            link_document_id: None,
        };
        let second = ResolvedOnshapeSourceIdentity {
            version_id: "vid-2".to_owned(),
            ..first.clone()
        };

        assert_eq!(source_hash(&first).unwrap(), source_hash(&second).unwrap());
    }

    #[test]
    fn source_hash_changes_when_microversion_changes() {
        let first = ResolvedOnshapeSourceIdentity {
            document_id: "did".to_owned(),
            version_id: "vid".to_owned(),
            microversion_id: "mid-1".to_owned(),
            element_id: "eid".to_owned(),
            element_kind: ElementKind::PartStudio,
            link_document_id: None,
        };
        let second = ResolvedOnshapeSourceIdentity {
            microversion_id: "mid-2".to_owned(),
            ..first.clone()
        };

        assert_ne!(source_hash(&first).unwrap(), source_hash(&second).unwrap());
    }

    #[test]
    fn config_hash_uses_source_hash_and_canonical_values() {
        let source_hash = "source";
        let first = BTreeMap::from([
            (
                "a".to_owned(),
                CanonicalParameterValue::Number {
                    numerator: "1".to_owned(),
                    denominator: "1".to_owned(),
                },
            ),
            (
                "b".to_owned(),
                CanonicalParameterValue::Boolean { value: true },
            ),
        ]);
        let second = BTreeMap::from([
            (
                "b".to_owned(),
                CanonicalParameterValue::Boolean { value: true },
            ),
            (
                "a".to_owned(),
                CanonicalParameterValue::Number {
                    numerator: "1".to_owned(),
                    denominator: "1".to_owned(),
                },
            ),
        ]);

        assert_eq!(
            config_hash(source_hash, 2, &first).unwrap(),
            config_hash(source_hash, 2, &second).unwrap()
        );
        assert_ne!(
            config_hash(source_hash, 2, &first).unwrap(),
            config_hash("other", 2, &second).unwrap()
        );
    }

    #[test]
    fn config_hash_distinguishes_boolean_and_text_values() {
        let boolean = BTreeMap::from([(
            "enabled".to_owned(),
            CanonicalParameterValue::Boolean { value: true },
        )]);
        let text = BTreeMap::from([(
            "enabled".to_owned(),
            CanonicalParameterValue::Text {
                value: "true".to_owned(),
            },
        )]);

        assert_ne!(
            config_hash("source", 2, &boolean).unwrap(),
            config_hash("source", 2, &text).unwrap()
        );
    }

    #[test]
    fn request_hash_changes_when_defaults_policy_changes() {
        let path_params = BTreeMap::from([
            ("did".to_owned(), "document".to_owned()),
            ("eid".to_owned(), "element".to_owned()),
        ]);
        let query = BTreeMap::<String, String>::new();
        let body = json!({"formatName": "STEP"});
        let first = RequestIdentity {
            api_host_class: "cad.onshape.com".to_owned(),
            api_spec_version: None,
            operation: "createExport".to_owned(),
            method: "POST".to_owned(),
            path_template: "/api/elements/d/{did}/e/{eid}/translations".to_owned(),
            path_params: path_params.clone(),
            query: query.clone(),
            body: body.clone(),
            encoded_configuration: Some(EncodedConfigurationIdentity {
                encoded_id: "encoded".to_owned(),
                query_param: "configuration=encoded".to_owned(),
            }),
            defaults_policy_version: "defaults-v1".to_owned(),
            request_builder_version: "builder-v1".to_owned(),
        };
        let second = RequestIdentity {
            defaults_policy_version: "defaults-v2".to_owned(),
            ..first.clone()
        };

        assert_ne!(
            request_hash(&first).unwrap(),
            request_hash(&second).unwrap()
        );
    }

    #[test]
    fn response_hash_changes_when_translation_id_changes() {
        let first = ResponseIdentity {
            translation_id: "translation-1".to_owned(),
            start_response: json!({"id": "translation-1"}),
            final_response: json!({"requestState": "DONE"}),
            poll_state: json!({"attempt": 3}),
            response_shape_version: RESPONSE_SHAPE_VERSION,
        };
        let second = ResponseIdentity {
            translation_id: "translation-2".to_owned(),
            start_response: json!({"id": "translation-2"}),
            ..first.clone()
        };

        assert_ne!(
            response_hash(&first).unwrap(),
            response_hash(&second).unwrap()
        );
    }

    #[test]
    fn response_hash_uses_canonical_response_v2_payload() {
        let first = ResponseIdentity {
            translation_id: "translation-1".to_owned(),
            start_response: json!({"b": 2, "a": 1}),
            final_response: json!({"requestState": "DONE", "resultExternalDataIds": ["fid"]}),
            poll_state: json!({"state": "DONE", "resultExternalDataIds": ["fid"]}),
            response_shape_version: RESPONSE_SHAPE_VERSION,
        };
        let second = ResponseIdentity {
            start_response: json!({"a": 1, "b": 2}),
            ..first.clone()
        };

        assert_eq!(
            response_hash(&first).unwrap(),
            response_hash(&second).unwrap()
        );
    }

    #[test]
    fn options_hash_excludes_exporter_package_version() {
        let options = json!({"resolution": "MEDIUM"});

        assert_eq!(
            options_hash("glb", "mesh-grouped-v2", &options).unwrap(),
            cache_key::hash_json(
                "options-v2",
                &json!({
                    "optionsVersion": "mesh-grouped-v2",
                    "format": "glb",
                    "options": {"resolution": "MEDIUM"}
                })
            )
            .unwrap()
        );
    }

    #[test]
    fn postprocess_hash_changes_when_policy_changes() {
        let first = PostprocessIdentity {
            raw_payload_hash: "raw".to_owned(),
            processor_name: "preview-extract".to_owned(),
            processor_version: "v1".to_owned(),
            policy: json!({"acceptedInputShapes": ["direct_glb"]}),
        };
        let second = PostprocessIdentity {
            policy: json!({"acceptedInputShapes": ["direct_glb", "zip_single_glb"]}),
            ..first.clone()
        };

        assert_ne!(
            postprocess_hash(&first).unwrap(),
            postprocess_hash(&second).unwrap()
        );
    }

    #[test]
    fn artifact_set_hash_uses_request_and_postprocess_identity() {
        let first = ArtifactSetIdentity {
            artifact_set_schema_version: ARTIFACT_SET_SCHEMA_VERSION,
            output_kind: "preview".to_owned(),
            format: "glb".to_owned(),
            source_hash: "source".to_owned(),
            config_hash: "config".to_owned(),
            options_hash: "options".to_owned(),
            request_hash: Some("request-1".to_owned()),
            raw_payload_hash: Some("raw".to_owned()),
            postprocess_hash: "postprocess".to_owned(),
            generator_processing_hash: None,
        };
        let second = ArtifactSetIdentity {
            request_hash: Some("request-2".to_owned()),
            ..first.clone()
        };

        assert_ne!(
            artifact_set_hash(&first).unwrap(),
            artifact_set_hash(&second).unwrap()
        );
        assert_eq!(
            artifact_set_hash(&first).unwrap(),
            cache_key::hash_json(
                "artifact-set-v2",
                &json!({
                    "artifactSetSchemaVersion": ARTIFACT_SET_SCHEMA_VERSION,
                    "outputKind": "preview",
                    "format": "glb",
                    "sourceHash": "source",
                    "configHash": "config",
                    "optionsHash": "options",
                    "requestHash": "request-1",
                    "rawPayloadHash": "raw",
                    "postprocessHash": "postprocess"
                })
            )
            .unwrap()
        );
    }

    #[test]
    fn multi_input_artifact_identity_omits_singular_request_and_payload_fields() {
        let identity = ArtifactSetIdentity {
            artifact_set_schema_version: ARTIFACT_SET_SCHEMA_VERSION,
            output_kind: "slicer_project".to_owned(),
            format: "project_3mf".to_owned(),
            source_hash: "source".to_owned(),
            config_hash: "config".to_owned(),
            options_hash: "settings".to_owned(),
            request_hash: None,
            raw_payload_hash: None,
            postprocess_hash: "recipe".to_owned(),
            generator_processing_hash: Some("recipe".to_owned()),
        };
        let serialized = serde_json::to_value(&identity).unwrap();

        assert!(serialized.get("requestHash").is_none());
        assert!(serialized.get("rawPayloadHash").is_none());
        assert_eq!(serialized["generatorProcessingHash"], "recipe");
    }
}
