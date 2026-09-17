use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use thiserror::Error;

use crate::cache_key;

pub const PROTOCOL_VERSION: u32 = 1;
pub const INPUT_MANIFEST_VERSION: u32 = 1;
pub const MAX_REQUEST_BYTES: usize = 1_048_576;
pub const MAX_INPUT_MANIFEST_BYTES: usize = 1_048_576;
pub const MAX_RESULT_BYTES: usize = 262_144;
pub const MAX_INPUT_OBJECTS: usize = 256;
pub const MAX_DIAGNOSTICS: usize = 64;
pub const MAX_ERRORS: usize = 64;

const MAX_IDENTITY_LENGTH: usize = 256;
const MAX_PATH_LENGTH: usize = 1_024;
const MAX_PATH_SEGMENTS: usize = 64;
const MAX_PATH_SEGMENT_LENGTH: usize = 255;
const MAX_DISPLAY_LENGTH: usize = 1_024;
const MAX_REASON_LENGTH: usize = 2_048;
const MAX_CODE_LENGTH: usize = 128;
const MAX_MESSAGE_LENGTH: usize = 2_048;
const MAX_CONTEXT_ENTRIES: usize = 16;
const MAX_CONTEXT_KEY_LENGTH: usize = 64;
const MAX_CONTEXT_VALUE_LENGTH: usize = 512;
const MAX_OCCURRENCE_SEGMENTS: usize = 64;
const MAX_SAFE_JSON_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("{document} exceeds its {limit}-byte limit")]
    DocumentTooLarge {
        document: &'static str,
        limit: usize,
    },
    #[error("invalid {document} JSON: {message}")]
    InvalidJson {
        document: &'static str,
        message: String,
    },
    #[error("{0}")]
    Invalid(String),
    #[error("could not compute protocol identity: {0}")]
    Identity(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequestDocumentType {
    #[serde(rename = "generatorRequest")]
    GeneratorRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ManifestDocumentType {
    #[serde(rename = "inputManifest")]
    InputManifest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResultDocumentType {
    #[serde(rename = "generatorResult")]
    GeneratorResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ManifestStatus {
    Available,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GroupingPolicy {
    Grouped,
    Individual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MappingStatus {
    Proven,
    Unproven,
    Missing,
    Duplicate,
    Ambiguous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InputRole {
    RawGeometry,
    AuxiliaryGeometry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OutputRole {
    GeneratedProject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResultStatus {
    Success,
    Failure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticSeverity {
    Info,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCategory {
    MalformedRequest,
    UnsupportedRequest,
    InvalidInput,
    GenerationFailed,
    InternalError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileContent {
    pub content_identity: String,
    pub path: String,
    pub sha256: String,
    pub byte_length: u64,
    pub media_type: String,
    pub detected_kind_identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MappingEvidence {
    pub classification: String,
    pub evidence_identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObjectMapping {
    pub status: MappingStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<MappingEvidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InputObject {
    pub object_identity: String,
    pub role: InputRole,
    pub retained_content: FileContent,
    pub mapping: ObjectMapping,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_object_identity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub occurrence_path: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub producer_result_identity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_object_identity: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportInput {
    pub kind_identity: String,
    pub schema_identity: String,
    pub grouping_policy: GroupingPolicy,
    pub observation_status: MappingStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observation_evidence_identity: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManifestDecision {
    pub status: ManifestStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InputManifest {
    pub document_type: ManifestDocumentType,
    pub protocol_version: u32,
    pub manifest_version: u32,
    pub manifest_identity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_set_identity: Option<String>,
    pub requirements_identity: String,
    pub source_identity: String,
    pub configuration_identity: String,
    pub export: ExportInput,
    pub decision: ManifestDecision,
    pub objects: Vec<InputObject>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdentityBindings {
    pub package_identity: String,
    pub build_identity: String,
    pub binary_identity: String,
    pub dialect_identity: String,
    pub capability_identities: Vec<String>,
    pub input_kind_identity: String,
    pub input_schema_identity: String,
    pub settings_identity: String,
    pub settings_schema_identity: String,
    pub provenance_set_identity: String,
    pub normalization_identity: String,
    pub validation_identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManifestReference {
    pub path: String,
    pub manifest_identity: String,
    pub input_set_identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsInput {
    pub settings_identity: String,
    pub schema_identity: String,
    pub content: FileContent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OutputDeclaration {
    pub output_identity: String,
    pub role: OutputRole,
    pub path: String,
    pub media_type: String,
    pub max_byte_length: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GeneratorRequest {
    pub document_type: RequestDocumentType,
    pub protocol_version: u32,
    pub invocation_identity: String,
    pub expected_identities: IdentityBindings,
    pub input_manifest: ManifestReference,
    pub settings: SettingsInput,
    pub output: OutputDeclaration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiagnosticContext {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_path: Option<String>,
    pub context: Vec<DiagnosticContext>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StructuredError {
    pub category: ErrorCategory,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_path: Option<String>,
    pub context: Vec<DiagnosticContext>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GeneratedOutput {
    pub output_identity: String,
    pub role: OutputRole,
    pub path: String,
    pub media_type: String,
    pub byte_length: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GeneratorResult {
    pub document_type: ResultDocumentType,
    pub protocol_version: u32,
    pub status: ResultStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invocation_identity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reported_identities: Option<IdentityBindings>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<GeneratedOutput>,
    pub diagnostics: Vec<Diagnostic>,
    pub errors: Vec<StructuredError>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InputSetIdentityPayload<'a> {
    protocol_version: u32,
    manifest_version: u32,
    objects: &'a [InputObject],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestIdentityPayload<'a> {
    document_type: ManifestDocumentType,
    protocol_version: u32,
    manifest_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    input_set_identity: &'a Option<String>,
    requirements_identity: &'a str,
    source_identity: &'a str,
    configuration_identity: &'a str,
    export: &'a ExportInput,
    decision: &'a ManifestDecision,
    objects: &'a [InputObject],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InvocationIdentityPayload<'a> {
    document_type: RequestDocumentType,
    protocol_version: u32,
    expected_identities: &'a IdentityBindings,
    input_manifest: &'a ManifestReference,
    settings: &'a SettingsInput,
    output: &'a OutputDeclaration,
}

impl FileContent {
    fn validate(&self, field: &str) -> Result<(), ProtocolError> {
        validate_identity(&self.content_identity, &format!("{field}.contentIdentity"))?;
        validate_relative_path(&self.path, &format!("{field}.path"))?;
        validate_sha256(&self.sha256, &format!("{field}.sha256"))?;
        validate_byte_length(self.byte_length, &format!("{field}.byteLength"))?;
        validate_media_type(&self.media_type, &format!("{field}.mediaType"))?;
        validate_identity(
            &self.detected_kind_identity,
            &format!("{field}.detectedKindIdentity"),
        )
    }
}

impl ObjectMapping {
    fn validate(&self, field: &str) -> Result<(), ProtocolError> {
        match self.status {
            MappingStatus::Proven => {
                ensure(
                    self.evidence.is_some(),
                    format!("{field}.evidence is required for a proven mapping"),
                )?;
                ensure(
                    self.reason.is_none(),
                    format!("{field}.reason is not allowed for a proven mapping"),
                )?;
            }
            _ => {
                ensure(
                    self.reason.is_some(),
                    format!("{field}.reason is required when mapping is not proven"),
                )?;
            }
        }
        if let Some(evidence) = &self.evidence {
            validate_identity(
                &evidence.classification,
                &format!("{field}.evidence.classification"),
            )?;
            validate_identity(
                &evidence.evidence_identity,
                &format!("{field}.evidence.evidenceIdentity"),
            )?;
        }
        if let Some(reason) = &self.reason {
            validate_text(reason, MAX_REASON_LENGTH, &format!("{field}.reason"))?;
        }
        Ok(())
    }
}

impl InputManifest {
    pub fn computed_input_set_identity(&self) -> Result<Option<String>, ProtocolError> {
        if self.decision.status == ManifestStatus::Unavailable {
            return Ok(None);
        }
        cache_key::hash_json(
            "generator-input-set-v1",
            &InputSetIdentityPayload {
                protocol_version: self.protocol_version,
                manifest_version: self.manifest_version,
                objects: &self.objects,
            },
        )
        .map(Some)
        .map_err(|error| ProtocolError::Identity(error.to_string()))
    }

    pub fn computed_manifest_identity(&self) -> Result<String, ProtocolError> {
        cache_key::hash_json(
            "generator-input-manifest-v1",
            &ManifestIdentityPayload {
                document_type: self.document_type,
                protocol_version: self.protocol_version,
                manifest_version: self.manifest_version,
                input_set_identity: &self.input_set_identity,
                requirements_identity: &self.requirements_identity,
                source_identity: &self.source_identity,
                configuration_identity: &self.configuration_identity,
                export: &self.export,
                decision: &self.decision,
                objects: &self.objects,
            },
        )
        .map_err(|error| ProtocolError::Identity(error.to_string()))
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_version(self.protocol_version, "input manifest protocolVersion")?;
        ensure(
            self.manifest_version == INPUT_MANIFEST_VERSION,
            format!(
                "unsupported input manifest version: {}",
                self.manifest_version
            ),
        )?;
        validate_sha256(&self.manifest_identity, "manifestIdentity")?;
        validate_identity(&self.requirements_identity, "requirementsIdentity")?;
        validate_identity(&self.source_identity, "sourceIdentity")?;
        validate_identity(&self.configuration_identity, "configurationIdentity")?;
        validate_identity(&self.export.kind_identity, "export.kindIdentity")?;
        validate_identity(&self.export.schema_identity, "export.schemaIdentity")?;

        match self.export.observation_status {
            MappingStatus::Proven => ensure(
                self.export.observation_evidence_identity.is_some(),
                "export.observationEvidenceIdentity is required when observationStatus is proven",
            )?,
            _ => ensure(
                self.export.observation_evidence_identity.is_none(),
                "export.observationEvidenceIdentity is only allowed when observationStatus is proven",
            )?,
        }
        if let Some(identity) = &self.export.observation_evidence_identity {
            validate_identity(identity, "export.observationEvidenceIdentity")?;
        }

        ensure(
            self.objects.len() <= MAX_INPUT_OBJECTS,
            format!("input manifest exceeds {MAX_INPUT_OBJECTS} objects"),
        )?;
        match self.decision.status {
            ManifestStatus::Available => {
                ensure(
                    self.decision.reason.is_none(),
                    "decision.reason is not allowed for an available manifest",
                )?;
                ensure(
                    !self.objects.is_empty(),
                    "an available manifest must contain at least one object",
                )?;
                ensure(
                    self.export.observation_status == MappingStatus::Proven,
                    "an available manifest requires a proven export observation",
                )?;
                ensure(
                    self.input_set_identity.is_some(),
                    "an available manifest requires inputSetIdentity",
                )?;
            }
            ManifestStatus::Unavailable => {
                ensure(
                    self.decision.reason.is_some(),
                    "an unavailable manifest requires decision.reason",
                )?;
                ensure(
                    self.input_set_identity.is_none(),
                    "an unavailable manifest must not declare inputSetIdentity",
                )?;
            }
        }
        if let Some(reason) = &self.decision.reason {
            validate_text(reason, MAX_REASON_LENGTH, "decision.reason")?;
        }

        let mut identities = HashSet::new();
        let mut paths = HashSet::new();
        let mut by_identity = HashMap::new();
        for (index, object) in self.objects.iter().enumerate() {
            let field = format!("objects[{index}]");
            validate_identity(&object.object_identity, &format!("{field}.objectIdentity"))?;
            ensure(
                identities.insert(object.object_identity.as_str()),
                format!(
                    "duplicate input object identity: {}",
                    object.object_identity
                ),
            )?;
            object
                .retained_content
                .validate(&format!("{field}.retainedContent"))?;
            ensure(
                is_under_directory(&object.retained_content.path, "inputs"),
                format!("{field}.retainedContent.path must be beneath inputs/"),
            )?;
            ensure(
                paths.insert(object.retained_content.path.as_str()),
                format!(
                    "duplicate retained-content path: {}",
                    object.retained_content.path
                ),
            )?;
            object.mapping.validate(&format!("{field}.mapping"))?;
            if self.decision.status == ManifestStatus::Available {
                ensure(
                    object.mapping.status == MappingStatus::Proven,
                    format!("{field} is not proven and cannot be used by an invocation"),
                )?;
            }
            validate_optional_identity(
                object.source_object_identity.as_deref(),
                &format!("{field}.sourceObjectIdentity"),
            )?;
            validate_optional_identity(
                object.producer_result_identity.as_deref(),
                &format!("{field}.producerResultIdentity"),
            )?;
            validate_optional_identity(
                object.parent_object_identity.as_deref(),
                &format!("{field}.parentObjectIdentity"),
            )?;
            validate_optional_display(
                object.source_filename.as_deref(),
                &format!("{field}.sourceFilename"),
            )?;
            validate_optional_display(
                object.display_name.as_deref(),
                &format!("{field}.displayName"),
            )?;
            if let Some(occurrence_path) = &object.occurrence_path {
                ensure(
                    !occurrence_path.is_empty() && occurrence_path.len() <= MAX_OCCURRENCE_SEGMENTS,
                    format!(
                        "{field}.occurrencePath must contain 1 to {MAX_OCCURRENCE_SEGMENTS} segments"
                    ),
                )?;
                for (segment_index, segment) in occurrence_path.iter().enumerate() {
                    validate_identity(
                        segment,
                        &format!("{field}.occurrencePath[{segment_index}]"),
                    )?;
                }
            }
            by_identity.insert(object.object_identity.as_str(), object);
        }

        ensure(
            self.decision.status != ManifestStatus::Available
                || self
                    .objects
                    .iter()
                    .any(|object| object.role == InputRole::RawGeometry),
            "an available manifest requires at least one rawGeometry object",
        )?;

        for object in &self.objects {
            if let Some(parent) = object.parent_object_identity.as_deref() {
                ensure(
                    by_identity.contains_key(parent),
                    format!(
                        "input object {} references missing parent {parent}",
                        object.object_identity
                    ),
                )?;
            }
            let mut visited = HashSet::new();
            let mut current = Some(object.object_identity.as_str());
            while let Some(identity) = current {
                ensure(
                    visited.insert(identity),
                    format!("input object parent cycle includes {identity}"),
                )?;
                current = by_identity
                    .get(identity)
                    .and_then(|candidate| candidate.parent_object_identity.as_deref());
            }
        }

        let computed_input_set_identity = self.computed_input_set_identity()?;
        ensure(
            self.input_set_identity == computed_input_set_identity,
            "inputSetIdentity does not match the ordered input objects",
        )?;
        ensure(
            self.manifest_identity == self.computed_manifest_identity()?,
            "manifestIdentity does not match the input manifest",
        )
    }
}

impl IdentityBindings {
    fn validate(&self, field: &str) -> Result<(), ProtocolError> {
        for (name, value) in [
            ("packageIdentity", &self.package_identity),
            ("buildIdentity", &self.build_identity),
            ("binaryIdentity", &self.binary_identity),
            ("dialectIdentity", &self.dialect_identity),
            ("inputKindIdentity", &self.input_kind_identity),
            ("inputSchemaIdentity", &self.input_schema_identity),
            ("settingsIdentity", &self.settings_identity),
            ("settingsSchemaIdentity", &self.settings_schema_identity),
            ("provenanceSetIdentity", &self.provenance_set_identity),
            ("normalizationIdentity", &self.normalization_identity),
            ("validationIdentity", &self.validation_identity),
        ] {
            validate_identity(value, &format!("{field}.{name}"))?;
        }
        ensure(
            self.capability_identities.len() <= MAX_INPUT_OBJECTS,
            format!("{field}.capabilityIdentities exceeds {MAX_INPUT_OBJECTS} entries"),
        )?;
        for (index, identity) in self.capability_identities.iter().enumerate() {
            validate_identity(identity, &format!("{field}.capabilityIdentities[{index}]"))?;
        }
        ensure(
            self.capability_identities
                .windows(2)
                .all(|pair| pair[0] < pair[1]),
            format!("{field}.capabilityIdentities must be unique and sorted lexicographically"),
        )
    }
}

impl GeneratorRequest {
    pub fn computed_invocation_identity(&self) -> Result<String, ProtocolError> {
        cache_key::hash_json(
            "generator-invocation-v1",
            &InvocationIdentityPayload {
                document_type: self.document_type,
                protocol_version: self.protocol_version,
                expected_identities: &self.expected_identities,
                input_manifest: &self.input_manifest,
                settings: &self.settings,
                output: &self.output,
            },
        )
        .map_err(|error| ProtocolError::Identity(error.to_string()))
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_version(self.protocol_version, "generator request protocolVersion")?;
        validate_sha256(&self.invocation_identity, "invocationIdentity")?;
        self.expected_identities.validate("expectedIdentities")?;
        validate_relative_path(&self.input_manifest.path, "inputManifest.path")?;
        ensure(
            is_under_directory(&self.input_manifest.path, "inputs"),
            "inputManifest.path must be beneath inputs/",
        )?;
        validate_sha256(
            &self.input_manifest.manifest_identity,
            "inputManifest.manifestIdentity",
        )?;
        validate_sha256(
            &self.input_manifest.input_set_identity,
            "inputManifest.inputSetIdentity",
        )?;
        validate_identity(
            &self.settings.settings_identity,
            "settings.settingsIdentity",
        )?;
        validate_identity(&self.settings.schema_identity, "settings.schemaIdentity")?;
        self.settings.content.validate("settings.content")?;
        ensure(
            is_under_directory(&self.settings.content.path, "inputs"),
            "settings.content.path must be beneath inputs/",
        )?;
        ensure(
            self.settings.content.path != self.input_manifest.path,
            "settings.content.path must differ from inputManifest.path",
        )?;
        validate_identity(&self.output.output_identity, "output.outputIdentity")?;
        validate_relative_path(&self.output.path, "output.path")?;
        ensure(
            is_under_directory(&self.output.path, "outputs"),
            "output.path must be beneath outputs/",
        )?;
        validate_media_type(&self.output.media_type, "output.mediaType")?;
        validate_byte_length(self.output.max_byte_length, "output.maxByteLength")?;

        ensure(
            self.expected_identities.settings_identity == self.settings.settings_identity,
            "expectedIdentities.settingsIdentity does not match settings.settingsIdentity",
        )?;
        ensure(
            self.expected_identities.settings_schema_identity == self.settings.schema_identity,
            "expectedIdentities.settingsSchemaIdentity does not match settings.schemaIdentity",
        )?;
        ensure(
            self.invocation_identity == self.computed_invocation_identity()?,
            "invocationIdentity does not match the generator request",
        )
    }

    pub fn validate_with_manifest(&self, manifest: &InputManifest) -> Result<(), ProtocolError> {
        self.validate()?;
        manifest.validate()?;
        ensure(
            manifest.decision.status == ManifestStatus::Available,
            "an unavailable input manifest cannot be invoked",
        )?;
        ensure(
            self.input_manifest.manifest_identity == manifest.manifest_identity,
            "request manifestIdentity does not match the input manifest",
        )?;
        ensure(
            manifest
                .objects
                .iter()
                .all(|object| object.retained_content.path != self.input_manifest.path),
            "inputManifest.path must differ from every retained-content path",
        )?;
        ensure(
            manifest.input_set_identity.as_deref()
                == Some(self.input_manifest.input_set_identity.as_str()),
            "request inputSetIdentity does not match the input manifest",
        )?;
        ensure(
            self.expected_identities.input_kind_identity == manifest.export.kind_identity,
            "expected inputKindIdentity does not match the input manifest",
        )?;
        ensure(
            self.expected_identities.input_schema_identity == manifest.export.schema_identity,
            "expected inputSchemaIdentity does not match the input manifest",
        )?;
        ensure(
            manifest
                .objects
                .iter()
                .all(|object| object.retained_content.path != self.settings.content.path),
            "settings.content.path must differ from every retained-content path",
        )
    }
}

impl GeneratorResult {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_version(self.protocol_version, "generator result protocolVersion")?;
        if let Some(identity) = &self.invocation_identity {
            validate_sha256(identity, "invocationIdentity")?;
        }
        if let Some(identities) = &self.reported_identities {
            identities.validate("reportedIdentities")?;
        }
        ensure(
            self.diagnostics.len() <= MAX_DIAGNOSTICS,
            format!("diagnostics exceeds {MAX_DIAGNOSTICS} entries"),
        )?;
        ensure(
            self.errors.len() <= MAX_ERRORS,
            format!("errors exceeds {MAX_ERRORS} entries"),
        )?;
        for (index, diagnostic) in self.diagnostics.iter().enumerate() {
            validate_report_entry(
                &diagnostic.code,
                &diagnostic.message,
                diagnostic.instance_path.as_deref(),
                &diagnostic.context,
                &format!("diagnostics[{index}]"),
            )?;
        }
        for (index, error) in self.errors.iter().enumerate() {
            validate_report_entry(
                &error.code,
                &error.message,
                error.instance_path.as_deref(),
                &error.context,
                &format!("errors[{index}]"),
            )?;
        }
        if let Some(output) = &self.output {
            validate_identity(&output.output_identity, "output.outputIdentity")?;
            validate_relative_path(&output.path, "output.path")?;
            ensure(
                is_under_directory(&output.path, "outputs"),
                "output.path must be beneath outputs/",
            )?;
            validate_media_type(&output.media_type, "output.mediaType")?;
            validate_byte_length(output.byte_length, "output.byteLength")?;
            validate_sha256(&output.sha256, "output.sha256")?;
        }

        match self.status {
            ResultStatus::Success => {
                ensure(
                    self.invocation_identity.is_some(),
                    "a success result requires invocationIdentity",
                )?;
                ensure(
                    self.reported_identities.is_some(),
                    "a success result requires reportedIdentities",
                )?;
                ensure(self.output.is_some(), "a success result requires output")?;
                ensure(
                    self.errors.is_empty(),
                    "a success result must not contain errors",
                )?;
            }
            ResultStatus::Failure => {
                ensure(
                    self.output.is_none(),
                    "a failure result must not contain output",
                )?;
                ensure(
                    !self.errors.is_empty(),
                    "a failure result requires at least one structured error",
                )?;
            }
        }
        Ok(())
    }

    pub fn validate_against_request(
        &self,
        request: &GeneratorRequest,
    ) -> Result<(), ProtocolError> {
        request.validate()?;
        self.validate()?;
        if let Some(identity) = &self.invocation_identity {
            ensure(
                identity == &request.invocation_identity,
                "result invocationIdentity does not match the request",
            )?;
        }
        if let Some(identities) = &self.reported_identities {
            ensure(
                identities == &request.expected_identities,
                "result identities do not match the request",
            )?;
        }
        if self.status == ResultStatus::Success {
            let output = self.output.as_ref().expect("validated success output");
            ensure(
                output.output_identity == request.output.output_identity
                    && output.role == request.output.role
                    && output.path == request.output.path
                    && output.media_type == request.output.media_type,
                "success result output declaration does not match the request",
            )?;
            ensure(
                output.byte_length <= request.output.max_byte_length,
                "success result output exceeds the request byte limit",
            )?;
        }
        Ok(())
    }
}

pub fn parse_request(bytes: &[u8]) -> Result<GeneratorRequest, ProtocolError> {
    let request: GeneratorRequest = parse_json(bytes, MAX_REQUEST_BYTES, "generator request")?;
    request.validate()?;
    Ok(request)
}

pub fn parse_input_manifest(bytes: &[u8]) -> Result<InputManifest, ProtocolError> {
    let manifest: InputManifest = parse_json(bytes, MAX_INPUT_MANIFEST_BYTES, "input manifest")?;
    manifest.validate()?;
    Ok(manifest)
}

pub fn parse_result(bytes: &[u8]) -> Result<GeneratorResult, ProtocolError> {
    let result: GeneratorResult = parse_json(bytes, MAX_RESULT_BYTES, "generator result")?;
    result.validate()?;
    Ok(result)
}

fn parse_json<T>(bytes: &[u8], limit: usize, document: &'static str) -> Result<T, ProtocolError>
where
    T: DeserializeOwned,
{
    if bytes.len() > limit {
        return Err(ProtocolError::DocumentTooLarge { document, limit });
    }
    serde_json::from_slice(bytes).map_err(|error| ProtocolError::InvalidJson {
        document,
        message: error.to_string(),
    })
}

fn validate_version(version: u32, field: &str) -> Result<(), ProtocolError> {
    ensure(
        version == PROTOCOL_VERSION,
        format!("unsupported {field}: {version}"),
    )
}

fn validate_identity(value: &str, field: &str) -> Result<(), ProtocolError> {
    ensure(
        !value.is_empty()
            && value.len() <= MAX_IDENTITY_LENGTH
            && value.bytes().all(|byte| byte.is_ascii_graphic()),
        format!("{field} must contain 1 to {MAX_IDENTITY_LENGTH} visible ASCII characters"),
    )
}

fn validate_optional_identity(value: Option<&str>, field: &str) -> Result<(), ProtocolError> {
    if let Some(value) = value {
        validate_identity(value, field)?;
    }
    Ok(())
}

fn validate_sha256(value: &str, field: &str) -> Result<(), ProtocolError> {
    ensure(
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        format!("{field} must be a lowercase 64-character SHA-256 value"),
    )
}

fn validate_byte_length(value: u64, field: &str) -> Result<(), ProtocolError> {
    ensure(
        value > 0 && value <= MAX_SAFE_JSON_INTEGER,
        format!("{field} must be between 1 and {MAX_SAFE_JSON_INTEGER}"),
    )
}

fn validate_media_type(value: &str, field: &str) -> Result<(), ProtocolError> {
    let mut parts = value.split('/');
    let type_name = parts.next().unwrap_or_default();
    let subtype = parts.next().unwrap_or_default();
    ensure(
        !type_name.is_empty()
            && !subtype.is_empty()
            && parts.next().is_none()
            && type_name.bytes().all(is_media_type_character)
            && subtype.bytes().all(is_media_type_character),
        format!("{field} must be a media type without parameters"),
    )
}

fn is_media_type_character(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-'
        )
}

fn validate_relative_path(value: &str, field: &str) -> Result<(), ProtocolError> {
    ensure(
        !value.is_empty() && value.len() <= MAX_PATH_LENGTH && value.is_ascii(),
        format!("{field} must be a non-empty ASCII path of at most {MAX_PATH_LENGTH} bytes"),
    )?;
    let segments: Vec<_> = value.split('/').collect();
    ensure(
        segments.len() <= MAX_PATH_SEGMENTS,
        format!("{field} exceeds {MAX_PATH_SEGMENTS} path segments"),
    )?;
    for segment in segments {
        ensure(
            !segment.is_empty()
                && segment.len() <= MAX_PATH_SEGMENT_LENGTH
                && segment
                    .bytes()
                    .next()
                    .is_some_and(|byte| byte.is_ascii_alphanumeric())
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')),
            format!(
                "{field} contains an invalid path segment; use ASCII letters, numbers, dot, underscore, or hyphen"
            ),
        )?;
    }
    Ok(())
}

fn is_under_directory(path: &str, directory: &str) -> bool {
    path.strip_prefix(directory)
        .is_some_and(|suffix| suffix.starts_with('/') && suffix.len() > 1)
}

fn validate_optional_display(value: Option<&str>, field: &str) -> Result<(), ProtocolError> {
    if let Some(value) = value {
        validate_text(value, MAX_DISPLAY_LENGTH, field)?;
    }
    Ok(())
}

fn validate_text(value: &str, limit: usize, field: &str) -> Result<(), ProtocolError> {
    ensure(
        !value.is_empty()
            && value.chars().count() <= limit
            && value.chars().all(|character| !character.is_control()),
        format!("{field} must contain 1 to {limit} non-control characters"),
    )
}

fn validate_report_entry(
    code: &str,
    message: &str,
    instance_path: Option<&str>,
    context: &[DiagnosticContext],
    field: &str,
) -> Result<(), ProtocolError> {
    ensure(
        !code.is_empty()
            && code.len() <= MAX_CODE_LENGTH
            && code.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
            }),
        format!("{field}.code must contain 1 to {MAX_CODE_LENGTH} lowercase ASCII code characters"),
    )?;
    validate_text(message, MAX_MESSAGE_LENGTH, &format!("{field}.message"))?;
    if let Some(instance_path) = instance_path {
        validate_json_pointer(instance_path, &format!("{field}.instancePath"))?;
    }
    ensure(
        context.len() <= MAX_CONTEXT_ENTRIES,
        format!("{field}.context exceeds {MAX_CONTEXT_ENTRIES} entries"),
    )?;
    let mut keys = HashSet::new();
    for (index, item) in context.iter().enumerate() {
        validate_identity_with_limit(
            &item.key,
            MAX_CONTEXT_KEY_LENGTH,
            &format!("{field}.context[{index}].key"),
        )?;
        validate_text(
            &item.value,
            MAX_CONTEXT_VALUE_LENGTH,
            &format!("{field}.context[{index}].value"),
        )?;
        ensure(
            keys.insert(item.key.as_str()),
            format!("{field}.context contains duplicate key {}", item.key),
        )?;
    }
    Ok(())
}

fn validate_identity_with_limit(
    value: &str,
    limit: usize,
    field: &str,
) -> Result<(), ProtocolError> {
    ensure(
        !value.is_empty()
            && value.len() <= limit
            && value.bytes().all(|byte| byte.is_ascii_graphic()),
        format!("{field} must contain 1 to {limit} visible ASCII characters"),
    )
}

fn validate_json_pointer(value: &str, field: &str) -> Result<(), ProtocolError> {
    ensure(
        value.len() <= MAX_PATH_LENGTH && (value.is_empty() || value.starts_with('/')),
        format!("{field} must be an empty JSON Pointer or start with slash"),
    )?;
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        ensure(
            !bytes[index].is_ascii_control(),
            format!("{field} must not contain control characters"),
        )?;
        if bytes[index] == b'~' {
            ensure(
                bytes
                    .get(index + 1)
                    .is_some_and(|byte| matches!(byte, b'0' | b'1')),
                format!("{field} contains an invalid JSON Pointer escape"),
            )?;
            index += 1;
        }
        index += 1;
    }
    Ok(())
}

fn ensure(condition: bool, message: impl Into<String>) -> Result<(), ProtocolError> {
    if condition {
        Ok(())
    } else {
        Err(ProtocolError::Invalid(message.into()))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    const SCHEMA: &str = include_str!("../protocol/generator/v1/generator-protocol.schema.json");
    const MANIFEST: &str = include_str!("../protocol/generator/v1/examples/input-manifest.json");
    const REQUEST: &str = include_str!("../protocol/generator/v1/examples/request.json");
    const SUCCESS_RESULT: &str =
        include_str!("../protocol/generator/v1/examples/success-result.json");
    const FAILURE_RESULT: &str =
        include_str!("../protocol/generator/v1/examples/failure-result.json");

    fn value(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    fn schema_accepts(instance: &Value) -> bool {
        jsonschema::draft202012::is_valid(&value(SCHEMA), instance)
    }

    #[test]
    fn schema_is_valid_draft_2020_12() {
        assert!(jsonschema::draft202012::meta::is_valid(&value(SCHEMA)));
    }

    #[test]
    fn schema_accepts_all_synthetic_examples() {
        for (name, example) in [
            ("input manifest", MANIFEST),
            ("request", REQUEST),
            ("success result", SUCCESS_RESULT),
            ("failure result", FAILURE_RESULT),
        ] {
            assert!(schema_accepts(&value(example)), "invalid {name} example");
        }
    }

    #[test]
    fn parses_and_cross_validates_success_examples() {
        let manifest = parse_input_manifest(MANIFEST.as_bytes()).unwrap();
        let request = parse_request(REQUEST.as_bytes()).unwrap();
        let result = parse_result(SUCCESS_RESULT.as_bytes()).unwrap();

        request.validate_with_manifest(&manifest).unwrap();
        result.validate_against_request(&request).unwrap();
        assert!(schema_accepts(&serde_json::to_value(manifest).unwrap()));
        assert!(schema_accepts(&serde_json::to_value(request).unwrap()));
        assert!(schema_accepts(&serde_json::to_value(result).unwrap()));
    }

    #[test]
    fn parses_structured_failure_without_request_identity() {
        let result = parse_result(FAILURE_RESULT.as_bytes()).unwrap();
        assert_eq!(result.status, ResultStatus::Failure);
        assert!(result.invocation_identity.is_none());
        assert!(result.output.is_none());
    }

    #[test]
    fn rejects_unknown_fields_and_unsupported_versions() {
        let mut request = value(REQUEST);
        request["unknown"] = json!(true);
        assert!(!schema_accepts(&request));
        assert!(parse_request(serde_json::to_vec(&request).unwrap().as_slice()).is_err());

        let mut request = value(REQUEST);
        request["protocolVersion"] = json!(2);
        assert!(!schema_accepts(&request));
        assert!(parse_request(serde_json::to_vec(&request).unwrap().as_slice()).is_err());
    }

    #[test]
    fn schema_rejects_invalid_manifest_and_result_states() {
        let mut manifest = value(MANIFEST);
        manifest["decision"]["reason"] = json!("not allowed when available");
        assert!(!schema_accepts(&manifest));

        let mut manifest = value(MANIFEST);
        manifest["objects"][0]["mapping"]["status"] = json!("ambiguous");
        manifest["objects"][0]["mapping"]
            .as_object_mut()
            .unwrap()
            .remove("evidence");
        manifest["objects"][0]["mapping"]["reason"] = json!("synthetic ambiguity");
        assert!(!schema_accepts(&manifest));

        let mut result = value(SUCCESS_RESULT);
        result["errors"] = json!([{
            "category": "generationFailed",
            "code": "synthetic_failure",
            "message": "Synthetic failure.",
            "context": []
        }]);
        assert!(!schema_accepts(&result));

        let mut result = value(FAILURE_RESULT);
        result["output"] = value(SUCCESS_RESULT)["output"].clone();
        assert!(!schema_accepts(&result));
    }

    #[test]
    fn rejects_duplicate_json_members() {
        let duplicate = REQUEST.replacen(
            "\"protocolVersion\": 1,",
            "\"protocolVersion\": 1,\n  \"protocolVersion\": 1,",
            1,
        );
        assert!(parse_request(duplicate.as_bytes()).is_err());
    }

    #[test]
    fn rejects_unsafe_paths() {
        let mut request: GeneratorRequest = serde_json::from_str(REQUEST).unwrap();
        request.output.path = "../candidate.bin".to_owned();
        assert!(request.validate().is_err());
        request.output.path = "outputs\\candidate.bin".to_owned();
        assert!(request.validate().is_err());
        request.output.path = "/outputs/candidate.bin".to_owned();
        assert!(request.validate().is_err());
    }

    #[test]
    fn rejects_duplicate_and_cyclic_manifest_objects() {
        let mut manifest: InputManifest = serde_json::from_str(MANIFEST).unwrap();
        let mut second = manifest.objects[0].clone();
        second.retained_content.path = "inputs/second.bin".to_owned();
        manifest.objects.push(second);
        assert!(manifest.validate().is_err());

        let mut manifest: InputManifest = serde_json::from_str(MANIFEST).unwrap();
        manifest.objects[0].parent_object_identity =
            Some(manifest.objects[0].object_identity.clone());
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn retained_paths_distinguish_logical_occurrences_with_equal_content() {
        let mut manifest: InputManifest = serde_json::from_str(MANIFEST).unwrap();
        let mut second = manifest.objects[0].clone();
        second.object_identity = "object-synthetic-002".to_owned();
        second.retained_content.path = "inputs/geometry-002.bin".to_owned();
        assert_eq!(
            second.retained_content.content_identity,
            manifest.objects[0].retained_content.content_identity
        );
        assert_eq!(
            second.retained_content.sha256,
            manifest.objects[0].retained_content.sha256
        );
        assert_eq!(
            second.retained_content.byte_length,
            manifest.objects[0].retained_content.byte_length
        );
        manifest.objects.push(second);
        manifest.input_set_identity = manifest.computed_input_set_identity().unwrap();
        manifest.manifest_identity = manifest.computed_manifest_identity().unwrap();
        manifest.validate().unwrap();

        manifest.objects[1].retained_content.path =
            manifest.objects[0].retained_content.path.clone();
        manifest.input_set_identity = manifest.computed_input_set_identity().unwrap();
        manifest.manifest_identity = manifest.computed_manifest_identity().unwrap();
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn unavailable_or_unproven_manifest_cannot_be_invoked() {
        let request: GeneratorRequest = serde_json::from_str(REQUEST).unwrap();
        let mut manifest: InputManifest = serde_json::from_str(MANIFEST).unwrap();
        manifest.decision = ManifestDecision {
            status: ManifestStatus::Unavailable,
            reason: Some("synthetic mapping unavailable".to_owned()),
        };
        manifest.input_set_identity = None;
        manifest.objects[0].mapping = ObjectMapping {
            status: MappingStatus::Ambiguous,
            evidence: None,
            reason: Some("synthetic relation is ambiguous".to_owned()),
        };
        manifest.manifest_identity = manifest.computed_manifest_identity().unwrap();

        manifest.validate().unwrap();
        assert!(request.validate_with_manifest(&manifest).is_err());
    }

    #[test]
    fn unavailable_manifest_identity_omits_absent_input_set() {
        let mut manifest: InputManifest = serde_json::from_str(MANIFEST).unwrap();
        manifest.export.observation_status = MappingStatus::Ambiguous;
        manifest.export.observation_evidence_identity = None;
        manifest.decision = ManifestDecision {
            status: ManifestStatus::Unavailable,
            reason: Some("synthetic mapping unavailable".to_owned()),
        };
        manifest.input_set_identity = None;
        manifest.objects[0].mapping = ObjectMapping {
            status: MappingStatus::Ambiguous,
            evidence: None,
            reason: Some("synthetic relation is ambiguous".to_owned()),
        };
        manifest.manifest_identity = manifest.computed_manifest_identity().unwrap();

        manifest.validate().unwrap();
        let mut payload = serde_json::to_value(&manifest).unwrap();
        let object = payload.as_object_mut().unwrap();
        object.remove("manifestIdentity");
        assert!(!object.contains_key("inputSetIdentity"));
        assert_eq!(
            manifest.manifest_identity,
            crate::cache_key::hash_json("generator-input-manifest-v1", &payload).unwrap()
        );
        assert!(schema_accepts(&serde_json::to_value(manifest).unwrap()));
    }

    #[test]
    fn rejects_cross_document_path_and_failure_identity_mismatches() {
        let manifest: InputManifest = serde_json::from_str(MANIFEST).unwrap();
        let mut request: GeneratorRequest = serde_json::from_str(REQUEST).unwrap();
        request.input_manifest.path = manifest.objects[0].retained_content.path.clone();
        request.invocation_identity = request.computed_invocation_identity().unwrap();
        request.validate().unwrap();
        assert!(request.validate_with_manifest(&manifest).is_err());

        let request: GeneratorRequest = serde_json::from_str(REQUEST).unwrap();
        let mut result: GeneratorResult = serde_json::from_str(FAILURE_RESULT).unwrap();
        result.invocation_identity = Some(request.invocation_identity.clone());
        result.reported_identities = Some(request.expected_identities.clone());
        result
            .reported_identities
            .as_mut()
            .unwrap()
            .package_identity = "package-synthetic-other".to_owned();
        assert!(result.validate_against_request(&request).is_err());
    }

    #[test]
    fn input_set_identity_changes_with_order_role_content_and_name() {
        let manifest: InputManifest = serde_json::from_str(MANIFEST).unwrap();
        let original = manifest.computed_input_set_identity().unwrap().unwrap();

        let mut changed = manifest.clone();
        changed.objects[0].role = InputRole::AuxiliaryGeometry;
        assert_ne!(
            original,
            changed.computed_input_set_identity().unwrap().unwrap()
        );

        let mut changed = manifest.clone();
        changed.objects[0].retained_content.sha256 = "d".repeat(64);
        assert_ne!(
            original,
            changed.computed_input_set_identity().unwrap().unwrap()
        );

        let mut changed = manifest.clone();
        changed.objects[0].display_name = Some("Changed synthetic name".to_owned());
        assert_ne!(
            original,
            changed.computed_input_set_identity().unwrap().unwrap()
        );

        let mut changed = manifest.clone();
        let mut second = changed.objects[0].clone();
        second.object_identity = "object-synthetic-002".to_owned();
        second.retained_content.path = "inputs/geometry-002.bin".to_owned();
        changed.objects.push(second);
        let forward = changed.computed_input_set_identity().unwrap().unwrap();
        changed.objects.reverse();
        assert_ne!(
            forward,
            changed.computed_input_set_identity().unwrap().unwrap()
        );
    }

    #[test]
    fn invocation_identity_changes_with_settings_and_capabilities() {
        let request: GeneratorRequest = serde_json::from_str(REQUEST).unwrap();
        let original = request.computed_invocation_identity().unwrap();

        let mut changed = request.clone();
        changed.settings.settings_identity = "settings-synthetic-002".to_owned();
        changed.expected_identities.settings_identity = changed.settings.settings_identity.clone();
        assert_ne!(original, changed.computed_invocation_identity().unwrap());

        let mut changed = request.clone();
        changed
            .expected_identities
            .capability_identities
            .push("capability-synthetic-beta-v1".to_owned());
        assert_ne!(original, changed.computed_invocation_identity().unwrap());
    }

    #[test]
    fn rejects_unsorted_capabilities_and_result_mismatches() {
        let mut request: GeneratorRequest = serde_json::from_str(REQUEST).unwrap();
        request.expected_identities.capability_identities = vec![
            "capability-synthetic-zeta-v1".to_owned(),
            "capability-synthetic-alpha-v1".to_owned(),
        ];
        assert!(request.validate().is_err());

        let request: GeneratorRequest = serde_json::from_str(REQUEST).unwrap();
        let mut result: GeneratorResult = serde_json::from_str(SUCCESS_RESULT).unwrap();
        result.output.as_mut().unwrap().path = "outputs/other.bin".to_owned();
        assert!(result.validate_against_request(&request).is_err());
    }

    #[test]
    fn diagnostics_and_documents_are_bounded() {
        let oversized = vec![b' '; MAX_RESULT_BYTES + 1];
        assert!(matches!(
            parse_result(&oversized),
            Err(ProtocolError::DocumentTooLarge { .. })
        ));

        let mut result: GeneratorResult = serde_json::from_str(FAILURE_RESULT).unwrap();
        result.errors[0].message = "x".repeat(MAX_MESSAGE_LENGTH + 1);
        assert!(result.validate().is_err());
    }
}
