//! Source-neutral construction from trusted acquisition to protocol v1 and
//! settings v2. Declares paths only; staging and execution belong to the runner.

use std::collections::HashSet;

use anyhow::{Context, ensure};
use serde::Serialize;

use crate::{
    cache_key, cache_model,
    catalog::ElementKind,
    generator_protocol::{
        self, ExportInput, FileContent, GeneratorRequest, GroupingPolicy, InputManifest,
        InputObject, ManifestDecision, ManifestDocumentType, ManifestStatus, MappingEvidence,
        MappingStatus, ObjectMapping,
    },
    onshape_annotation::{
        self, AuthoringDocument, AuthoringObject, ExpectedPlacementSummaryV2,
        GeneratorSettingsPlacementV2, GeneratorSettingsV2,
    },
    onshape_geometry::{self, AcquiredGeometry, RetainedPayload, TrustedAcquisitionPlan},
    onshape_selection::SelectionElementKind,
};

pub const RETAINED_PATH_ALLOCATION_VERSION: u32 = 1;

/// Reviewed service policy, not discovered from filenames or interpreted bytes.
/// Input kind/schema compatibility remains the deployed-generator owner's job.
pub struct GeneratorInputPolicy {
    pub requirements_identity: String,
    pub input_kind_identity: String,
    pub input_schema_identity: String,
    pub detected_kind_identity: String,
    pub media_type: String,
}

/// Immutable, complete construction result. Source evidence is service-only;
/// only manifest/settings documents are generator-facing.
pub struct ConstructedGeneratorInputs {
    manifest: InputManifest,
    settings: GeneratorSettingsV2,
    expected_placements: Vec<ExpectedPlacementSummaryV2>,
    settings_identity: String,
    settings_schema_identity: String,
    settings_bytes: Vec<u8>,
    acquired: AcquiredGeometry,
}

impl ConstructedGeneratorInputs {
    pub fn manifest(&self) -> &InputManifest {
        &self.manifest
    }

    pub fn settings(&self) -> &GeneratorSettingsV2 {
        &self.settings
    }

    pub fn expected_placements(&self) -> &[ExpectedPlacementSummaryV2] {
        &self.expected_placements
    }

    pub fn settings_identity(&self) -> &str {
        &self.settings_identity
    }

    pub fn settings_schema_identity(&self) -> &str {
        &self.settings_schema_identity
    }

    pub fn settings_bytes(&self) -> &[u8] {
        &self.settings_bytes
    }

    /// Select by manifest position, never by filename, name, or content equality.
    pub fn payload(&self, position: usize) -> Option<&RetainedPayload> {
        self.acquired
            .bindings
            .get(position)
            .map(|binding| &binding.retained_payload)
    }

    /// Complete ordered source and causal provenance; never serialize as settings.
    pub fn provenance(&self) -> &AcquiredGeometry {
        &self.acquired
    }

    /// Required contextual check when a later owner supplies the invocation.
    /// Does not stage files, dispatch, evaluate compatibility, or create cache rows.
    pub fn validate_request(&self, request: &GeneratorRequest) -> anyhow::Result<()> {
        validate_construction(&self.manifest, &self.settings, &self.expected_placements)?;
        let bytes = cache_key::canonical_json_bytes(request)?;
        generator_protocol::parse_request(&bytes)?;
        request.validate_with_manifest(&self.manifest)?;
        ensure!(
            request.settings.settings_identity == self.settings_identity
                && request.settings.schema_identity == self.settings_schema_identity
                && request.settings.content.sha256 == cache_key::hex_sha256(&self.settings_bytes)
                && request.settings.content.byte_length == self.settings_bytes.len() as u64,
            "request settings do not match the constructed canonical settings"
        );
        Ok(())
    }
}

/// V1 bytes: inputs/geometry-v1/ + exactly three ASCII decimal index digits +
/// '-' + 64 lowercase hexadecimal object-identity digits + '.3mf'. No other input.
pub fn allocate_retained_paths(identities: &[String]) -> anyhow::Result<Vec<String>> {
    ensure!(
        (1..=generator_protocol::MAX_INPUT_OBJECTS).contains(&identities.len()),
        "path allocation requires 1 to 256 manifest objects"
    );
    let mut unique = HashSet::new();
    for identity in identities {
        ensure!(
            crate::onshape_api::is_sha256(identity) && unique.insert(identity),
            "path allocation requires unique lowercase SHA-256 object identities"
        );
    }
    let paths: Vec<_> = identities
        .iter()
        .enumerate()
        .map(|(position, identity)| format!("inputs/geometry-v1/{position:03}-{identity}.3mf"))
        .collect();
    validate_allocated_paths(&paths)?;
    Ok(paths)
}

