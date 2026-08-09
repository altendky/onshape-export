use anyhow::{Context, ensure};
use serde::Serialize;

use crate::{
    cache_key,
    cache_model::{self, ARTIFACT_SET_SCHEMA_VERSION, ArtifactSetIdentity},
    deployed_generator::{
        DeployedGenerator, DeployedGeneratorError, GeneratorCompatibilityRequest,
    },
    generator_protocol::{
        FileContent, GeneratorRequest, InputManifest, ManifestReference, ManifestStatus,
        OutputDeclaration, RequestDocumentType, SettingsInput,
    },
    onshape_annotation::{
        ExpectedPlacementSummaryV2, GeneratorSettingsV2, generator_settings_v2_identity,
        generator_settings_v2_schema_identity, normalize_generator_settings_v2,
        validate_settings_context_v2,
    },
};

pub const GENERATOR_PROCESSING_RECIPE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratorCompatibilityIdentity {
    pub protocol_version: u32,
    pub dialect_identity: String,
    pub capability_identities: Vec<String>,
    pub input_kind_identity: String,
    pub input_schema_identity: String,
    pub settings_schema_identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum GeneratorCompatibilityDecision {
    Supported,
    Unsupported { field: String },
}

impl GeneratorCompatibilityDecision {
    pub fn status(&self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Unsupported { .. } => "unsupported",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratorProcessingRecipe {
    pub recipe_version: u32,
    pub deployed_generator_identity: String,
    pub compatibility: GeneratorCompatibilityIdentity,
    pub compatibility_decision: GeneratorCompatibilityDecision,
    pub compatibility_decision_identity: String,
    pub input_manifest: InputManifest,
    pub settings: GeneratorSettingsV2,
    pub settings_identity: String,
    pub settings_schema_identity: String,
    pub invocation: GeneratorRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratorProcessingProtocolInputs {
    pub manifest_path: String,
    pub settings_content_identity: String,
    pub settings_path: String,
    pub settings_media_type: String,
    pub settings_detected_kind_identity: String,
    pub output: OutputDeclaration,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GeneratorProcessingOccurrence {
    pub occurrence_identity: String,
    pub occurrence_order: usize,
    pub object_identity: String,
    pub content_identity: String,
    pub content_sha256: String,
    pub content_byte_length: u64,
    pub staged_path: String,
    pub transport_role: String,
    pub display_name: Option<String>,
    pub mapping_json: String,
    pub provenance_json: String,
    pub placement_json: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PreparedGeneratorProcessing {
    processing_hash: String,
    recipe_json: String,
    recipe: GeneratorProcessingRecipe,
    occurrences: Vec<GeneratorProcessingOccurrence>,
}

impl PreparedGeneratorProcessing {
    pub fn processing_hash(&self) -> &str {
        &self.processing_hash
    }

    pub fn recipe(&self) -> &GeneratorProcessingRecipe {
        &self.recipe
    }

    pub fn occurrences(&self) -> &[GeneratorProcessingOccurrence] {
        &self.occurrences
    }

    pub(crate) fn recipe_json(&self) -> &str {
        &self.recipe_json
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CompatibilityDecisionIdentityPayload<'a> {
    compatibility: &'a GeneratorCompatibilityIdentity,
    decision: &'a GeneratorCompatibilityDecision,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OccurrenceIdentityPayload<'a> {
    processing_hash: &'a str,
    occurrence_order: usize,
    object_identity: &'a str,
    staged_path: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OccurrenceProvenance<'a> {
    source_object_identity: &'a Option<String>,
    occurrence_path: &'a Option<Vec<String>>,
    producer_result_identity: &'a Option<String>,
    source_filename: &'a Option<String>,
    parent_object_identity: &'a Option<String>,
}

pub fn prepare_generator_processing(
    generator: &DeployedGenerator,
    compatibility: &GeneratorCompatibilityRequest,
    manifest: &InputManifest,
    settings: &GeneratorSettingsV2,
    expected_placements: &[ExpectedPlacementSummaryV2],
    protocol_inputs: &GeneratorProcessingProtocolInputs,
) -> anyhow::Result<PreparedGeneratorProcessing> {
    manifest
        .validate()
        .context("invalid generator input manifest")?;
    ensure!(
        manifest.decision.status == ManifestStatus::Available,
        "generator processing requires an available input manifest"
    );
    ensure!(
        manifest.export.kind_identity == compatibility.input_kind_identity,
        "manifest input kind does not match the compatibility request"
    );
    ensure!(
        manifest.export.schema_identity == compatibility.input_schema_identity,
        "manifest input schema does not match the compatibility request"
    );

    let normalized_settings = normalize_generator_settings_v2(settings);
    ensure!(
        expected_placements.len() == manifest.objects.len()
            && expected_placements
                .iter()
                .zip(&manifest.objects)
                .all(
                    |(expected, object)| expected.object_identity == object.object_identity
                        && expected.transport_role == object.role
                ),
        "expected placements must match manifest object identity, role, and order"
    );
    validate_settings_context_v2(&normalized_settings, expected_placements)
        .context("invalid generator settings context")?;
    let settings_schema_identity =
        generator_settings_v2_schema_identity().context("could not identify settings schema")?;
    ensure!(
        settings_schema_identity == compatibility.settings_schema_identity,
        "implemented settings schema does not match the compatibility request"
    );
    let settings_identity = generator_settings_v2_identity(&normalized_settings)
        .context("could not identify generator settings")?;
    let settings_bytes =
        crate::onshape_annotation::generator_settings_v2_canonical_json_bytes(&normalized_settings)
            .context("could not serialize generator settings")?;

    let compatibility_identity = GeneratorCompatibilityIdentity {
        protocol_version: compatibility.protocol_version,
        dialect_identity: compatibility.dialect_identity.clone(),
        capability_identities: compatibility.capability_identities.clone(),
        input_kind_identity: compatibility.input_kind_identity.clone(),
        input_schema_identity: compatibility.input_schema_identity.clone(),
        settings_schema_identity: compatibility.settings_schema_identity.clone(),
    };
    let compatibility_decision = match generator.ensure_compatible(compatibility) {
        Ok(()) => GeneratorCompatibilityDecision::Supported,
        Err(DeployedGeneratorError::UnsupportedCombination { field }) => {
            GeneratorCompatibilityDecision::Unsupported {
                field: field.to_owned(),
            }
        }
        Err(error) => return Err(error).context("could not evaluate generator compatibility"),
    };
    let compatibility_decision_identity = cache_key::hash_json(
        "generator-compatibility-decision-v1",
        &CompatibilityDecisionIdentityPayload {
            compatibility: &compatibility_identity,
            decision: &compatibility_decision,
        },
    )?;

    let input_set_identity = manifest
        .input_set_identity
        .clone()
        .context("available manifest has no input-set identity")?;
    let mut invocation = GeneratorRequest {
        document_type: RequestDocumentType::GeneratorRequest,
        protocol_version: compatibility.protocol_version,
        invocation_identity: String::new(),
        expected_identities: generator.identity_bindings(settings_identity.clone()),
        input_manifest: ManifestReference {
            path: protocol_inputs.manifest_path.clone(),
            manifest_identity: manifest.manifest_identity.clone(),
            input_set_identity,
        },
        settings: SettingsInput {
            settings_identity: settings_identity.clone(),
            schema_identity: settings_schema_identity.clone(),
            content: FileContent {
                content_identity: protocol_inputs.settings_content_identity.clone(),
                path: protocol_inputs.settings_path.clone(),
                sha256: cache_key::hex_sha256(&settings_bytes),
                byte_length: settings_bytes.len().try_into()?,
                media_type: protocol_inputs.settings_media_type.clone(),
                detected_kind_identity: protocol_inputs.settings_detected_kind_identity.clone(),
            },
        },
        output: protocol_inputs.output.clone(),
    };
    invocation.invocation_identity = invocation
        .computed_invocation_identity()
        .context("could not identify generator invocation")?;
    invocation
        .validate_with_manifest(manifest)
        .context("invalid generator invocation")?;

    let recipe = GeneratorProcessingRecipe {
        recipe_version: GENERATOR_PROCESSING_RECIPE_VERSION,
        deployed_generator_identity: generator.static_identity().to_owned(),
        compatibility: compatibility_identity,
        compatibility_decision,
        compatibility_decision_identity,
        input_manifest: manifest.clone(),
        settings: normalized_settings,
        settings_identity,
        settings_schema_identity,
        invocation,
    };
    let processing_hash = generator_processing_hash(&recipe)?;
    let recipe_json = String::from_utf8(cache_key::canonical_json_bytes(&recipe)?)
        .expect("canonical JSON is UTF-8");
    let occurrences = derive_occurrences(&processing_hash, &recipe)?;

    Ok(PreparedGeneratorProcessing {
        processing_hash,
        recipe_json,
        recipe,
        occurrences,
    })
}

pub fn generator_processing_hash(recipe: &GeneratorProcessingRecipe) -> anyhow::Result<String> {
    cache_key::hash_json("generator-processing-recipe-v1", recipe)
}

fn derive_occurrences(
    processing_hash: &str,
    recipe: &GeneratorProcessingRecipe,
) -> anyhow::Result<Vec<GeneratorProcessingOccurrence>> {
    recipe
        .input_manifest
        .objects
        .iter()
        .zip(&recipe.settings.placements)
        .enumerate()
        .map(|(occurrence_order, (object, placement))| {
            let occurrence_identity = cache_key::hash_json(
                "generator-processing-occurrence-v1",
                &OccurrenceIdentityPayload {
                    processing_hash,
                    occurrence_order,
                    object_identity: &object.object_identity,
                    staged_path: &object.retained_content.path,
                },
            )?;
            Ok(GeneratorProcessingOccurrence {
                occurrence_identity,
                occurrence_order,
                object_identity: object.object_identity.clone(),
                content_identity: object.retained_content.content_identity.clone(),
                content_sha256: object.retained_content.sha256.clone(),
                content_byte_length: object.retained_content.byte_length,
                staged_path: object.retained_content.path.clone(),
                transport_role: json_string(&object.role)?,
                display_name: object.display_name.clone(),
                mapping_json: canonical_json_string(&object.mapping)?,
                provenance_json: canonical_json_string(&OccurrenceProvenance {
                    source_object_identity: &object.source_object_identity,
                    occurrence_path: &object.occurrence_path,
                    producer_result_identity: &object.producer_result_identity,
                    source_filename: &object.source_filename,
                    parent_object_identity: &object.parent_object_identity,
                })?,
                placement_json: canonical_json_string(placement)?,
            })
        })
        .collect()
}

fn canonical_json_string<T: Serialize + ?Sized>(value: &T) -> anyhow::Result<String> {
    Ok(
        String::from_utf8(cache_key::canonical_json_bytes(value)?)
            .expect("canonical JSON is UTF-8"),
    )
}

fn json_string<T: Serialize>(value: &T) -> anyhow::Result<String> {
    let value = serde_json::to_value(value)?;
    value
        .as_str()
        .map(ToOwned::to_owned)
        .context("expected a string serialization")
}

pub fn generator_artifact_set_identity(
    prepared: &PreparedGeneratorProcessing,
    output_kind: impl Into<String>,
    format: impl Into<String>,
) -> ArtifactSetIdentity {
    ArtifactSetIdentity {
        artifact_set_schema_version: ARTIFACT_SET_SCHEMA_VERSION,
        output_kind: output_kind.into(),
        format: format.into(),
        source_hash: prepared.recipe.input_manifest.source_identity.clone(),
        config_hash: prepared
            .recipe
            .input_manifest
            .configuration_identity
            .clone(),
        options_hash: prepared.recipe.settings_identity.clone(),
        request_hash: None,
        raw_payload_hash: None,
        postprocess_hash: prepared.processing_hash.clone(),
        generator_processing_hash: Some(prepared.processing_hash.clone()),
    }
}

pub fn generator_artifact_set_hash(
    prepared: &PreparedGeneratorProcessing,
    output_kind: impl Into<String>,
    format: impl Into<String>,
) -> anyhow::Result<String> {
    cache_model::artifact_set_hash(&generator_artifact_set_identity(
        prepared,
        output_kind,
        format,
    ))
}

#[cfg(test)]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt, path::Path};

    use serde_json::json;

    use super::*;
    use crate::{
        deployed_generator::DeployedGeneratorDocument,
        generator_protocol::{
            ExportInput, FileContent, GroupingPolicy, InputObject, InputRole, ManifestDecision,
            ManifestDocumentType, MappingEvidence, MappingStatus, ObjectMapping, OutputRole,
            PROTOCOL_VERSION,
        },
        onshape_annotation::{GeneratorSettingsBlocker, GeneratorSettingsPlacementV2},
    };

    const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn write_executable(directory: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = directory.join(name);
        fs::write(&path, bytes).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
        path
    }

    fn generator_document(executable_path: &Path, bytes: &[u8]) -> DeployedGeneratorDocument {
        DeployedGeneratorDocument {
            executable_path: executable_path.to_owned(),
            package_identity: "package-v1".to_owned(),
            package_sha256: SHA_A.to_owned(),
            build_identity: "build-v1".to_owned(),
            binary_identity: "binary-v1".to_owned(),
            binary_sha256: cache_key::hex_sha256(bytes),
            protocol_version: PROTOCOL_VERSION,
            dialect_identity: "dialect-v1".to_owned(),
            capability_identities: vec!["capability-v1".to_owned()],
            input_kind_identity: "input-kind-v1".to_owned(),
            input_schema_identity: "input-schema-v1".to_owned(),
            settings_schema_identity: generator_settings_v2_schema_identity().unwrap(),
            provenance_set_identity: "provenance-v1".to_owned(),
            normalization_identity: "normalization-v1".to_owned(),
            validation_identity: "validation-v1".to_owned(),
        }
    }

    fn load_generator(document: &DeployedGeneratorDocument) -> DeployedGenerator {
        DeployedGenerator::from_bytes(&serde_json::to_vec(document).unwrap()).unwrap()
    }

    fn compatibility(document: &DeployedGeneratorDocument) -> GeneratorCompatibilityRequest {
        GeneratorCompatibilityRequest {
            protocol_version: document.protocol_version,
            dialect_identity: document.dialect_identity.clone(),
            capability_identities: document.capability_identities.clone(),
            input_kind_identity: document.input_kind_identity.clone(),
            input_schema_identity: document.input_schema_identity.clone(),
            settings_schema_identity: document.settings_schema_identity.clone(),
        }
    }

    fn input_object(identity: &str, path: &str, role: InputRole) -> InputObject {
        InputObject {
            object_identity: identity.to_owned(),
            role,
            retained_content: FileContent {
                content_identity: "shared-content-v1".to_owned(),
                path: path.to_owned(),
                sha256: SHA_B.to_owned(),
                byte_length: 42,
                media_type: "application/octet-stream".to_owned(),
                detected_kind_identity: "neutral-kind-v1".to_owned(),
            },
            mapping: ObjectMapping {
                status: MappingStatus::Proven,
                evidence: Some(MappingEvidence {
                    classification: "synthetic-proof".to_owned(),
                    evidence_identity: "evidence-v1".to_owned(),
                }),
                reason: None,
            },
            source_object_identity: Some(format!("source-{identity}")),
            occurrence_path: Some(vec![format!("occurrence-{identity}")]),
            producer_result_identity: Some("producer-v1".to_owned()),
            source_filename: Some(format!("{identity}.bin")),
            display_name: Some(format!("Synthetic {identity}")),
            parent_object_identity: None,
        }
    }

    fn manifest() -> InputManifest {
        let mut manifest = InputManifest {
            document_type: ManifestDocumentType::InputManifest,
            protocol_version: PROTOCOL_VERSION,
            manifest_version: 1,
            manifest_identity: SHA_A.to_owned(),
            input_set_identity: None,
            requirements_identity: "requirements-v1".to_owned(),
            source_identity: "source-v1".to_owned(),
            configuration_identity: "configuration-v1".to_owned(),
            export: ExportInput {
                kind_identity: "input-kind-v1".to_owned(),
                schema_identity: "input-schema-v1".to_owned(),
                grouping_policy: GroupingPolicy::Grouped,
                observation_status: MappingStatus::Proven,
                observation_evidence_identity: Some("observation-v1".to_owned()),
            },
            decision: ManifestDecision {
                status: ManifestStatus::Available,
                reason: None,
            },
            objects: vec![
                input_object("object-a", "inputs/a.bin", InputRole::RawGeometry),
                input_object("object-b", "inputs/b.bin", InputRole::AuxiliaryGeometry),
            ],
        };
        manifest.input_set_identity = manifest.computed_input_set_identity().unwrap();
        manifest.manifest_identity = manifest.computed_manifest_identity().unwrap();
        manifest
    }

    fn matrix(translation: f64) -> Vec<f64> {
        vec![
            1.0,
            0.0,
            0.0,
            translation,
            0.0,
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
        ]
    }

    fn settings() -> GeneratorSettingsV2 {
        GeneratorSettingsV2 {
            schema_version: 2,
            blockers: vec![GeneratorSettingsBlocker {
                object_identity: "object-b".to_owned(),
                targets: vec!["object-a".to_owned()],
            }],
            placements: vec![
                GeneratorSettingsPlacementV2 {
                    object_identity: "object-a".to_owned(),
                    matrix: matrix(1.0),
                },
                GeneratorSettingsPlacementV2 {
                    object_identity: "object-b".to_owned(),
                    matrix: matrix(2.0),
                },
            ],
        }
    }

    fn expected(settings: &GeneratorSettingsV2) -> Vec<ExpectedPlacementSummaryV2> {
        settings
            .placements
            .iter()
            .enumerate()
            .map(|(index, placement)| ExpectedPlacementSummaryV2 {
                object_identity: placement.object_identity.clone(),
                transport_role: if index == 0 {
                    InputRole::RawGeometry
                } else {
                    InputRole::AuxiliaryGeometry
                },
                expected_neutral_placement_matrix: placement.matrix.clone(),
            })
            .collect()
    }

    fn protocol_inputs() -> GeneratorProcessingProtocolInputs {
        GeneratorProcessingProtocolInputs {
            manifest_path: "inputs/manifest.json".to_owned(),
            settings_content_identity: "settings-content-v1".to_owned(),
            settings_path: "inputs/settings.json".to_owned(),
            settings_media_type: "application/json".to_owned(),
            settings_detected_kind_identity: "generator-settings-v2".to_owned(),
            output: OutputDeclaration {
                output_identity: "synthetic-project-v1".to_owned(),
                role: OutputRole::GeneratedProject,
                path: "outputs/project.3mf".to_owned(),
                media_type: "application/octet-stream".to_owned(),
                max_byte_length: 1_000_000,
            },
        }
    }

    #[test]
    fn recipe_is_stable_and_artifact_identity_uses_only_the_recipe_for_processing() {
        let directory = tempfile::tempdir().unwrap();
        let bytes = b"synthetic generator";
        let path = write_executable(directory.path(), "generator", bytes);
        let document = generator_document(&path, bytes);
        let generator = load_generator(&document);
        let manifest = manifest();
        let settings = settings();

        let first = prepare_generator_processing(
            &generator,
            &compatibility(&document),
            &manifest,
            &settings,
            &expected(&settings),
            &protocol_inputs(),
        )
        .unwrap();
        let second = prepare_generator_processing(
            &generator,
            &compatibility(&document),
            &manifest,
            &settings,
            &expected(&settings),
            &protocol_inputs(),
        )
        .unwrap();

        assert_eq!(first, second);
        assert_eq!(first.occurrences.len(), 2);
        assert_eq!(
            first.occurrences[0].content_sha256,
            first.occurrences[1].content_sha256
        );
        assert_ne!(
            first.occurrences[0].occurrence_identity,
            first.occurrences[1].occurrence_identity
        );
        assert_ne!(
            first.occurrences[0].staged_path,
            first.occurrences[1].staged_path
        );
        let artifact = generator_artifact_set_identity(&first, "slicer_project", "project_3mf");
        assert_eq!(artifact.request_hash, None);
        assert_eq!(artifact.raw_payload_hash, None);
        assert_eq!(artifact.postprocess_hash, first.processing_hash);
        assert_eq!(
            artifact.generator_processing_hash,
            Some(first.processing_hash.clone())
        );
        assert_eq!(
            generator_artifact_set_hash(&first, "slicer_project", "project_3mf").unwrap(),
            cache_model::artifact_set_hash(&artifact).unwrap()
        );

        let mut mismatched_expected = expected(&settings);
        mismatched_expected.swap(0, 1);
        assert!(
            prepare_generator_processing(
                &generator,
                &compatibility(&document),
                &manifest,
                &settings,
                &mismatched_expected,
                &protocol_inputs(),
            )
            .unwrap_err()
            .to_string()
            .contains("must match manifest")
        );
    }

    #[test]
    fn executable_path_is_excluded_but_package_and_binary_digests_are_bound() {
        let directory = tempfile::tempdir().unwrap();
        let bytes = b"same synthetic generator";
        let first_path = write_executable(directory.path(), "first", bytes);
        let second_path = write_executable(directory.path(), "second", bytes);
        let first_document = generator_document(&first_path, bytes);
        let second_document = generator_document(&second_path, bytes);
        let first_generator = load_generator(&first_document);
        let second_generator = load_generator(&second_document);
        let manifest = manifest();
        let settings = settings();
        let prepare = |generator: &DeployedGenerator, document: &DeployedGeneratorDocument| {
            prepare_generator_processing(
                generator,
                &compatibility(document),
                &manifest,
                &settings,
                &expected(&settings),
                &protocol_inputs(),
            )
            .unwrap()
        };

        assert_eq!(
            prepare(&first_generator, &first_document).processing_hash,
            prepare(&second_generator, &second_document).processing_hash
        );

        let mut package_changed = first_document.clone();
        package_changed.package_sha256 = SHA_B.to_owned();
        assert_ne!(
            prepare(&first_generator, &first_document).processing_hash,
            prepare(&load_generator(&package_changed), &package_changed).processing_hash
        );

        let changed_bytes = b"changed synthetic generator";
        let changed_path = write_executable(directory.path(), "changed", changed_bytes);
        let binary_changed = generator_document(&changed_path, changed_bytes);
        assert_ne!(
            prepare(&first_generator, &first_document).processing_hash,
            prepare(&load_generator(&binary_changed), &binary_changed).processing_hash
        );
    }

    #[test]
    fn recipe_hash_binds_manifest_settings_decision_and_order() {
        let directory = tempfile::tempdir().unwrap();
        let bytes = b"synthetic generator";
        let path = write_executable(directory.path(), "generator", bytes);
        let document = generator_document(&path, bytes);
        let generator = load_generator(&document);
        let manifest = manifest();
        let settings = settings();
        let prepared = prepare_generator_processing(
            &generator,
            &compatibility(&document),
            &manifest,
            &settings,
            &expected(&settings),
            &protocol_inputs(),
        )
        .unwrap();
        let baseline = prepared.processing_hash;
        let changed_hash = |mutate: fn(&mut GeneratorProcessingRecipe)| {
            let mut recipe = prepared.recipe.clone();
            mutate(&mut recipe);
            generator_processing_hash(&recipe).unwrap()
        };

        let mutations: Vec<fn(&mut GeneratorProcessingRecipe)> = vec![
            |recipe| recipe.recipe_version += 1,
            |recipe| recipe.deployed_generator_identity.push('x'),
            |recipe| recipe.compatibility.protocol_version += 1,
            |recipe| recipe.compatibility.dialect_identity.push('x'),
            |recipe| {
                recipe
                    .compatibility
                    .capability_identities
                    .push("capability-v2".to_owned())
            },
            |recipe| recipe.compatibility.input_kind_identity.push('x'),
            |recipe| recipe.compatibility.input_schema_identity.push('x'),
            |recipe| recipe.compatibility.settings_schema_identity.push('x'),
            |recipe| {
                recipe.compatibility_decision = GeneratorCompatibilityDecision::Unsupported {
                    field: "dialectIdentity".to_owned(),
                }
            },
            |recipe| recipe.compatibility_decision_identity.push('x'),
            |recipe| recipe.input_manifest.protocol_version += 1,
            |recipe| recipe.input_manifest.manifest_version += 1,
            |recipe| recipe.input_manifest.manifest_identity.push('x'),
            |recipe| {
                recipe
                    .input_manifest
                    .input_set_identity
                    .as_mut()
                    .unwrap()
                    .push('x')
            },
            |recipe| recipe.input_manifest.requirements_identity.push('x'),
            |recipe| recipe.input_manifest.source_identity.push('x'),
            |recipe| recipe.input_manifest.configuration_identity.push('x'),
            |recipe| recipe.input_manifest.export.kind_identity.push('x'),
            |recipe| recipe.input_manifest.export.schema_identity.push('x'),
            |recipe| recipe.input_manifest.export.grouping_policy = GroupingPolicy::Individual,
            |recipe| recipe.input_manifest.export.observation_status = MappingStatus::Unproven,
            |recipe| {
                recipe
                    .input_manifest
                    .export
                    .observation_evidence_identity
                    .as_mut()
                    .unwrap()
                    .push('x')
            },
            |recipe| recipe.input_manifest.decision.status = ManifestStatus::Unavailable,
            |recipe| recipe.input_manifest.decision.reason = Some("synthetic reason".to_owned()),
            |recipe| recipe.input_manifest.objects[0].object_identity.push('x'),
            |recipe| recipe.input_manifest.objects[0].role = InputRole::AuxiliaryGeometry,
            |recipe| {
                recipe.input_manifest.objects[0]
                    .retained_content
                    .content_identity
                    .push('x')
            },
            |recipe| {
                recipe.input_manifest.objects[0]
                    .retained_content
                    .path
                    .push('x')
            },
            |recipe| recipe.input_manifest.objects[0].retained_content.sha256 = SHA_A.to_owned(),
            |recipe| {
                recipe.input_manifest.objects[0]
                    .retained_content
                    .byte_length += 1
            },
            |recipe| {
                recipe.input_manifest.objects[0]
                    .retained_content
                    .media_type
                    .push('x')
            },
            |recipe| {
                recipe.input_manifest.objects[0]
                    .retained_content
                    .detected_kind_identity
                    .push('x')
            },
            |recipe| recipe.input_manifest.objects[0].mapping.status = MappingStatus::Unproven,
            |recipe| {
                recipe.input_manifest.objects[0]
                    .mapping
                    .evidence
                    .as_mut()
                    .unwrap()
                    .classification
                    .push('x')
            },
            |recipe| {
                recipe.input_manifest.objects[0]
                    .mapping
                    .evidence
                    .as_mut()
                    .unwrap()
                    .evidence_identity
                    .push('x')
            },
            |recipe| {
                recipe.input_manifest.objects[0].mapping.reason =
                    Some("synthetic reason".to_owned())
            },
            |recipe| {
                recipe.input_manifest.objects[0]
                    .source_object_identity
                    .as_mut()
                    .unwrap()
                    .push('x')
            },
            |recipe| {
                recipe.input_manifest.objects[0]
                    .occurrence_path
                    .as_mut()
                    .unwrap()
                    .push("nested".to_owned())
            },
            |recipe| {
                recipe.input_manifest.objects[0]
                    .producer_result_identity
                    .as_mut()
                    .unwrap()
                    .push('x')
            },
            |recipe| {
                recipe.input_manifest.objects[0]
                    .source_filename
                    .as_mut()
                    .unwrap()
                    .push('x')
            },
            |recipe| {
                recipe.input_manifest.objects[0]
                    .display_name
                    .as_mut()
                    .unwrap()
                    .push('x')
            },
            |recipe| {
                recipe.input_manifest.objects[0].parent_object_identity =
                    Some("object-b".to_owned())
            },
            |recipe| recipe.input_manifest.objects.swap(0, 1),
            |recipe| recipe.settings.schema_version += 1,
            |recipe| recipe.settings.blockers[0].targets[0].push('x'),
            |recipe| recipe.settings.blockers[0].object_identity.push('x'),
            |recipe| {
                recipe.settings.blockers.push(GeneratorSettingsBlocker {
                    object_identity: "object-c".to_owned(),
                    targets: vec!["object-a".to_owned()],
                });
                recipe.settings.blockers.swap(0, 1);
            },
            |recipe| recipe.settings.placements[0].object_identity.push('x'),
            |recipe| recipe.settings.placements[0].matrix[3] += 1.0,
            |recipe| recipe.settings.placements.swap(0, 1),
            |recipe| recipe.settings_identity.push('x'),
            |recipe| recipe.settings_schema_identity.push('x'),
            |recipe| recipe.invocation.invocation_identity.push('x'),
            |recipe| recipe.invocation.input_manifest.path.push('x'),
            |recipe| {
                recipe
                    .invocation
                    .settings
                    .content
                    .content_identity
                    .push('x')
            },
            |recipe| recipe.invocation.settings.content.path.push('x'),
            |recipe| recipe.invocation.settings.content.sha256 = SHA_A.to_owned(),
            |recipe| recipe.invocation.settings.content.byte_length += 1,
            |recipe| recipe.invocation.settings.content.media_type.push('x'),
            |recipe| {
                recipe
                    .invocation
                    .settings
                    .content
                    .detected_kind_identity
                    .push('x')
            },
            |recipe| recipe.invocation.output.output_identity.push('x'),
            |recipe| recipe.invocation.output.path.push('x'),
            |recipe| recipe.invocation.output.media_type.push('x'),
            |recipe| recipe.invocation.output.max_byte_length += 1,
        ];
        for mutate in mutations {
            assert_ne!(baseline, changed_hash(mutate));
        }

        let mut unsupported = compatibility(&document);
        unsupported.dialect_identity = "unsupported-dialect".to_owned();
        let unsupported = prepare_generator_processing(
            &generator,
            &unsupported,
            &manifest,
            &settings,
            &expected(&settings),
            &protocol_inputs(),
        )
        .unwrap();
        assert!(matches!(
            unsupported.recipe.compatibility_decision,
            GeneratorCompatibilityDecision::Unsupported { .. }
        ));
        assert_ne!(baseline, unsupported.processing_hash);
        assert!(unsupported.recipe_json.contains("unsupported"));
        assert!(serde_json::from_str::<serde_json::Value>(&prepared.recipe_json).is_ok());
        assert_ne!(json!(prepared.recipe), json!(unsupported.recipe));
    }

    #[tokio::test]
    async fn prepared_recipe_persists_idempotently_through_the_typed_boundary() {
        let directory = tempfile::tempdir().unwrap();
        let bytes = b"synthetic generator";
        let path = write_executable(directory.path(), "generator", bytes);
        let document = generator_document(&path, bytes);
        let generator = load_generator(&document);
        let manifest = manifest();
        let settings = settings();
        let prepared = prepare_generator_processing(
            &generator,
            &compatibility(&document),
            &manifest,
            &settings,
            &expected(&settings),
            &protocol_inputs(),
        )
        .unwrap();
        let database_path = directory.path().join("cache.db");
        let database_url = format!("sqlite://{}?mode=rwc", database_path.display());
        let database = crate::db::Database::connect(&database_url).await.unwrap();

        assert!(
            database
                .insert_generator_processing_recipe(&prepared)
                .await
                .unwrap()
        );
        assert!(
            !database
                .insert_generator_processing_recipe(&prepared)
                .await
                .unwrap()
        );
    }
}
