use std::{
    collections::{HashMap, HashSet},
    fmt,
};

use serde::{
    Deserialize, Deserializer, Serialize,
    de::{MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Number, Value};
use thiserror::Error;

use crate::{cache_key, generator_protocol::InputRole};

pub const SCHEMA_VERSION: u32 = 1;
pub const SETTINGS_V2_SCHEMA_VERSION: u32 = 2;
pub const MAX_DOCUMENT_BYTES: usize = 1_048_576;
pub const MAX_OBJECTS: usize = 256;
pub const MAX_DESCRIPTION_BYTES: usize = 65_536;
pub const MAX_DESCRIPTION_ANNOTATION_BYTES: usize = 4_096;
pub const MAX_NAME_SUFFIX_BYTES: usize = 192;
pub const MAX_DISPLAY_NAME_SCALARS: usize = 256;
pub const MAX_DESCRIPTION_TARGETS: usize = 64;
pub const MAX_NAME_TARGETS: usize = 4;
pub const MAX_OCCURRENCE_PATH_SEGMENTS: usize = 64;
pub const MAX_SOURCE_VALUE_BYTES: usize = 4_096;
pub const MAX_IDENTITY_LENGTH: usize = 256;
pub const MAX_SETTINGS_EDGES: usize = 16_384;

const DESCRIPTION_MARKER_PREFIX: &str = "onshape-export:";
const DESCRIPTION_V1_PREFIX: &str = "onshape-export:v1 ";
const NAME_MARKER_PREFIX: &str = " [onshape-export:";
const AUTHORING_SCHEMA_DOMAIN: &str = "onshape-export-authoring-schema-v1";
const AUTHORING_DOCUMENT_DOMAIN: &str = "onshape-export-authoring-document-v1";
const SETTINGS_SCHEMA_DOMAIN: &str = "onshape-export-generator-settings-schema-v1";
const SETTINGS_DOCUMENT_DOMAIN: &str = "onshape-export-generator-settings-v1";
const SETTINGS_V2_SCHEMA_DOMAIN: &str = "onshape-export-generator-settings-schema-v2";
const SETTINGS_V2_DOCUMENT_DOMAIN: &str = "onshape-export-generator-settings-v2";

const AUTHORING_SCHEMA: &str =
    include_str!("../protocol/authoring/v1/onshape-authoring.schema.json");
const SETTINGS_SCHEMA: &str =
    include_str!("../protocol/generator-settings/v1/generator-settings.schema.json");
const SETTINGS_V2_SCHEMA: &str =
    include_str!("../protocol/generator-settings/v2/generator-settings.schema.json");

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AnnotationError {
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
    #[error("could not compute identity: {0}")]
    Identity(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AuthoringSelector {
    PartStudioPart {
        document_id: String,
        document_microversion: String,
        element_id: String,
        configuration_identity: String,
        part_id: String,
    },
    AssemblyOccurrence {
        document_id: String,
        document_microversion: String,
        element_id: String,
        configuration_identity: String,
        occurrence_path: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SemanticRole {
    Printable,
    SupportBlocker,
}

impl SemanticRole {
    pub fn transport_role(self) -> InputRole {
        match self {
            Self::Printable => InputRole::RawGeometry,
            Self::SupportBlocker => InputRole::AuxiliaryGeometry,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NormalizedAnnotation {
    pub role: SemanticRole,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_key",
        skip_serializing_if = "Option::is_none"
    )]
    pub key: Option<String>,
    pub targets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCarriers {
    pub display_name: String,
    pub annotation: NormalizedAnnotation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthoringObject {
    pub selector: AuthoringSelector,
    pub display_name: String,
    pub annotation: NormalizedAnnotation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthoringDocument {
    pub schema_version: u32,
    pub objects: Vec<AuthoringObject>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoringContextEntry {
    pub selector: AuthoringSelector,
    pub plan_position: usize,
    pub configured_part_identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GeneratorSettingsBlocker {
    pub object_identity: String,
    pub targets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GeneratorSettings {
    pub schema_version: u32,
    pub blockers: Vec<GeneratorSettingsBlocker>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GeneratorSettingsPlacementV2 {
    pub object_identity: String,
    pub matrix: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GeneratorSettingsV2 {
    pub schema_version: u32,
    pub blockers: Vec<GeneratorSettingsBlocker>,
    pub placements: Vec<GeneratorSettingsPlacementV2>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestObjectSummary {
    pub object_identity: String,
    pub transport_role: InputRole,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExpectedPlacementSummaryV2 {
    pub object_identity: String,
    pub transport_role: InputRole,
    pub expected_neutral_placement_matrix: Vec<f64>,
}

pub fn parse_carriers(description: &str, name: &str) -> Result<ParsedCarriers, AnnotationError> {
    let description_annotation = parse_description_annotation(description)?;
    let (display_name, name_annotation) = parse_name_annotation(name)?;

    let annotation = match (description_annotation, name_annotation) {
        (Some(description), Some(name)) => {
            ensure(
                description == name,
                "Description and Name annotations disagree",
            )?;
            description
        }
        (Some(description), None) => description,
        (None, Some(name)) => name,
        (None, None) => NormalizedAnnotation {
            role: SemanticRole::Printable,
            key: None,
            targets: Vec::new(),
        },
    };

    Ok(ParsedCarriers {
        display_name,
        annotation,
    })
}

pub fn normalize_authoring_object(
    selector: AuthoringSelector,
    description: &str,
    name: &str,
) -> Result<AuthoringObject, AnnotationError> {
    let parsed = parse_carriers(description, name)?;
    let object = AuthoringObject {
        selector,
        display_name: parsed.display_name,
        annotation: parsed.annotation,
    };
    validate_authoring_object(&object, 0)?;
    Ok(object)
}

fn parse_description_annotation(
    description: &str,
) -> Result<Option<NormalizedAnnotation>, AnnotationError> {
    ensure(
        description.len() <= MAX_DESCRIPTION_BYTES,
        format!("Description exceeds {MAX_DESCRIPTION_BYTES} UTF-8 bytes"),
    )?;

    let marker_lines: Vec<_> = description
        .split('\n')
        .filter(|line| line.starts_with(DESCRIPTION_MARKER_PREFIX))
        .collect();
    if marker_lines.is_empty() {
        return Ok(None);
    }
    ensure(
        marker_lines.len() == 1,
        "Description must contain exactly one recognized annotation marker line",
    )?;
    let line = marker_lines[0];
    let json = line.strip_prefix(DESCRIPTION_V1_PREFIX).ok_or_else(|| {
        AnnotationError::Invalid("unsupported or incomplete Description marker".to_owned())
    })?;
    ensure(!json.is_empty(), "Description annotation JSON is missing")?;
    ensure(
        json.starts_with('{') && json.ends_with('}'),
        "Description annotation JSON object must begin immediately after the marker and extend through the line end",
    )?;
    ensure(
        json.len() <= MAX_DESCRIPTION_ANNOTATION_BYTES,
        format!("Description annotation exceeds {MAX_DESCRIPTION_ANNOTATION_BYTES} UTF-8 bytes"),
    )?;
    let annotation: NormalizedAnnotation =
        parse_typed_json(json.as_bytes(), json.len(), "Description annotation")?;
    validate_annotation(
        &annotation,
        MAX_DESCRIPTION_TARGETS,
        "Description annotation",
    )?;
    Ok(Some(annotation))
}

fn parse_name_annotation(
    name: &str,
) -> Result<(String, Option<NormalizedAnnotation>), AnnotationError> {
    validate_display_name(name, "API Part Name")?;
    let Some(marker_start) = name.rfind(NAME_MARKER_PREFIX) else {
        return Ok((name.to_owned(), None));
    };
    let suffix = &name[marker_start..];
    let recognized = suffix.ends_with(']') || !suffix.contains(']');
    if !recognized {
        return Ok((name.to_owned(), None));
    }

    ensure(suffix.is_ascii(), "Name annotation suffix must be ASCII")?;
    ensure(
        suffix.len() <= MAX_NAME_SUFFIX_BYTES,
        format!("Name annotation suffix exceeds {MAX_NAME_SUFFIX_BYTES} ASCII bytes"),
    )?;
    ensure(
        !suffix.contains('%'),
        "Name annotation suffix forbids percent",
    )?;
    let body = suffix
        .strip_prefix(" [onshape-export:")
        .and_then(|value| value.strip_suffix(']'))
        .ok_or_else(|| AnnotationError::Invalid("incomplete Name annotation suffix".to_owned()))?;
    let directives: Vec<_> = body.split(';').collect();
    ensure(
        matches!(directives.len(), 3 | 4),
        "Name annotation has an invalid directive count",
    )?;
    ensure(directives[0] == "v1", "unsupported Name annotation version")?;
    let role = match directives[1].strip_prefix("role=") {
        Some("printable") => SemanticRole::Printable,
        Some("supportBlocker") => SemanticRole::SupportBlocker,
        _ => {
            return Err(AnnotationError::Invalid(
                "invalid Name annotation role".to_owned(),
            ));
        }
    };
    let (key, targets_directive) = if directives.len() == 4 {
        let key = directives[2]
            .strip_prefix("key=")
            .ok_or_else(|| AnnotationError::Invalid("invalid Name key directive".to_owned()))?;
        validate_key(key, "Name annotation key")?;
        (Some(key.to_owned()), directives[3])
    } else {
        (None, directives[2])
    };
    let targets_text = targets_directive
        .strip_prefix("targets=")
        .ok_or_else(|| AnnotationError::Invalid("invalid Name targets directive".to_owned()))?;
    let targets = if targets_text.is_empty() {
        Vec::new()
    } else {
        targets_text.split(',').map(str::to_owned).collect()
    };
    let annotation = NormalizedAnnotation { role, key, targets };
    validate_annotation(&annotation, MAX_NAME_TARGETS, "Name annotation")?;

    let display_name = &name[..marker_start];
    validate_display_name(display_name, "displayName after Name suffix removal")?;
    Ok((display_name.to_owned(), Some(annotation)))
}

pub fn parse_authoring_document(bytes: &[u8]) -> Result<AuthoringDocument, AnnotationError> {
    let document: AuthoringDocument =
        parse_typed_json(bytes, MAX_DOCUMENT_BYTES, "authoring document")?;
    validate_authoring_document(&document)?;
    Ok(document)
}

pub fn validate_authoring_document(document: &AuthoringDocument) -> Result<(), AnnotationError> {
    validate_serialized_size(document, "authoring document")?;
    ensure(
        document.schema_version == SCHEMA_VERSION,
        format!(
            "unsupported authoring schemaVersion: {}",
            document.schema_version
        ),
    )?;
    ensure(
        document.objects.len() <= MAX_OBJECTS,
        format!("authoring document exceeds {MAX_OBJECTS} objects"),
    )?;

    let mut selectors = HashSet::new();
    let mut keys: HashMap<&str, Vec<usize>> = HashMap::new();
    for (index, object) in document.objects.iter().enumerate() {
        validate_authoring_object(object, index)?;
        ensure(
            selectors.insert(&object.selector),
            format!("objects[{index}].selector duplicates another selector"),
        )?;
        if let Some(key) = object.annotation.key.as_deref() {
            keys.entry(key).or_default().push(index);
        }
    }

    for (index, object) in document.objects.iter().enumerate() {
        for target in &object.annotation.targets {
            let matches = keys.get(target.as_str()).ok_or_else(|| {
                AnnotationError::Invalid(format!(
                    "objects[{index}] target {target} does not resolve"
                ))
            })?;
            ensure(
                matches.len() == 1,
                format!("objects[{index}] target {target} is ambiguous"),
            )?;
            let target_index = matches[0];
            ensure(
                target_index != index,
                format!("objects[{index}] cannot target itself"),
            )?;
            ensure(
                document.objects[target_index].annotation.role == SemanticRole::Printable,
                format!("objects[{index}] target {target} is not printable"),
            )?;
        }
    }
    Ok(())
}

fn validate_authoring_object(
    object: &AuthoringObject,
    index: usize,
) -> Result<(), AnnotationError> {
    validate_selector(&object.selector, &format!("objects[{index}].selector"))?;
    validate_display_name(
        &object.display_name,
        &format!("objects[{index}].displayName"),
    )?;
    validate_annotation(
        &object.annotation,
        MAX_DESCRIPTION_TARGETS,
        &format!("objects[{index}].annotation"),
    )
}

pub fn validate_authoring_context(
    document: &AuthoringDocument,
    context: &[AuthoringContextEntry],
) -> Result<(), AnnotationError> {
    validate_authoring_document(document)?;
    ensure(
        context.len() == document.objects.len(),
        "authoring context must contain exactly one entry per object",
    )?;

    let mut by_configured_part: HashMap<&str, (String, Vec<u8>)> = HashMap::new();
    type KeyGroupEntry<'a> = (usize, &'a str, Vec<u8>);
    let mut key_groups: HashMap<&str, Vec<KeyGroupEntry<'_>>> = HashMap::new();
    for (index, (object, entry)) in document.objects.iter().zip(context).enumerate() {
        ensure(
            entry.plan_position == index,
            format!("context[{index}].planPosition does not match document order"),
        )?;
        ensure(
            entry.selector == object.selector,
            format!("context[{index}].selector does not match the authoring object"),
        )?;
        ensure(
            !entry.configured_part_identity.is_empty(),
            format!("context[{index}].configuredPartIdentity must not be empty"),
        )?;
        let canonical = canonical_annotation(&object.annotation)?;
        if let Some((display_name, annotation)) =
            by_configured_part.get(entry.configured_part_identity.as_str())
        {
            ensure(
                display_name == &object.display_name,
                format!(
                    "configured part {} has conflicting display names",
                    entry.configured_part_identity
                ),
            )?;
            ensure(
                annotation == &canonical,
                format!(
                    "configured part {} has conflicting annotations",
                    entry.configured_part_identity
                ),
            )?;
        } else {
            by_configured_part.insert(
                &entry.configured_part_identity,
                (object.display_name.clone(), canonical.clone()),
            );
        }
        if let Some(key) = object.annotation.key.as_deref() {
            key_groups.entry(key).or_default().push((
                index,
                &entry.configured_part_identity,
                canonical,
            ));
        }
    }

    for (key, group) in key_groups {
        if group.len() <= 1 {
            continue;
        }
        let (_, configured_part, annotation) = &group[0];
        ensure(
            group
                .iter()
                .all(|(_, candidate_part, candidate_annotation)| {
                    candidate_part == configured_part && candidate_annotation == annotation
                }),
            format!("duplicate key {key} does not represent identical configured-part fan-out"),
        )?;
    }
    Ok(())
}

pub fn build_generator_settings(
    document: &AuthoringDocument,
    manifest_object_identities: &[String],
) -> Result<GeneratorSettings, AnnotationError> {
    validate_authoring_document(document)?;
    ensure(
        manifest_object_identities.len() == document.objects.len(),
        "manifest identity list must align exactly with authoring objects",
    )?;
    for (index, identity) in manifest_object_identities.iter().enumerate() {
        validate_identity(identity, &format!("manifestObjectIdentities[{index}]"))?;
    }

    let unique_keys: HashMap<&str, usize> = document
        .objects
        .iter()
        .enumerate()
        .filter_map(|(index, object)| object.annotation.key.as_deref().map(|key| (key, index)))
        .collect();
    let blockers = document
        .objects
        .iter()
        .enumerate()
        .filter(|(_, object)| object.annotation.role == SemanticRole::SupportBlocker)
        .map(|(index, object)| GeneratorSettingsBlocker {
            object_identity: manifest_object_identities[index].clone(),
            targets: object
                .annotation
                .targets
                .iter()
                .map(|target| manifest_object_identities[unique_keys[target.as_str()]].clone())
                .collect(),
        })
        .collect();
    let settings = GeneratorSettings {
        schema_version: SCHEMA_VERSION,
        blockers,
    };
    validate_generator_settings(&settings)?;
    Ok(settings)
}

pub fn parse_generator_settings(bytes: &[u8]) -> Result<GeneratorSettings, AnnotationError> {
    let settings: GeneratorSettings =
        parse_typed_json(bytes, MAX_DOCUMENT_BYTES, "generator settings")?;
    validate_generator_settings(&settings)?;
    Ok(settings)
}

pub fn validate_generator_settings(settings: &GeneratorSettings) -> Result<(), AnnotationError> {
    validate_serialized_size(settings, "generator settings")?;
    ensure(
        settings.schema_version == SCHEMA_VERSION,
        format!(
            "unsupported generator settings schemaVersion: {}",
            settings.schema_version
        ),
    )?;
    ensure(
        settings.blockers.len() <= MAX_OBJECTS,
        format!("generator settings exceed {MAX_OBJECTS} blockers"),
    )?;
    let mut blockers = HashSet::new();
    let mut edge_count = 0usize;
    for (index, blocker) in settings.blockers.iter().enumerate() {
        validate_identity(
            &blocker.object_identity,
            &format!("blockers[{index}].objectIdentity"),
        )?;
        ensure(
            blockers.insert(blocker.object_identity.as_str()),
            format!("duplicate blocker identity: {}", blocker.object_identity),
        )?;
        ensure(
            !blocker.targets.is_empty() && blocker.targets.len() <= MAX_DESCRIPTION_TARGETS,
            format!("blockers[{index}].targets must contain 1 to 64 identities"),
        )?;
        let mut targets = HashSet::new();
        for (target_index, target) in blocker.targets.iter().enumerate() {
            validate_identity(
                target,
                &format!("blockers[{index}].targets[{target_index}]"),
            )?;
            ensure(
                targets.insert(target.as_str()),
                format!("blockers[{index}] contains duplicate target {target}"),
            )?;
        }
        edge_count += blocker.targets.len();
    }
    ensure(
        edge_count <= MAX_SETTINGS_EDGES,
        format!("generator settings exceed {MAX_SETTINGS_EDGES} target edges"),
    )
}

pub fn validate_settings_context(
    settings: &GeneratorSettings,
    manifest: &[ManifestObjectSummary],
) -> Result<(), AnnotationError> {
    validate_generator_settings(settings)?;
    let mut by_identity = HashMap::new();
    for (index, object) in manifest.iter().enumerate() {
        validate_identity(
            &object.object_identity,
            &format!("manifest[{index}].objectIdentity"),
        )?;
        ensure(
            by_identity
                .insert(object.object_identity.as_str(), object.transport_role)
                .is_none(),
            format!(
                "manifest contains duplicate identity {}",
                object.object_identity
            ),
        )?;
    }
    let auxiliary: HashSet<_> = manifest
        .iter()
        .filter(|object| object.transport_role == InputRole::AuxiliaryGeometry)
        .map(|object| object.object_identity.as_str())
        .collect();
    let blockers: HashSet<_> = settings
        .blockers
        .iter()
        .map(|blocker| blocker.object_identity.as_str())
        .collect();
    ensure(
        blockers == auxiliary,
        "settings blockers must equal the manifest auxiliaryGeometry identity set",
    )?;
    for blocker in &settings.blockers {
        ensure(
            by_identity.get(blocker.object_identity.as_str())
                == Some(&InputRole::AuxiliaryGeometry),
            format!(
                "blocker {} is not auxiliaryGeometry",
                blocker.object_identity
            ),
        )?;
        for target in &blocker.targets {
            ensure(
                by_identity.get(target.as_str()) == Some(&InputRole::RawGeometry),
                format!("target {target} is missing or is not rawGeometry"),
            )?;
        }
    }
    Ok(())
}

pub fn normalize_generator_settings_v2(settings: &GeneratorSettingsV2) -> GeneratorSettingsV2 {
    let mut normalized = settings.clone();
    for placement in &mut normalized.placements {
        normalize_matrix(&mut placement.matrix);
    }
    normalized
}

pub fn parse_generator_settings_v2(bytes: &[u8]) -> Result<GeneratorSettingsV2, AnnotationError> {
    let settings: GeneratorSettingsV2 =
        parse_typed_json(bytes, MAX_DOCUMENT_BYTES, "generator settings v2")?;
    let normalized = normalize_generator_settings_v2(&settings);
    validate_normalized_generator_settings_v2(&normalized)?;
    Ok(normalized)
}

pub fn validate_generator_settings_v2(
    settings: &GeneratorSettingsV2,
) -> Result<(), AnnotationError> {
    validate_normalized_generator_settings_v2(&normalize_generator_settings_v2(settings))
}

fn validate_normalized_generator_settings_v2(
    settings: &GeneratorSettingsV2,
) -> Result<(), AnnotationError> {
    validate_serialized_size(settings, "generator settings v2")?;
    ensure(
        settings.schema_version == SETTINGS_V2_SCHEMA_VERSION,
        format!(
            "unsupported generator settings v2 schemaVersion: {}",
            settings.schema_version
        ),
    )?;
    validate_generator_settings(&GeneratorSettings {
        schema_version: SCHEMA_VERSION,
        blockers: settings.blockers.clone(),
    })?;
    ensure(
        settings.placements.len() <= MAX_OBJECTS,
        format!("generator settings v2 exceed {MAX_OBJECTS} placements"),
    )?;
    let mut identities = HashSet::new();
    for (index, placement) in settings.placements.iter().enumerate() {
        validate_identity(
            &placement.object_identity,
            &format!("placements[{index}].objectIdentity"),
        )?;
        ensure(
            identities.insert(placement.object_identity.as_str()),
            format!(
                "duplicate placement identity: {}",
                placement.object_identity
            ),
        )?;
        validate_matrix(&placement.matrix, &format!("placements[{index}].matrix"))?;
    }
    Ok(())
}

pub fn validate_settings_context_v2(
    settings: &GeneratorSettingsV2,
    expected: &[ExpectedPlacementSummaryV2],
) -> Result<(), AnnotationError> {
    let normalized = normalize_generator_settings_v2(settings);
    validate_normalized_generator_settings_v2(&normalized)?;
    ensure(
        normalized.placements.len() == expected.len(),
        "settings placements must contain exactly one entry per expected manifest object",
    )?;

    let manifest: Vec<_> = expected
        .iter()
        .map(|entry| ManifestObjectSummary {
            object_identity: entry.object_identity.clone(),
            transport_role: entry.transport_role,
        })
        .collect();
    validate_settings_context(
        &GeneratorSettings {
            schema_version: SCHEMA_VERSION,
            blockers: normalized.blockers.clone(),
        },
        &manifest,
    )?;

    for (index, (placement, entry)) in normalized.placements.iter().zip(expected).enumerate() {
        ensure(
            placement.object_identity == entry.object_identity,
            format!("placements[{index}].objectIdentity does not match expected manifest order"),
        )?;
        let mut expected_matrix = entry.expected_neutral_placement_matrix.clone();
        normalize_matrix(&mut expected_matrix);
        validate_matrix(
            &expected_matrix,
            &format!("expected[{index}].expectedNeutralPlacementMatrix"),
        )?;
        ensure(
            placement.matrix == expected_matrix,
            format!("placements[{index}].matrix does not match expected neutral placement"),
        )?;
    }
    Ok(())
}

fn normalize_matrix(matrix: &mut [f64]) {
    for scalar in matrix {
        if *scalar == 0.0 {
            *scalar = 0.0;
        }
    }
}

fn validate_matrix(matrix: &[f64], field: &str) -> Result<(), AnnotationError> {
    ensure(
        matrix.len() == 16,
        format!("{field} must contain exactly 16 scalars"),
    )?;
    ensure(
        matrix.iter().all(|scalar| scalar.is_finite()),
        format!("{field} must contain only finite scalars"),
    )?;
    ensure(
        matrix[12..] == [0.0, 0.0, 0.0, 1.0],
        format!("{field} final row must be [0,0,0,1] after signed-zero normalization"),
    )
}

pub fn authoring_schema_identity() -> Result<String, AnnotationError> {
    schema_identity(
        AUTHORING_SCHEMA.as_bytes(),
        AUTHORING_SCHEMA_DOMAIN,
        "authoring schema",
    )
}

pub fn generator_settings_schema_identity() -> Result<String, AnnotationError> {
    schema_identity(
        SETTINGS_SCHEMA.as_bytes(),
        SETTINGS_SCHEMA_DOMAIN,
        "generator settings schema",
    )
}

pub fn generator_settings_v2_schema_identity() -> Result<String, AnnotationError> {
    schema_identity(
        SETTINGS_V2_SCHEMA.as_bytes(),
        SETTINGS_V2_SCHEMA_DOMAIN,
        "generator settings v2 schema",
    )
}

pub fn authoring_document_identity(
    document: &AuthoringDocument,
) -> Result<String, AnnotationError> {
    validate_authoring_document(document)?;
    hash_json(AUTHORING_DOCUMENT_DOMAIN, document)
}

pub fn generator_settings_identity(
    settings: &GeneratorSettings,
) -> Result<String, AnnotationError> {
    validate_generator_settings(settings)?;
    hash_json(SETTINGS_DOCUMENT_DOMAIN, settings)
}

pub fn generator_settings_v2_canonical_json_bytes(
    settings: &GeneratorSettingsV2,
) -> Result<Vec<u8>, AnnotationError> {
    let normalized = normalize_generator_settings_v2(settings);
    validate_normalized_generator_settings_v2(&normalized)?;
    cache_key::canonical_json_bytes(&normalized)
        .map_err(|error| AnnotationError::Identity(error.to_string()))
}

pub fn generator_settings_v2_identity(
    settings: &GeneratorSettingsV2,
) -> Result<String, AnnotationError> {
    let normalized = normalize_generator_settings_v2(settings);
    validate_normalized_generator_settings_v2(&normalized)?;
    hash_json(SETTINGS_V2_DOCUMENT_DOMAIN, &normalized)
}

fn schema_identity(
    bytes: &[u8],
    domain: &str,
    document: &'static str,
) -> Result<String, AnnotationError> {
    let schema = parse_strict_value(bytes, MAX_DOCUMENT_BYTES, document)?;
    hash_json(domain, &schema)
}

fn hash_json<T: Serialize + ?Sized>(domain: &str, payload: &T) -> Result<String, AnnotationError> {
    cache_key::hash_json(domain, payload)
        .map_err(|error| AnnotationError::Identity(error.to_string()))
}

fn canonical_annotation(annotation: &NormalizedAnnotation) -> Result<Vec<u8>, AnnotationError> {
    cache_key::canonical_json_bytes(annotation)
        .map_err(|error| AnnotationError::Identity(error.to_string()))
}

fn validate_selector(selector: &AuthoringSelector, field: &str) -> Result<(), AnnotationError> {
    let (document_id, microversion, element_id, configuration) = match selector {
        AuthoringSelector::PartStudioPart {
            document_id,
            document_microversion,
            element_id,
            configuration_identity,
            part_id,
        } => {
            validate_source_value(part_id, &format!("{field}.partId"))?;
            (
                document_id,
                document_microversion,
                element_id,
                configuration_identity,
            )
        }
        AuthoringSelector::AssemblyOccurrence {
            document_id,
            document_microversion,
            element_id,
            configuration_identity,
            occurrence_path,
        } => {
            ensure(
                !occurrence_path.is_empty()
                    && occurrence_path.len() <= MAX_OCCURRENCE_PATH_SEGMENTS,
                format!(
                    "{field}.occurrencePath must contain 1 to {MAX_OCCURRENCE_PATH_SEGMENTS} segments"
                ),
            )?;
            for (index, segment) in occurrence_path.iter().enumerate() {
                validate_source_value(segment, &format!("{field}.occurrencePath[{index}]"))?;
            }
            (
                document_id,
                document_microversion,
                element_id,
                configuration_identity,
            )
        }
    };
    validate_source_value(document_id, &format!("{field}.documentId"))?;
    validate_source_value(microversion, &format!("{field}.documentMicroversion"))?;
    validate_source_value(element_id, &format!("{field}.elementId"))?;
    validate_source_value(configuration, &format!("{field}.configurationIdentity"))
}

fn validate_annotation(
    annotation: &NormalizedAnnotation,
    target_limit: usize,
    field: &str,
) -> Result<(), AnnotationError> {
    if let Some(key) = &annotation.key {
        validate_key(key, &format!("{field}.key"))?;
    }
    ensure(
        annotation.targets.len() <= target_limit,
        format!("{field}.targets exceeds {target_limit} entries"),
    )?;
    let mut targets = HashSet::new();
    for (index, target) in annotation.targets.iter().enumerate() {
        validate_key(target, &format!("{field}.targets[{index}]"))?;
        ensure(
            targets.insert(target.as_str()),
            format!("{field}.targets contains duplicate key {target}"),
        )?;
    }
    match annotation.role {
        SemanticRole::Printable => ensure(
            annotation.targets.is_empty(),
            format!("{field}: printable annotations cannot have targets"),
        ),
        SemanticRole::SupportBlocker => ensure(
            !annotation.targets.is_empty(),
            format!("{field}: supportBlocker annotations require a target"),
        ),
    }
}

fn validate_key(value: &str, field: &str) -> Result<(), AnnotationError> {
    let mut bytes = value.bytes();
    ensure(
        value.len() <= 32
            && bytes
                .next()
                .is_some_and(|byte| byte.is_ascii_alphanumeric())
            && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')),
        format!("{field} must match [A-Za-z0-9][A-Za-z0-9._-]{{0,31}}"),
    )
}

fn validate_display_name(value: &str, field: &str) -> Result<(), AnnotationError> {
    ensure(
        !value.is_empty()
            && value.chars().count() <= MAX_DISPLAY_NAME_SCALARS
            && value.chars().all(|character| !character.is_control()),
        format!(
            "{field} must contain 1 to {MAX_DISPLAY_NAME_SCALARS} Unicode scalar values and no Cc characters"
        ),
    )
}

fn validate_source_value(value: &str, field: &str) -> Result<(), AnnotationError> {
    ensure(
        !value.is_empty()
            && value.len() <= MAX_SOURCE_VALUE_BYTES
            && value.bytes().all(|byte| byte.is_ascii_graphic()),
        format!("{field} must contain 1 to {MAX_SOURCE_VALUE_BYTES} visible ASCII bytes"),
    )
}

fn validate_identity(value: &str, field: &str) -> Result<(), AnnotationError> {
    ensure(
        !value.is_empty()
            && value.len() <= MAX_IDENTITY_LENGTH
            && value.bytes().all(|byte| byte.is_ascii_graphic()),
        format!("{field} must contain 1 to {MAX_IDENTITY_LENGTH} visible ASCII characters"),
    )
}

fn validate_serialized_size<T: Serialize + ?Sized>(
    value: &T,
    document: &'static str,
) -> Result<(), AnnotationError> {
    let size = serde_json::to_vec(value)
        .map_err(|error| AnnotationError::InvalidJson {
            document,
            message: error.to_string(),
        })?
        .len();
    if size > MAX_DOCUMENT_BYTES {
        Err(AnnotationError::DocumentTooLarge {
            document,
            limit: MAX_DOCUMENT_BYTES,
        })
    } else {
        Ok(())
    }
}

fn deserialize_optional_key<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    String::deserialize(deserializer).map(Some)
}

fn parse_typed_json<T: for<'de> Deserialize<'de>>(
    bytes: &[u8],
    limit: usize,
    document: &'static str,
) -> Result<T, AnnotationError> {
    if bytes.len() > limit {
        return Err(AnnotationError::DocumentTooLarge { document, limit });
    }
    serde_json::from_slice(bytes).map_err(|error| AnnotationError::InvalidJson {
        document,
        message: error.to_string(),
    })
}

fn parse_strict_value(
    bytes: &[u8],
    limit: usize,
    document: &'static str,
) -> Result<Value, AnnotationError> {
    if bytes.len() > limit {
        return Err(AnnotationError::DocumentTooLarge { document, limit });
    }
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let StrictValue(value) = StrictValue::deserialize(&mut deserializer).map_err(|error| {
        AnnotationError::InvalidJson {
            document,
            message: error.to_string(),
        }
    })?;
    deserializer
        .end()
        .map_err(|error| AnnotationError::InvalidJson {
            document,
            message: error.to_string(),
        })?;
    Ok(value)
}

struct StrictValue(Value);

impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictValueVisitor)
    }
}

struct StrictValueVisitor;

impl<'de> Visitor<'de> for StrictValueVisitor {
    type Value = StrictValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value without duplicate object members")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Number(Number::from(value))))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Number(Number::from(value))))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Number::from_f64(value)
            .map(Value::Number)
            .map(StrictValue)
            .ok_or_else(|| E::custom("non-finite JSON number"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::String(value.to_owned())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Null))
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(StrictValue(value)) = sequence.next_element()? {
            values.push(value);
        }
        Ok(StrictValue(Value::Array(values)))
    }

    fn visit_map<A>(self, mut object: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = Map::new();
        while let Some((key, StrictValue(value))) = object.next_entry::<String, StrictValue>()? {
            if values.contains_key(&key) {
                return Err(serde::de::Error::custom(format!(
                    "duplicate object member {key}"
                )));
            }
            values.insert(key, value);
        }
        Ok(StrictValue(Value::Object(values)))
    }
}

fn ensure(condition: bool, message: impl Into<String>) -> Result<(), AnnotationError> {
    if condition {
        Ok(())
    } else {
        Err(AnnotationError::Invalid(message.into()))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn selector(part_id: &str) -> AuthoringSelector {
        AuthoringSelector::PartStudioPart {
            document_id: "document-synthetic".to_owned(),
            document_microversion: "microversion-synthetic".to_owned(),
            element_id: "element-synthetic".to_owned(),
            configuration_identity: "configuration-synthetic".to_owned(),
            part_id: part_id.to_owned(),
        }
    }

    fn occurrence(path: &[&str]) -> AuthoringSelector {
        AuthoringSelector::AssemblyOccurrence {
            document_id: "document-synthetic".to_owned(),
            document_microversion: "microversion-synthetic".to_owned(),
            element_id: "assembly-synthetic".to_owned(),
            configuration_identity: "configuration-synthetic".to_owned(),
            occurrence_path: path.iter().map(|segment| (*segment).to_owned()).collect(),
        }
    }

    fn annotation(role: SemanticRole, key: Option<&str>, targets: &[&str]) -> NormalizedAnnotation {
        NormalizedAnnotation {
            role,
            key: key.map(str::to_owned),
            targets: targets.iter().map(|target| (*target).to_owned()).collect(),
        }
    }

    fn object(
        selector: AuthoringSelector,
        display_name: &str,
        annotation: NormalizedAnnotation,
    ) -> AuthoringObject {
        AuthoringObject {
            selector,
            display_name: display_name.to_owned(),
            annotation,
        }
    }

    fn document(objects: Vec<AuthoringObject>) -> AuthoringDocument {
        AuthoringDocument {
            schema_version: SCHEMA_VERSION,
            objects,
        }
    }

    fn printable(part_id: &str, display_name: &str, key: Option<&str>) -> AuthoringObject {
        object(
            selector(part_id),
            display_name,
            annotation(SemanticRole::Printable, key, &[]),
        )
    }

    fn schema_value(schema: &str) -> Value {
        serde_json::from_str(schema).unwrap()
    }

    #[test]
    fn schemas_are_valid_draft_2020_12_with_exact_ids() {
        for (schema, expected_id) in [
            (
                AUTHORING_SCHEMA,
                "https://github.com/altendky/onshape-export/protocol/authoring/v1/onshape-authoring.schema.json",
            ),
            (
                SETTINGS_SCHEMA,
                "https://github.com/altendky/onshape-export/protocol/generator-settings/v1/generator-settings.schema.json",
            ),
            (
                SETTINGS_V2_SCHEMA,
                "https://github.com/altendky/onshape-export/protocol/generator-settings/v2/generator-settings.schema.json",
            ),
        ] {
            let schema = schema_value(schema);
            assert!(jsonschema::draft202012::meta::is_valid(&schema));
            assert_eq!(schema["$id"], expected_id);
        }
    }

    #[test]
    fn schemas_accept_valid_documents_and_reject_unknown_fields() {
        let authoring = document(vec![
            printable("part-a", "Printable A", Some("a")),
            object(
                occurrence(&["root", "leaf"]),
                "Blocker",
                annotation(SemanticRole::SupportBlocker, None, &["a"]),
            ),
        ]);
        let settings = GeneratorSettings {
            schema_version: 1,
            blockers: vec![GeneratorSettingsBlocker {
                object_identity: "object-blocker".to_owned(),
                targets: vec!["object-a".to_owned()],
            }],
        };
        assert!(jsonschema::draft202012::is_valid(
            &schema_value(AUTHORING_SCHEMA),
            &serde_json::to_value(&authoring).unwrap()
        ));
        assert!(jsonschema::draft202012::is_valid(
            &schema_value(SETTINGS_SCHEMA),
            &serde_json::to_value(&settings).unwrap()
        ));
        let settings_v2 = settings_v2();
        assert!(jsonschema::draft202012::is_valid(
            &schema_value(SETTINGS_V2_SCHEMA),
            &serde_json::to_value(&settings_v2).unwrap()
        ));

        let mut invalid = serde_json::to_value(authoring).unwrap();
        invalid["objects"][0]["annotation"]["displayName"] = json!("forbidden");
        assert!(!jsonschema::draft202012::is_valid(
            &schema_value(AUTHORING_SCHEMA),
            &invalid
        ));
        let mut invalid = serde_json::to_value(settings).unwrap();
        invalid["blockers"][0]["sourceSelector"] = json!("forbidden");
        assert!(!jsonschema::draft202012::is_valid(
            &schema_value(SETTINGS_SCHEMA),
            &invalid
        ));
        let mut invalid = serde_json::to_value(settings_v2).unwrap();
        invalid["placements"][0]["sourceSelector"] = json!("forbidden");
        assert!(!jsonschema::draft202012::is_valid(
            &schema_value(SETTINGS_V2_SCHEMA),
            &invalid
        ));
    }

    #[test]
    fn parses_description_marker_among_ordinary_text() {
        let parsed = parse_carriers(
            "ordinary text\nonshape-export:v1 {\"role\":\"supportBlocker\",\"key\":\"block\",\"targets\":[\"a\",\"b\"]}\nmore text",
            "Synthetic blocker",
        )
        .unwrap();
        assert_eq!(parsed.display_name, "Synthetic blocker");
        assert_eq!(
            parsed.annotation,
            annotation(SemanticRole::SupportBlocker, Some("block"), &["a", "b"])
        );
    }

    #[test]
    fn description_parser_rejects_duplicate_keys_trailing_data_and_markers() {
        for description in [
            r#"onshape-export:v1 {"role":"printable","role":"printable","targets":[]}"#,
            r#"onshape-export:v1 {"role":"printable","targets":[]} trailing"#,
            "onshape-export:v1 {\"role\":\"printable\",\"targets\":[]}\nonshape-export:v1 {\"role\":\"printable\",\"targets\":[]}",
            r#"onshape-export:v2 {"role":"printable","targets":[]}"#,
            "onshape-export:v1",
            r#"onshape-export:v1 {"role":"printable","targets":[],"unknown":true}"#,
            r#"onshape-export:v1 {"role":"printable","targets":[]"#,
            r#"onshape-export:v1  {"role":"printable","targets":[]}"#,
            "onshape-export:v1 {\"role\":\"printable\",\"targets\":[]} ",
            r#"onshape-export:v1 {"role":"printable","key":null,"targets":[]}"#,
        ] {
            assert!(
                parse_carriers(description, "Part").is_err(),
                "{description}"
            );
        }
    }

    #[test]
    fn description_parser_enforces_byte_and_target_bounds() {
        assert!(parse_carriers(&"x".repeat(MAX_DESCRIPTION_BYTES), "Part").is_ok());
        assert!(parse_carriers(&"x".repeat(MAX_DESCRIPTION_BYTES + 1), "Part").is_err());

        let oversized_json = format!(
            "onshape-export:v1 {}{{\"role\":\"printable\",\"targets\":[]}}",
            " ".repeat(MAX_DESCRIPTION_ANNOTATION_BYTES)
        );
        assert!(parse_carriers(&oversized_json, "Part").is_err());

        let annotation = r#"{"role":"printable","targets":[]}"#;
        let exact_json = format!(
            "{}{}{}",
            &annotation[..annotation.len() - 1],
            " ".repeat(MAX_DESCRIPTION_ANNOTATION_BYTES - annotation.len()),
            "}"
        );
        assert_eq!(exact_json.len(), MAX_DESCRIPTION_ANNOTATION_BYTES);
        assert!(parse_carriers(&format!("onshape-export:v1 {exact_json}"), "Part").is_ok());
        let over_json = format!("{} }}", &exact_json[..exact_json.len() - 1]);
        assert_eq!(over_json.len(), MAX_DESCRIPTION_ANNOTATION_BYTES + 1);
        assert!(parse_carriers(&format!("onshape-export:v1 {over_json}"), "Part").is_err());

        let targets: Vec<_> = (0..=MAX_DESCRIPTION_TARGETS)
            .map(|index| format!("k{index}"))
            .collect();
        let description = format!(
            "onshape-export:v1 {}",
            serde_json::to_string(&NormalizedAnnotation {
                role: SemanticRole::SupportBlocker,
                key: None,
                targets,
            })
            .unwrap()
        );
        assert!(parse_carriers(&description, "Part").is_err());
    }

    #[test]
    fn parses_exact_name_fallback_and_removes_only_the_suffix() {
        let parsed = parse_carriers(
            "ordinary",
            "Blocker [onshape-export:v1;role=supportBlocker;key=block;targets=a,b]",
        )
        .unwrap();
        assert_eq!(parsed.display_name, "Blocker");
        assert_eq!(
            parsed.annotation,
            annotation(SemanticRole::SupportBlocker, Some("block"), &["a", "b"])
        );

        let parsed = parse_carriers(
            "ordinary",
            "Printable [onshape-export:v1;role=printable;targets=]",
        )
        .unwrap();
        assert_eq!(parsed.display_name, "Printable");
        assert_eq!(
            parsed.annotation,
            annotation(SemanticRole::Printable, None, &[])
        );
    }

    #[test]
    fn name_fallback_rejects_malformed_recognized_terminal_suffixes() {
        for name in [
            "Part [onshape-export:v2;role=printable;targets=]",
            "Part [onshape-export:v1;targets=;role=printable]",
            "Part [onshape-export:v1;role=printable;role=printable;targets=]",
            "Part [onshape-export:v1;role=printable;unknown=x;targets=]",
            "Part [onshape-export:v1;role=printable;targets=x]",
            "Part [onshape-export:v1;role=supportBlocker;targets=]",
            "Part [onshape-export:v1;role=printable;targets=%41]",
            "Part [onshape-export:v1;role=printable;targets=",
            " [onshape-export:v1;role=printable;targets=]",
        ] {
            assert!(parse_carriers("", name).is_err(), "{name}");
        }
    }

    #[test]
    fn nonterminal_marker_text_remains_literal() {
        let name = "Literal [onshape-export:not-an-annotation] trailing";
        let parsed = parse_carriers("ordinary", name).unwrap();
        assert_eq!(parsed.display_name, name);
        assert_eq!(parsed.annotation.role, SemanticRole::Printable);
        assert!(parsed.annotation.key.is_none());
    }

    #[test]
    fn name_fallback_enforces_target_and_suffix_bounds() {
        assert!(
            parse_carriers(
                "",
                "Part [onshape-export:v1;role=supportBlocker;targets=a,b,c,d]"
            )
            .is_ok()
        );
        assert!(
            parse_carriers(
                "",
                "Part [onshape-export:v1;role=supportBlocker;targets=a,b,c,d,e]"
            )
            .is_err()
        );
        let oversized = format!("Part [onshape-export:{}", "x".repeat(MAX_NAME_SUFFIX_BYTES));
        assert!(parse_carriers("", &oversized).is_err());

        let prefix = format!(
            " [onshape-export:v1;role=supportBlocker;key={};targets={},{},{},",
            "k".repeat(32),
            "a".repeat(32),
            "b".repeat(32),
            "c".repeat(32),
        );
        let exact_suffix = format!("{prefix}{}]", "d".repeat(7));
        assert_eq!(exact_suffix.len(), MAX_NAME_SUFFIX_BYTES);
        assert!(parse_carriers("", &format!("Part{exact_suffix}")).is_ok());
        let oversized_suffix = format!("{prefix}{}]", "d".repeat(8));
        assert_eq!(oversized_suffix.len(), MAX_NAME_SUFFIX_BYTES + 1);
        assert!(parse_carriers("", &format!("Part{oversized_suffix}")).is_err());
    }

    #[test]
    fn carrier_precedence_requires_both_valid_and_semantically_equal() {
        let description = r#"onshape-export:v1 {"targets":[],"role":"printable","key":"p"}"#;
        let equal_name = "Part [onshape-export:v1;role=printable;key=p;targets=]";
        assert!(parse_carriers(description, equal_name).is_ok());

        let conflicting_name = "Part [onshape-export:v1;role=printable;key=q;targets=]";
        assert!(parse_carriers(description, conflicting_name).is_err());
        let malformed_name = "Part [onshape-export:v1;role=printable;targets=%]";
        assert!(parse_carriers(description, malformed_name).is_err());
    }

    #[test]
    fn unannotated_names_normalize_to_printable_without_a_key() {
        let parsed = parse_carriers("ordinary description", "Duplicate name").unwrap();
        assert_eq!(parsed.display_name, "Duplicate name");
        assert_eq!(
            parsed.annotation,
            annotation(SemanticRole::Printable, None, &[])
        );
    }

    #[test]
    fn display_names_preserve_unicode_without_normalization_or_byte_limit() {
        let decomposed = "Cafe\u{301}";
        let composed = "Café";
        assert_eq!(
            parse_carriers("", decomposed).unwrap().display_name,
            decomposed
        );
        assert_eq!(parse_carriers("", composed).unwrap().display_name, composed);
        assert_ne!(decomposed, composed);

        let multibyte = "😀".repeat(MAX_DISPLAY_NAME_SCALARS);
        assert!(multibyte.len() > MAX_DISPLAY_NAME_SCALARS);
        assert!(parse_carriers("", &multibyte).is_ok());
        assert!(parse_carriers("", &(multibyte + "😀")).is_err());
        assert!(parse_carriers("", "control\u{001f}").is_err());
        assert!(parse_carriers("", "control\u{0085}").is_err());
        assert!(parse_carriers("", "allowed\u{00a0}").is_ok());
    }

    #[test]
    fn marker_allows_unicode_only_outside_suffix() {
        assert!(parse_carriers("", "部品 [onshape-export:v1;role=printable;targets=]").is_ok());
        assert!(
            parse_carriers(
                "",
                "Part [onshape-export:v1;role=printable;key=部品;targets=]"
            )
            .is_err()
        );
    }

    #[test]
    fn authoring_parser_rejects_invalid_utf8_duplicate_members_and_versions() {
        assert!(parse_authoring_document(&[0xff]).is_err());
        assert!(
            parse_authoring_document(br#"{"schemaVersion":1,"schemaVersion":1,"objects":[]}"#)
                .is_err()
        );
        assert!(parse_authoring_document(br#"{"schemaVersion":2,"objects":[]}"#).is_err());
        assert!(
            parse_authoring_document(
                br#"{"schemaVersion":1,"objects":[{"selector":{"kind":"partStudioPart","documentId":"d","documentMicroversion":"m","elementId":"e","configurationIdentity":"c","partId":"p"},"displayName":"Part","annotation":{"role":"printable","key":null,"targets":[]}}]}"#
            )
            .is_err()
        );
        assert!(matches!(
            parse_authoring_document(&vec![b' '; MAX_DOCUMENT_BYTES + 1]),
            Err(AnnotationError::DocumentTooLarge { .. })
        ));

        let mut exact = br#"{"schemaVersion":1,"objects":[]}"#.to_vec();
        exact.resize(MAX_DOCUMENT_BYTES, b' ');
        assert!(parse_authoring_document(&exact).is_ok());
        exact.push(b' ');
        assert!(matches!(
            parse_authoring_document(&exact),
            Err(AnnotationError::DocumentTooLarge { .. })
        ));
    }

    #[test]
    fn standalone_validation_resolves_targets_and_rejects_bad_semantics() {
        let valid = document(vec![
            printable("a", "A", Some("a")),
            printable("b", "B", Some("b")),
            object(
                selector("blocker"),
                "Blocker",
                annotation(SemanticRole::SupportBlocker, Some("block"), &["b", "a"]),
            ),
        ]);
        validate_authoring_document(&valid).unwrap();

        let mut missing = valid.clone();
        missing.objects[2].annotation.targets[0] = "missing".to_owned();
        assert!(validate_authoring_document(&missing).is_err());
        let mut blocker_target = valid.clone();
        blocker_target.objects[2].annotation.targets[0] = "block".to_owned();
        assert!(validate_authoring_document(&blocker_target).is_err());
        let mut duplicate = valid.clone();
        duplicate.objects[2].annotation.targets = vec!["a".to_owned(), "a".to_owned()];
        assert!(validate_authoring_document(&duplicate).is_err());
        let mut self_target = valid.clone();
        self_target.objects[2].annotation.targets = vec!["block".to_owned()];
        assert!(validate_authoring_document(&self_target).is_err());
        let mut duplicate_selector = valid.clone();
        duplicate_selector.objects[1].selector = duplicate_selector.objects[0].selector.clone();
        assert!(validate_authoring_document(&duplicate_selector).is_err());
    }

    #[test]
    fn duplicate_keys_are_allowed_only_when_unreferenced_before_context() {
        let duplicate_keys = document(vec![
            printable("a-occurrence", "A", Some("a")),
            printable("a-other-occurrence", "A", Some("a")),
        ]);
        validate_authoring_document(&duplicate_keys).unwrap();

        let targeted = document(vec![
            duplicate_keys.objects[0].clone(),
            duplicate_keys.objects[1].clone(),
            object(
                selector("blocker"),
                "Blocker",
                annotation(SemanticRole::SupportBlocker, None, &["a"]),
            ),
        ]);
        assert!(validate_authoring_document(&targeted).is_err());
    }

    #[test]
    fn contextual_validation_allows_identical_part_fan_out() {
        let authoring = document(vec![
            object(
                occurrence(&["root-a", "leaf"]),
                "Repeated",
                annotation(SemanticRole::Printable, Some("same"), &[]),
            ),
            object(
                occurrence(&["root-b", "leaf"]),
                "Repeated",
                annotation(SemanticRole::Printable, Some("same"), &[]),
            ),
        ]);
        let context = vec![
            AuthoringContextEntry {
                selector: authoring.objects[0].selector.clone(),
                plan_position: 0,
                configured_part_identity: "configured-part-a".to_owned(),
            },
            AuthoringContextEntry {
                selector: authoring.objects[1].selector.clone(),
                plan_position: 1,
                configured_part_identity: "configured-part-a".to_owned(),
            },
        ];
        validate_authoring_context(&authoring, &context).unwrap();
        assert_ne!(authoring.objects[0].selector, authoring.objects[1].selector);
    }

    #[test]
    fn contextual_validation_rejects_cross_part_or_conflicting_fan_out() {
        let mut authoring = document(vec![
            printable("occurrence-a", "Repeated", Some("same")),
            printable("occurrence-b", "Repeated", Some("same")),
        ]);
        let mut context = vec![
            AuthoringContextEntry {
                selector: authoring.objects[0].selector.clone(),
                plan_position: 0,
                configured_part_identity: "configured-part-a".to_owned(),
            },
            AuthoringContextEntry {
                selector: authoring.objects[1].selector.clone(),
                plan_position: 1,
                configured_part_identity: "configured-part-b".to_owned(),
            },
        ];
        assert!(validate_authoring_context(&authoring, &context).is_err());

        context[1].configured_part_identity = "configured-part-a".to_owned();
        authoring.objects[1].display_name = "Renamed".to_owned();
        assert!(validate_authoring_context(&authoring, &context).is_err());
        authoring.objects[1].display_name = "Repeated".to_owned();
        authoring.objects[1].annotation.key = Some("other".to_owned());
        assert!(validate_authoring_context(&authoring, &context).is_err());
    }

    #[test]
    fn contextual_validation_checks_selector_and_plan_position_alignment() {
        let authoring = document(vec![printable("a", "A", None)]);
        let mut context = vec![AuthoringContextEntry {
            selector: authoring.objects[0].selector.clone(),
            plan_position: 1,
            configured_part_identity: "configured-a".to_owned(),
        }];
        assert!(validate_authoring_context(&authoring, &context).is_err());
        context[0].plan_position = 0;
        context[0].selector = selector("different");
        assert!(validate_authoring_context(&authoring, &context).is_err());
    }

    #[test]
    fn settings_builder_maps_roles_and_preserves_object_and_target_order() {
        let authoring = document(vec![
            printable("a", "A", Some("a")),
            printable("b", "B", Some("b")),
            object(
                selector("blocker-one"),
                "Blocker one",
                annotation(SemanticRole::SupportBlocker, None, &["b", "a"]),
            ),
            object(
                selector("blocker-two"),
                "Blocker two",
                annotation(SemanticRole::SupportBlocker, None, &["a"]),
            ),
        ]);
        let identities = ["object-a", "object-b", "blocker-one", "blocker-two"].map(str::to_owned);
        let settings = build_generator_settings(&authoring, &identities).unwrap();
        assert_eq!(
            settings,
            GeneratorSettings {
                schema_version: 1,
                blockers: vec![
                    GeneratorSettingsBlocker {
                        object_identity: "blocker-one".to_owned(),
                        targets: vec!["object-b".to_owned(), "object-a".to_owned()],
                    },
                    GeneratorSettingsBlocker {
                        object_identity: "blocker-two".to_owned(),
                        targets: vec!["object-a".to_owned()],
                    },
                ],
            }
        );
        assert_eq!(
            SemanticRole::Printable.transport_role(),
            InputRole::RawGeometry
        );
        assert_eq!(
            SemanticRole::SupportBlocker.transport_role(),
            InputRole::AuxiliaryGeometry
        );
    }

    #[test]
    fn settings_context_requires_exact_auxiliary_set_and_raw_targets() {
        let settings = GeneratorSettings {
            schema_version: 1,
            blockers: vec![GeneratorSettingsBlocker {
                object_identity: "blocker".to_owned(),
                targets: vec!["printable".to_owned()],
            }],
        };
        let manifest = vec![
            ManifestObjectSummary {
                object_identity: "printable".to_owned(),
                transport_role: InputRole::RawGeometry,
            },
            ManifestObjectSummary {
                object_identity: "untargeted".to_owned(),
                transport_role: InputRole::RawGeometry,
            },
            ManifestObjectSummary {
                object_identity: "blocker".to_owned(),
                transport_role: InputRole::AuxiliaryGeometry,
            },
        ];
        validate_settings_context(&settings, &manifest).unwrap();

        let mut missing = settings.clone();
        missing.blockers.clear();
        assert!(validate_settings_context(&missing, &manifest).is_err());
        let mut wrong_target = manifest.clone();
        wrong_target[0].transport_role = InputRole::AuxiliaryGeometry;
        assert!(validate_settings_context(&settings, &wrong_target).is_err());
        let mut duplicate_manifest = manifest.clone();
        duplicate_manifest.push(manifest[0].clone());
        assert!(validate_settings_context(&settings, &duplicate_manifest).is_err());
    }

    #[test]
    fn settings_parser_rejects_duplicates_unknowns_bad_references_and_limits() {
        assert!(parse_generator_settings(&[0xff]).is_err());
        assert!(
            parse_generator_settings(br#"{"schemaVersion":1,"schemaVersion":1,"blockers":[]}"#)
                .is_err()
        );
        assert!(
            parse_generator_settings(br#"{"schemaVersion":1,"blockers":[],"displayName":"x"}"#)
                .is_err()
        );
        let duplicate_blockers = GeneratorSettings {
            schema_version: 1,
            blockers: vec![
                GeneratorSettingsBlocker {
                    object_identity: "blocker".to_owned(),
                    targets: vec!["a".to_owned()],
                },
                GeneratorSettingsBlocker {
                    object_identity: "blocker".to_owned(),
                    targets: vec!["b".to_owned()],
                },
            ],
        };
        assert!(validate_generator_settings(&duplicate_blockers).is_err());
        let duplicate_edges = GeneratorSettings {
            schema_version: 1,
            blockers: vec![GeneratorSettingsBlocker {
                object_identity: "blocker".to_owned(),
                targets: vec!["a".to_owned(), "a".to_owned()],
            }],
        };
        assert!(validate_generator_settings(&duplicate_edges).is_err());
    }

    #[test]
    fn selector_and_collection_bounds_fail_closed() {
        let mut invalid_source = printable("a", "A", None);
        if let AuthoringSelector::PartStudioPart { document_id, .. } = &mut invalid_source.selector
        {
            *document_id = "with space".to_owned();
        }
        assert!(validate_authoring_document(&document(vec![invalid_source])).is_err());

        let long_path = vec!["segment"; MAX_OCCURRENCE_PATH_SEGMENTS + 1];
        assert!(
            validate_authoring_document(&document(vec![object(
                occurrence(&long_path),
                "Part",
                annotation(SemanticRole::Printable, None, &[]),
            )]))
            .is_err()
        );
        let too_many: Vec<_> = (0..=MAX_OBJECTS)
            .map(|index| printable(&format!("part-{index}"), "Part", None))
            .collect();
        assert!(validate_authoring_document(&document(too_many)).is_err());
    }

    #[test]
    fn typed_validators_enforce_serialized_document_size_limits() {
        let source_value = "x".repeat(MAX_SOURCE_VALUE_BYTES);
        let objects = (0..MAX_OBJECTS)
            .map(|index| {
                object(
                    AuthoringSelector::PartStudioPart {
                        document_id: source_value.clone(),
                        document_microversion: source_value.clone(),
                        element_id: source_value.clone(),
                        configuration_identity: source_value.clone(),
                        part_id: format!("{index}-{}", "x".repeat(MAX_SOURCE_VALUE_BYTES - 4)),
                    },
                    "Part",
                    annotation(SemanticRole::Printable, None, &[]),
                )
            })
            .collect();
        assert!(matches!(
            validate_authoring_document(&document(objects)),
            Err(AnnotationError::DocumentTooLarge { .. })
        ));

        let blockers = (0..MAX_OBJECTS)
            .map(|blocker_index| GeneratorSettingsBlocker {
                object_identity: format!("blocker-{blocker_index}"),
                targets: (0..MAX_DESCRIPTION_TARGETS)
                    .map(|target_index| {
                        format!(
                            "target-{blocker_index}-{target_index}-{}",
                            "x".repeat(MAX_IDENTITY_LENGTH - 15)
                        )
                    })
                    .collect(),
            })
            .collect();
        assert!(matches!(
            validate_generator_settings(&GeneratorSettings {
                schema_version: 1,
                blockers,
            }),
            Err(AnnotationError::DocumentTooLarge { .. })
        ));
    }

    #[test]
    fn strict_value_rejects_nested_duplicate_members() {
        assert!(parse_strict_value(br#"{"outer":{"a":1,"a":2}}"#, 100, "test").is_err());
        assert_eq!(
            parse_strict_value(br#"{"outer":{"a":1}}"#, 100, "test").unwrap(),
            json!({"outer": {"a": 1}})
        );
    }

    #[test]
    fn identities_are_order_and_scalar_sensitive_with_fixed_domains() {
        let original = document(vec![
            printable("a", "A", Some("a")),
            printable("b", "B", Some("b")),
        ]);
        let original_identity = authoring_document_identity(&original).unwrap();
        assert_eq!(
            original_identity,
            "bdb2a4eb8c46f266027c2c80c24edf1a862a1375a6d7690739bb3138c31dc304"
        );
        assert!(
            original_identity
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        );
        let mut reordered = original.clone();
        reordered.objects.reverse();
        assert_ne!(
            original_identity,
            authoring_document_identity(&reordered).unwrap()
        );
        let mut renamed = original.clone();
        renamed.objects[0].display_name = "Renamed".to_owned();
        assert_ne!(
            original_identity,
            authoring_document_identity(&renamed).unwrap()
        );
        let mut changed_selector = original.clone();
        changed_selector.objects[0].selector = selector("changed");
        assert_ne!(
            original_identity,
            authoring_document_identity(&changed_selector).unwrap()
        );

        assert_eq!(
            authoring_schema_identity().unwrap(),
            "24f9150eeae0996429bfd043bc8015ebe8a05e89809ba64e6d346eeea5e25ea1"
        );
        assert_eq!(
            generator_settings_schema_identity().unwrap(),
            "847e1a9b0f8ceb1cff89f03bd443d3ef491b44c953e1f008d28700c877999a95"
        );
        assert_eq!(
            generator_settings_identity(&GeneratorSettings {
                schema_version: 1,
                blockers: vec![GeneratorSettingsBlocker {
                    object_identity: "blocker".to_owned(),
                    targets: vec!["a".to_owned(), "b".to_owned()],
                }],
            })
            .unwrap(),
            "18e345d07cf608ad178ee1a1e08cda4ca0fee6913c98a07be39f4ede0af66d83"
        );
    }

    #[test]
    fn rename_changes_authoring_identity_but_not_manifest_bound_settings() {
        let original = document(vec![
            printable("a", "Original", Some("a")),
            object(
                selector("blocker"),
                "Blocker",
                annotation(SemanticRole::SupportBlocker, None, &["a"]),
            ),
        ]);
        let identities = ["manifest-a".to_owned(), "manifest-blocker".to_owned()];
        let settings = build_generator_settings(&original, &identities).unwrap();
        let settings_identity = generator_settings_identity(&settings).unwrap();

        let mut renamed = original.clone();
        renamed.objects[0].display_name = "Renamed".to_owned();
        assert_ne!(
            authoring_document_identity(&original).unwrap(),
            authoring_document_identity(&renamed).unwrap()
        );
        let renamed_settings = build_generator_settings(&renamed, &identities).unwrap();
        assert_eq!(settings, renamed_settings);
        assert_eq!(
            settings_identity,
            generator_settings_identity(&renamed_settings).unwrap()
        );
    }

    #[test]
    fn rename_flows_through_authoring_manifest_and_invocation_identities() {
        let authoring = document(vec![printable("a", "Original", Some("a"))]);
        let mut renamed_authoring = authoring.clone();
        renamed_authoring.objects[0].display_name = "Renamed".to_owned();
        assert_ne!(
            authoring_document_identity(&authoring).unwrap(),
            authoring_document_identity(&renamed_authoring).unwrap()
        );

        let manifest_text = include_str!("../protocol/generator/v1/examples/input-manifest.json");
        let request_text = include_str!("../protocol/generator/v1/examples/request.json");
        let original_manifest =
            crate::generator_protocol::parse_input_manifest(manifest_text.as_bytes()).unwrap();
        let original_object_identity = original_manifest.objects[0].object_identity.clone();
        let original_input_set = original_manifest.input_set_identity.clone();
        let original_manifest_identity = original_manifest.manifest_identity.clone();

        let mut renamed_manifest = original_manifest.clone();
        renamed_manifest.objects[0].display_name = Some("Renamed".to_owned());
        renamed_manifest.input_set_identity =
            renamed_manifest.computed_input_set_identity().unwrap();
        renamed_manifest.manifest_identity = renamed_manifest.computed_manifest_identity().unwrap();
        renamed_manifest.validate().unwrap();
        assert_eq!(
            renamed_manifest.objects[0].object_identity,
            original_object_identity
        );
        assert_ne!(renamed_manifest.input_set_identity, original_input_set);
        assert_ne!(
            renamed_manifest.manifest_identity,
            original_manifest_identity
        );

        let original_request =
            crate::generator_protocol::parse_request(request_text.as_bytes()).unwrap();
        let original_settings_identity = original_request.settings.settings_identity.clone();
        let mut renamed_request = original_request.clone();
        renamed_request.input_manifest.input_set_identity =
            renamed_manifest.input_set_identity.clone().unwrap();
        renamed_request.input_manifest.manifest_identity =
            renamed_manifest.manifest_identity.clone();
        renamed_request.invocation_identity =
            renamed_request.computed_invocation_identity().unwrap();
        renamed_request.validate().unwrap();
        renamed_request
            .validate_with_manifest(&renamed_manifest)
            .unwrap();
        assert_eq!(
            renamed_request.settings.settings_identity,
            original_settings_identity
        );
        assert_ne!(
            renamed_request.invocation_identity,
            original_request.invocation_identity
        );
    }

    #[test]
    fn target_order_changes_annotation_and_settings_identities() {
        let forward = document(vec![
            printable("a", "A", Some("a")),
            printable("b", "B", Some("b")),
            object(
                selector("blocker"),
                "Blocker",
                annotation(SemanticRole::SupportBlocker, None, &["a", "b"]),
            ),
        ]);
        let mut reversed = forward.clone();
        reversed.objects[2].annotation.targets.reverse();
        assert_ne!(
            authoring_document_identity(&forward).unwrap(),
            authoring_document_identity(&reversed).unwrap()
        );
        let identities = ["manifest-a", "manifest-b", "manifest-blocker"].map(str::to_owned);
        let forward_settings = build_generator_settings(&forward, &identities).unwrap();
        let reversed_settings = build_generator_settings(&reversed, &identities).unwrap();
        assert_ne!(
            generator_settings_identity(&forward_settings).unwrap(),
            generator_settings_identity(&reversed_settings).unwrap()
        );
    }

    fn identity_matrix() -> Vec<f64> {
        vec![
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]
    }

    fn translated_matrix() -> Vec<f64> {
        vec![
            1.0, 0.0, 0.0, 0.25, 0.0, 1.0, 0.0, -0.5, 0.0, 0.0, 1.0, 1.25, 0.0, 0.0, 0.0, 1.0,
        ]
    }

    fn settings_v2() -> GeneratorSettingsV2 {
        GeneratorSettingsV2 {
            schema_version: SETTINGS_V2_SCHEMA_VERSION,
            blockers: vec![GeneratorSettingsBlocker {
                object_identity: "blocker".to_owned(),
                targets: vec!["printable".to_owned()],
            }],
            placements: vec![
                GeneratorSettingsPlacementV2 {
                    object_identity: "printable".to_owned(),
                    matrix: identity_matrix(),
                },
                GeneratorSettingsPlacementV2 {
                    object_identity: "blocker".to_owned(),
                    matrix: translated_matrix(),
                },
            ],
        }
    }

    fn expected_v2() -> Vec<ExpectedPlacementSummaryV2> {
        vec![
            ExpectedPlacementSummaryV2 {
                object_identity: "printable".to_owned(),
                transport_role: InputRole::RawGeometry,
                expected_neutral_placement_matrix: identity_matrix(),
            },
            ExpectedPlacementSummaryV2 {
                object_identity: "blocker".to_owned(),
                transport_role: InputRole::AuxiliaryGeometry,
                expected_neutral_placement_matrix: translated_matrix(),
            },
        ]
    }

    #[test]
    fn settings_v2_accepts_zero_placements_and_rejects_matrix_failures() {
        validate_generator_settings_v2(&GeneratorSettingsV2 {
            schema_version: SETTINGS_V2_SCHEMA_VERSION,
            blockers: Vec::new(),
            placements: Vec::new(),
        })
        .unwrap();

        for matrix in [vec![0.0; 15], vec![0.0; 17]] {
            let mut invalid = settings_v2();
            invalid.placements[0].matrix = matrix;
            assert!(validate_generator_settings_v2(&invalid).is_err());
        }
        for scalar in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut invalid = settings_v2();
            invalid.placements[0].matrix[0] = scalar;
            assert!(validate_generator_settings_v2(&invalid).is_err());
        }
        for (index, scalar) in [(12, 1.0), (13, 1.0), (14, 1.0), (15, 0.0)] {
            let mut invalid = settings_v2();
            invalid.placements[0].matrix[index] = scalar;
            assert!(validate_generator_settings_v2(&invalid).is_err());
        }
        assert!(
            parse_generator_settings_v2(
                br#"{"schemaVersion":2,"blockers":[],"placements":[{"objectIdentity":"a","matrix":[0,0]}]}"#
            )
            .is_err()
        );
    }

    #[test]
    fn settings_v2_context_fails_closed_on_correspondence_and_scalar_errors() {
        let settings = settings_v2();
        let expected = expected_v2();
        validate_settings_context_v2(&settings, &expected).unwrap();

        assert!(validate_settings_context_v2(&settings, &expected[..1]).is_err());
        let mut extra = expected.clone();
        extra.push(expected[0].clone());
        assert!(validate_settings_context_v2(&settings, &extra).is_err());

        let mut duplicate = settings.clone();
        duplicate.placements[1].object_identity = "printable".to_owned();
        assert!(validate_settings_context_v2(&duplicate, &expected).is_err());

        let mut reordered = settings.clone();
        reordered.placements.reverse();
        assert!(validate_settings_context_v2(&reordered, &expected).is_err());

        let mut wrong_identity = settings.clone();
        wrong_identity.placements[0].object_identity = "other".to_owned();
        assert!(validate_settings_context_v2(&wrong_identity, &expected).is_err());

        let mut wrong_role = expected.clone();
        wrong_role[0].transport_role = InputRole::AuxiliaryGeometry;
        assert!(validate_settings_context_v2(&settings, &wrong_role).is_err());

        let mut scalar_mismatch = expected.clone();
        scalar_mismatch[1].expected_neutral_placement_matrix[3] = 0.5;
        assert!(validate_settings_context_v2(&settings, &scalar_mismatch).is_err());
    }

    #[test]
    fn settings_v2_normalizes_signed_zero_before_all_observable_operations() {
        let positive = settings_v2();
        let mut negative = positive.clone();
        for index in [1, 7, 12, 13, 14] {
            negative.placements[0].matrix[index] = -0.0;
        }
        let parsed = parse_generator_settings_v2(&serde_json::to_vec(&negative).unwrap()).unwrap();
        assert!(
            parsed.placements[0]
                .matrix
                .iter()
                .filter(|scalar| **scalar == 0.0)
                .all(|scalar| scalar.is_sign_positive())
        );
        assert_eq!(normalize_generator_settings_v2(&positive), parsed);
        assert_eq!(
            generator_settings_v2_canonical_json_bytes(&positive).unwrap(),
            generator_settings_v2_canonical_json_bytes(&negative).unwrap()
        );
        assert_eq!(
            generator_settings_v2_identity(&positive).unwrap(),
            generator_settings_v2_identity(&negative).unwrap()
        );

        let mut expected = expected_v2();
        expected[0].expected_neutral_placement_matrix[1] = -0.0;
        expected[0].expected_neutral_placement_matrix[3] = -0.0;
        expected[0].expected_neutral_placement_matrix[12] = -0.0;
        validate_settings_context_v2(&positive, &expected).unwrap();

        let mut opposite_nonzero = positive.clone();
        opposite_nonzero.placements[1].matrix[3] = -0.25;
        assert_ne!(
            generator_settings_v2_identity(&positive).unwrap(),
            generator_settings_v2_identity(&opposite_nonzero).unwrap()
        );
    }

    #[test]
    fn settings_v2_has_golden_schema_document_and_jcs_identities() {
        let settings = settings_v2();
        assert_eq!(
            generator_settings_v2_schema_identity().unwrap(),
            "adfbdd411a8562cd84918ca9facf8c91f9cfeebed1a5cc606f46b2289920e465"
        );
        assert_eq!(
            generator_settings_v2_identity(&settings).unwrap(),
            "80d005dc2fb36187a0112890c7b263fbad18d0641e82ef9addd13a6b73767a89"
        );
        assert_eq!(
            String::from_utf8(generator_settings_v2_canonical_json_bytes(&settings).unwrap())
                .unwrap(),
            "{\"blockers\":[{\"objectIdentity\":\"blocker\",\"targets\":[\"printable\"]}],\"placements\":[{\"matrix\":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1],\"objectIdentity\":\"printable\"},{\"matrix\":[1,0,0,0.25,0,1,0,-0.5,0,0,1,1.25,0,0,0,1],\"objectIdentity\":\"blocker\"}],\"schemaVersion\":2}"
        );
    }
}