fn validate_allocated_paths(paths: &[String]) -> anyhow::Result<()> {
    let mut unique = HashSet::new();
    for path in paths {
        generator_protocol::validate_relative_path(path, "retainedContent.path")?;
        ensure!(
            path.starts_with("inputs/") && unique.insert(path),
            "allocated retained paths must be unique and beneath inputs/"
        );
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestObjectIdentity<'a> {
    plan_local_object_identity: &'a str,
}

fn manifest_object_identity(plan_local_object_identity: &str) -> anyhow::Result<String> {
    cache_key::hash_json(
        "onshape-export-manifest-object-v1",
        &ManifestObjectIdentity {
            plan_local_object_identity,
        },
    )
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ContentIdentity<'a> {
    sha256: &'a str,
    byte_length: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ObservationBinding<'a> {
    object_identity: &'a str,
    evidence_identity: &'a str,
}

/// Consumes only a complete successful acquisition, verified against its trusted
/// planning invocation and original retention proof. Failure returns no bundle.
pub fn construct_generator_inputs(
    trusted: &TrustedAcquisitionPlan,
    acquired: AcquiredGeometry,
    policy: &GeneratorInputPolicy,
) -> anyhow::Result<ConstructedGeneratorInputs> {
    onshape_geometry::validate_acquisition_handoff(trusted, &acquired)
        .context("invalid trusted acquisition handoff")?;
    for (field, identity) in [
        ("requirementsIdentity", &policy.requirements_identity),
        ("inputKindIdentity", &policy.input_kind_identity),
        ("inputSchemaIdentity", &policy.input_schema_identity),
        ("detectedKindIdentity", &policy.detected_kind_identity),
    ] {
        generator_protocol::validate_identity(identity, field)?;
    }
    let plan = &acquired.plan;
    let identities = plan
        .objects
        .iter()
        .map(|object| manifest_object_identity(&object.plan_local_object_identity))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let paths = allocate_retained_paths(&identities)?;
    let mut objects = Vec::with_capacity(identities.len());
    for ((object, binding), (identity, path)) in plan
        .objects
        .iter()
        .zip(&acquired.bindings)
        .zip(identities.iter().zip(paths))
    {
        let evidence_identity =
            onshape_geometry::acquisition_evidence_identity(&binding.acquisition_evidence)?;
        let payload = &binding.retained_payload;
        let content_identity = cache_key::hash_json(
            "onshape-export-retained-geometry-content-v1",
            &ContentIdentity {
                sha256: &payload.sha256,
                byte_length: payload.byte_length,
            },
        )?;
        objects.push(InputObject {
            object_identity: identity.clone(),
            role: object.annotation.role.transport_role(),
            retained_content: FileContent {
                content_identity,
                path,
                sha256: payload.sha256.clone(),
                byte_length: payload.byte_length,
                media_type: policy.media_type.clone(),
                detected_kind_identity: policy.detected_kind_identity.clone(),
            },
            mapping: ObjectMapping {
                status: MappingStatus::Proven,
                evidence: Some(MappingEvidence {
                    classification: "immutable-leaf-causal-acquisition-v1".to_owned(),
                    evidence_identity,
                }),
                reason: None,
            },
            source_object_identity: Some(object.plan_local_object_identity.clone()),
            occurrence_path: None,
            producer_result_identity: None,
            source_filename: None,
            display_name: Some(object.display_name.clone()),
            parent_object_identity: None,
        });
    }
    let observation: Vec<_> = objects
        .iter()
        .map(|object| ObservationBinding {
            object_identity: &object.object_identity,
            // Every object was just constructed with proven causal evidence.
            evidence_identity: &object.mapping.evidence.as_ref().unwrap().evidence_identity,
        })
        .collect();
    let observation_identity =
        cache_key::hash_json("onshape-export-geometry-observation-v1", &observation)?;
    let mut manifest = InputManifest {
        document_type: ManifestDocumentType::InputManifest,
        protocol_version: generator_protocol::PROTOCOL_VERSION,
        manifest_version: generator_protocol::INPUT_MANIFEST_VERSION,
        manifest_identity: String::new(),
        input_set_identity: None,
        requirements_identity: policy.requirements_identity.clone(),
        source_identity: cache_model::source_hash(&cache_model::ResolvedOnshapeSourceIdentity {
            document_id: plan.root.document_id.clone(),
            version_id: plan.root.version_id.clone(),
            microversion_id: plan.root.document_microversion.clone(),
            element_id: plan.root.element_id.clone(),
            element_kind: match plan.root.element_kind {
                SelectionElementKind::PartStudio => ElementKind::PartStudio,
                SelectionElementKind::Assembly => ElementKind::Assembly,
            },
            link_document_id: None,
        })?,
        configuration_identity: plan.root.configuration_identity.clone(),
        export: ExportInput {
            kind_identity: policy.input_kind_identity.clone(),
            schema_identity: policy.input_schema_identity.clone(),
            grouping_policy: GroupingPolicy::Individual,
            observation_status: MappingStatus::Proven,
            observation_evidence_identity: Some(observation_identity),
        },
        decision: ManifestDecision {
            status: ManifestStatus::Available,
            reason: None,
        },
        objects,
    };
    manifest.input_set_identity = manifest.computed_input_set_identity()?;
    manifest.manifest_identity = manifest.computed_manifest_identity()?;

    let authoring = AuthoringDocument {
        schema_version: onshape_annotation::SCHEMA_VERSION,
        objects: plan
            .objects
            .iter()
            .map(|object| AuthoringObject {
                selector: object.selector.clone(),
                display_name: object.display_name.clone(),
                annotation: object.annotation.clone(),
            })
            .collect(),
    };
    let blockers = onshape_annotation::build_generator_settings(&authoring, &identities)?.blockers;
    let settings = GeneratorSettingsV2 {
        schema_version: onshape_annotation::SETTINGS_V2_SCHEMA_VERSION,
        blockers,
        placements: plan
            .objects
            .iter()
            .zip(&identities)
            .map(|(object, identity)| GeneratorSettingsPlacementV2 {
                object_identity: identity.clone(),
                matrix: object.expected_neutral_placement_matrix.to_vec(),
            })
            .collect(),
    };
    let expected_placements = plan
        .objects
        .iter()
        .zip(&manifest.objects)
        .map(|(planned, object)| ExpectedPlacementSummaryV2 {
            object_identity: object.object_identity.clone(),
            transport_role: object.role,
            expected_neutral_placement_matrix: planned.expected_neutral_placement_matrix.to_vec(),
        })
        .collect::<Vec<_>>();
    let settings = onshape_annotation::normalize_generator_settings_v2(&settings);
    validate_construction(&manifest, &settings, &expected_placements)?;
    let settings_identity = onshape_annotation::generator_settings_v2_identity(&settings)?;
    let settings_schema_identity = onshape_annotation::generator_settings_v2_schema_identity()?;
    let settings_bytes = onshape_annotation::generator_settings_v2_canonical_json_bytes(&settings)?;
    Ok(ConstructedGeneratorInputs {
        manifest,
        settings,
        expected_placements,
        settings_identity,
        settings_schema_identity,
        settings_bytes,
        acquired,
    })
}

fn validate_construction(
    manifest: &InputManifest,
    settings: &GeneratorSettingsV2,
    expected: &[ExpectedPlacementSummaryV2],
) -> anyhow::Result<()> {
    // Parsing the canonical document also enforces protocol-v1 document bounds.
    generator_protocol::parse_input_manifest(&cache_key::canonical_json_bytes(manifest)?)?;
    ensure!(
        manifest.objects.len() == expected.len()
            && manifest
                .objects
                .iter()
                .zip(expected)
                .all(|(object, entry)| {
                    object.object_identity == entry.object_identity
                        && object.role == entry.transport_role
                }),
        "expected placement summary does not match manifest order, identity, and role"
    );
    onshape_annotation::validate_generator_settings_v2(settings)?;
    onshape_annotation::validate_settings_context_v2(settings, expected)?;
    Ok(())
}

#[cfg(test)]
#[path = "generator_inputs_tests.rs"]
mod tests;
