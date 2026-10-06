//! Trusted same-invocation provenance and opaque immutable-leaf Geometry 3MF
//! acquisition. No geometry interpretation, protocol paths, or generator calls.

use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::{Seek, SeekFrom, Write},
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::time::Instant;

use crate::{
    cache_key,
    cache_model::ResolvedOnshapeSourceIdentity,
    catalog::{ElementKind, OnshapeSource},
    configuration_encoding::{self, EncodingContext, EncodingHandoff},
    db::Database,
    onshape_annotation::AuthoringSelector,
    onshape_api::{self, ApiFailure, FailureKind, OnshapeApi},
    onshape_selection::{
        self, ConfiguredLeaf, ResolvedSelectionPlan, SelectionElementKind, SelectionRequest,
        SelectionSelector,
    },
    parameters::CanonicalParameterValue,
};

const RECORD_LIMIT: usize = 16_777_216;
const CONTRACT: &str = "onshape-export-configured-leaf-geometry-request-v1";
const PROVENANCE_DOMAIN: &str = "onshape-export-acquisition-provenance-v1";

fn invariant(code: &str) -> ApiFailure {
    ApiFailure::new(FailureKind::OperationalApiContractFailure, code)
        .operation("acquisitionProvenance")
}

fn invalid() -> ApiFailure {
    ApiFailure::new(FailureKind::InvalidSelection, "invalid_acquisition_handoff")
}

fn timeout() -> ApiFailure {
    ApiFailure::new(
        FailureKind::OperationalTimeoutFailure,
        "acquisition_deadline",
    )
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum Representation {
    #[serde(rename = "part_studio_original_encoding_v1")]
    PartStudioOriginalEncoding,
    #[serde(rename = "assembly_leaf_full_configuration_v1")]
    AssemblyLeafFullConfiguration,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProvenanceBinding {
    position: usize,
    plan_local_object_identity: String,
    selector: AuthoringSelector,
    configured_leaf: ConfiguredLeaf,
    representation: Representation,
    configuration_request_value: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProvenancePayload {
    provenance_schema_version: u32,
    request_contract: String,
    configuration_encoding: EncodingHandoff,
    encoding_context: EncodingContext,
    plan: ResolvedSelectionPlan,
    bindings: Vec<ProvenanceBinding>,
}

// Do not flatten: serde's flatten/deny_unknown_fields interaction must not weaken
// the closed stored record. Identity is excluded explicitly by payload().
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProvenanceRecord {
    provenance_schema_version: u32,
    request_contract: String,
    configuration_encoding: EncodingHandoff,
    encoding_context: EncodingContext,
    plan: ResolvedSelectionPlan,
    bindings: Vec<ProvenanceBinding>,
    provenance_identity: String,
}

impl ProvenanceRecord {
    fn payload(&self) -> ProvenancePayload {
        ProvenancePayload {
            provenance_schema_version: self.provenance_schema_version,
            request_contract: self.request_contract.clone(),
            configuration_encoding: self.configuration_encoding.clone(),
            encoding_context: self.encoding_context.clone(),
            plan: self.plan.clone(),
            bindings: self.bindings.clone(),
        }
    }

    fn identity(&self) -> Result<String, ApiFailure> {
        #[derive(Serialize)]
        struct Envelope {
            domain: &'static str,
            payload: ProvenancePayload,
        }
        Ok(cache_key::hex_sha256(&bounded_canonical(&Envelope {
            domain: PROVENANCE_DOMAIN,
            payload: self.payload(),
        })?))
    }
}

struct BoundedBytes(Vec<u8>);
impl Write for BoundedBytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > RECORD_LIMIT.saturating_sub(self.0.len()) {
            return Err(std::io::Error::other("acquisition record bound"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn bounded_canonical(value: &impl Serialize) -> Result<Vec<u8>, ApiFailure> {
    let mut bytes = BoundedBytes(Vec::new());
    serde_jcs::to_writer(&mut bytes, value)
        .map_err(|_| invariant("provenance_canonicalization"))?;
    // The same bounds apply to internal and reopened serialized records.
    onshape_api::parse_json(&bytes.0, RECORD_LIMIT)
        .map_err(|_| invariant("provenance_structure"))?;
    Ok(bytes.0)
}

fn source(plan: &ResolvedSelectionPlan) -> ResolvedOnshapeSourceIdentity {
    ResolvedOnshapeSourceIdentity {
        document_id: plan.root.document_id.clone(),
        version_id: plan.root.version_id.clone(),
        microversion_id: plan.root.document_microversion.clone(),
        element_id: plan.root.element_id.clone(),
        element_kind: match plan.root.element_kind {
            SelectionElementKind::PartStudio => ElementKind::PartStudio,
            SelectionElementKind::Assembly => ElementKind::Assembly,
        },
        link_document_id: None,
    }
}

/// Only this producer and fully validated trusted lookup can construct a handle.
/// It deliberately has no Deserialize or public record/plan constructor.
pub struct TrustedAcquisitionPlan {
    record: ProvenanceRecord,
}

impl TrustedAcquisitionPlan {
    pub fn plan(&self) -> &ResolvedSelectionPlan {
        &self.record.plan
    }

    pub fn lookup_request(&self) -> Result<Vec<u8>, ApiFailure> {
        bounded_canonical(&ProvenanceLookup {
            provenance_schema_version: 1,
            plan_identity: self.record.plan.plan_identity.clone(),
            provenance_identity: self.record.provenance_identity.clone(),
            plan: None,
            record: None,
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProvenanceLookup {
    provenance_schema_version: u32,
    plan_identity: String,
    provenance_identity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    plan: Option<ResolvedSelectionPlan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    record: Option<ProvenanceRecord>,
}

/// Existing coordinator followed by the unchanged planner, with no caller-supplied
/// successful plan. The frozen request is owned throughout the invocation.
pub async fn plan_for_acquisition(
    db: &Database,
    api: &OnshapeApi,
    root: &OnshapeSource,
    typed_values: &BTreeMap<String, CanonicalParameterValue>,
    selectors: Vec<SelectionSelector>,
) -> Result<TrustedAcquisitionPlan, ApiFailure> {
    let element_kind = match root.element_kind {
        ElementKind::PartStudio => SelectionElementKind::PartStudio,
        ElementKind::Assembly => SelectionElementKind::Assembly,
    };
    // Validate source/selector shape before the coordinator's first network call.
    let mut request = SelectionRequest {
        document_id: root.document_id.clone(),
        version_id: root.version_id.clone(),
        element_id: root.element_id.clone(),
        element_kind,
        configuration_encoding: EncodingHandoff {
            source_hash: "0".repeat(64),
            config_hash: "0".repeat(64),
            encoding_context_hash: "0".repeat(64),
            encoded_id: "preflight".to_owned(),
        },
        selectors,
    };
    request.preflight()?;
    if root.link_document_id.is_some() {
        return Err(invalid());
    }
    request.configuration_encoding =
        configuration_encoding::resolve(db, api, root, typed_values).await?;
    record_planning_invocation(db, api, request).await
}

async fn record_planning_invocation(
    db: &Database,
    api: &OnshapeApi,
    request: SelectionRequest,
) -> Result<TrustedAcquisitionPlan, ApiFailure> {
    request.preflight()?;
    let h = &request.configuration_encoding;
    let cached = db
        .configuration_encoding(&h.source_hash, &h.config_hash, &h.encoding_context_hash)
        .await
        .map_err(|_| invariant("provenance_encoding_read"))?
        .ok_or_else(|| invariant("provenance_encoding_missing"))?;
    let context_value = onshape_api::parse_json(
        cached.encoding_context_json.as_bytes(),
        onshape_api::SMALL_RESPONSE_LIMIT,
    )?;
    let original_context: EncodingContext = serde_json::from_value(context_value.clone())
        .map_err(|_| invariant("provenance_encoding_context"))?;
    if serde_json::to_value(&original_context)
        .map_err(|_| invariant("provenance_encoding_context"))?
        != context_value
    {
        return Err(invariant("provenance_encoding_context"));
    }
    let plan = onshape_selection::plan_selection(db, api, &request).await?;
    // Compare selector projections: request and full authoring selector schemas differ.
    if plan.root.document_id != request.document_id
        || plan.root.version_id != request.version_id
        || plan.root.element_id != request.element_id
        || plan.root.element_kind != request.element_kind
        || plan.objects.len() != request.selectors.len()
        || !plan
            .objects
            .iter()
            .zip(&request.selectors)
            .all(|(object, requested)| match (&object.selector, requested) {
                (
                    AuthoringSelector::PartStudioPart { part_id, .. },
                    SelectionSelector::Part { part_id: selected },
                ) => part_id == selected,
                (
                    AuthoringSelector::AssemblyOccurrence {
                        occurrence_path, ..
                    },
                    SelectionSelector::Occurrence {
                        occurrence_path: selected,
                    },
                ) => occurrence_path == selected,
                _ => false,
            })
    {
        return Err(invariant("provenance_invocation_mismatch"));
    }
    let bindings = plan
        .objects
        .iter()
        .enumerate()
        .map(|(position, object)| {
            let (representation, configuration_request_value) = match plan.root.element_kind {
                SelectionElementKind::PartStudio => (
                    Representation::PartStudioOriginalEncoding,
                    h.encoded_id.clone(),
                ),
                SelectionElementKind::Assembly => (
                    Representation::AssemblyLeafFullConfiguration,
                    object.configured_leaf.configuration_identity.clone(),
                ),
            };
            ProvenanceBinding {
                position,
                plan_local_object_identity: object.plan_local_object_identity.clone(),
                selector: object.selector.clone(),
                configured_leaf: object.configured_leaf.clone(),
                representation,
                configuration_request_value,
            }
        })
        .collect();
    let mut record = ProvenanceRecord {
        provenance_schema_version: 1,
        request_contract: CONTRACT.to_owned(),
        configuration_encoding: h.clone(),
        encoding_context: original_context,
        plan,
        bindings,
        provenance_identity: String::new(),
    };
    record.provenance_identity = record.identity()?;
    validate_record(db, api, &record).await?;
    let canonical =
        String::from_utf8(bounded_canonical(&record)?).map_err(|_| invariant("provenance_utf8"))?;
    db.record_acquisition_provenance(
        &record.plan.plan_identity,
        &record.provenance_identity,
        &canonical,
    )
    .await
    .map_err(|_| invariant("provenance_recording_failed"))?;
    // Reread via the same trusted consumer path, including a concurrent winner.
    read_association(
        db,
        api,
        &record.plan.plan_identity,
        &record.provenance_identity,
    )
    .await
}

async fn validate_record(
    db: &Database,
    api: &OnshapeApi,
    record: &ProvenanceRecord,
) -> Result<(), ApiFailure> {
    onshape_selection::validate_retained_plan(&record.plan)?;
    if record.provenance_schema_version != 1
        || record.request_contract != CONTRACT
        || !onshape_api::is_sha256(&record.provenance_identity)
        || record.identity()? != record.provenance_identity
        || record.bindings.len() != record.plan.objects.len()
    {
        return Err(invariant("provenance_record_identity"));
    }
    let source = source(&record.plan);
    let expected_context = configuration_encoding::context(api.origin(), &source)?;
    if record.encoding_context != expected_context {
        return Err(invariant("provenance_context_mismatch"));
    }
    configuration_encoding::validate_handoff(db, api, &source, &record.configuration_encoding)
        .await
        .map_err(|_| invariant("provenance_handoff_mismatch"))?;
    for (position, (binding, object)) in
        record.bindings.iter().zip(&record.plan.objects).enumerate()
    {
        let (representation, value) = match record.plan.root.element_kind {
            SelectionElementKind::PartStudio => (
                Representation::PartStudioOriginalEncoding,
                &record.configuration_encoding.encoded_id,
            ),
            SelectionElementKind::Assembly => (
                Representation::AssemblyLeafFullConfiguration,
                &object.configured_leaf.configuration_identity,
            ),
        };
        if binding.position != position
            || binding.plan_local_object_identity != object.plan_local_object_identity
            || binding.selector != object.selector
            || binding.configured_leaf != object.configured_leaf
            || binding.representation != representation
            || &binding.configuration_request_value != value
        {
            return Err(invariant("provenance_binding_mismatch").position(position));
        }
    }
    Ok(())
}

async fn read_association(
    db: &Database,
    api: &OnshapeApi,
    plan_identity: &str,
    provenance_identity: &str,
) -> Result<TrustedAcquisitionPlan, ApiFailure> {
    let (stored_identity, json) = db
        .acquisition_provenance(plan_identity)
        .await
        .map_err(|_| invariant("provenance_lookup_failed"))?
        .ok_or_else(|| invariant("provenance_association_missing"))?;
    let value = onshape_api::parse_json(json.as_bytes(), RECORD_LIMIT)
        .map_err(|_| invariant("provenance_record_json"))?;
    let record: ProvenanceRecord =
        serde_json::from_value(value.clone()).map_err(|_| invariant("provenance_record_shape"))?;
    if record.plan.plan_identity != plan_identity
        || record.provenance_identity != provenance_identity
        || stored_identity != provenance_identity
        || bounded_canonical(&record)? != json.as_bytes()
    {
        return Err(invariant("provenance_association_mismatch"));
    }
    validate_record(db, api, &record).await?;
    Ok(TrustedAcquisitionPlan { record })
}

/// Serialized input authorizes lookup only, never record creation or repair.
pub async fn lookup_acquisition_plan(
    db: &Database,
    api: &OnshapeApi,
    bytes: &[u8],
) -> Result<TrustedAcquisitionPlan, ApiFailure> {
    let value = onshape_api::parse_json(bytes, RECORD_LIMIT).map_err(|_| invalid())?;
    let lookup: ProvenanceLookup = serde_json::from_value(value.clone()).map_err(|_| invalid())?;
    if lookup.provenance_schema_version != 1
        || !onshape_api::is_sha256(&lookup.plan_identity)
        || !onshape_api::is_sha256(&lookup.provenance_identity)
        || value.get("plan").is_some_and(Value::is_null)
        || value.get("record").is_some_and(Value::is_null)
        || cache_key::canonical_json_bytes(&value).map_err(|_| invalid())?
            != bounded_canonical(&lookup)?
    {
        return Err(invalid());
    }
    let trusted =
        read_association(db, api, &lookup.plan_identity, &lookup.provenance_identity).await?;
    if let Some(plan) = &lookup.plan {
        onshape_selection::validate_retained_plan(plan)?;
    }
    if let Some(record) = &lookup.record {
        validate_record(db, api, record).await?;
    }
    if lookup
        .plan
        .as_ref()
        .is_some_and(|plan| plan != &trusted.record.plan)
        || lookup.record.as_ref().is_some_and(|record| {
            bounded_canonical(record).ok() != bounded_canonical(&trusted.record).ok()
        })
    {
        return Err(invariant("provenance_supplied_pair_mismatch"));
    }
    Ok(trusted)
}

/// Private anonymous file owned by its payload references. No caller can mutate
/// it or derive a protocol path from its location; last-reference drop closes it.
pub struct RetainedBytes {
    file: Mutex<File>,
}

#[derive(Clone)]
pub struct RetainedPayload {
    pub storage_reference: Arc<RetainedBytes>,
    pub byte_length: u64,
    pub sha256: String,
}

impl RetainedPayload {
    /// Staging owns the output path. Concurrent copies serialize file offsets.
    pub fn copy_to(&self, output: &mut impl Write) -> Result<(), ApiFailure> {
        let mut file = self
            .storage_reference
            .file
            .lock()
            .map_err(|_| invariant("retention_lock"))?;
        file.seek(SeekFrom::Start(0))
            .map_err(|_| invariant("retention_read"))?;
        let count = std::io::copy(&mut *file, output).map_err(|_| invariant("retention_copy"))?;
        if count != self.byte_length {
            return Err(invariant("retention_length"));
        }
        Ok(())
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcquisitionEvidence {
    request_contract: String,
    representation: Representation,
    configuration_request_value: String,
    document_id: String,
    version_id: String,
    configured_leaf: ConfiguredLeaf,
    create_request_path: String,
    create_request_query: BTreeMap<String, String>,
    create_request_body: Value,
    create_translation_id: String,
    terminal_translation_id: String,
    external_data_id: String,
    download_document_id: String,
    transport_media: String,
    byte_length: u64,
    sha256: String,
}

pub struct OccurrencePayloadBinding {
    pub position: usize,
    pub plan_local_object_identity: String,
    pub configured_leaf: ConfiguredLeaf,
    pub retained_payload: RetainedPayload,
    pub acquisition_evidence: AcquisitionEvidence,
}

pub struct AcquiredGeometry {
    pub plan: ResolvedSelectionPlan,
    pub provenance_identity: String,
    pub bindings: Vec<OccurrencePayloadBinding>,
    // Freeze the successful producer's exact retention references and causal
    // evidence. Public handoff metadata cannot establish a new acquisition.
    retention_proof: Vec<(Arc<RetainedBytes>, String)>,
}

pub(crate) fn acquisition_evidence_identity(
    evidence: &AcquisitionEvidence,
) -> Result<String, ApiFailure> {
    cache_key::hash_json("onshape-export-geometry-acquisition-evidence-v1", evidence)
        .map_err(|_| invariant("acquisition_evidence_identity"))
}

/// Pure consumer check; no source resolution, storage reads, or API calls.
pub(crate) fn validate_acquisition_handoff(
    trusted: &TrustedAcquisitionPlan,
    acquired: &AcquiredGeometry,
) -> Result<(), ApiFailure> {
    onshape_selection::validate_retained_plan(&acquired.plan)?;
    if acquired.plan != trusted.record.plan
        || acquired.provenance_identity != trusted.record.provenance_identity
        || acquired.retention_proof.len() != acquired.bindings.len()
    {
        return Err(invariant("acquisition_handoff_proof"));
    }
    validate_acquired(&trusted.record, &acquired.bindings)?;
    for (binding, (storage, evidence_identity)) in
        acquired.bindings.iter().zip(&acquired.retention_proof)
    {
        if !Arc::ptr_eq(storage, &binding.retained_payload.storage_reference)
            || *evidence_identity != acquisition_evidence_identity(&binding.acquisition_evidence)?
        {
            return Err(invariant("acquisition_handoff_proof").position(binding.position));
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct AcquisitionTiming {
    plan: Duration,
    leaf: Duration,
    translation: Duration,
    poll_intervals: [Duration; 5],
    max_polls: usize,
}

impl Default for AcquisitionTiming {
    fn default() -> Self {
        Self {
            plan: Duration::from_secs(900),
            leaf: Duration::from_secs(660),
            translation: Duration::from_secs(600),
            poll_intervals: [2, 4, 8, 15, 30].map(Duration::from_secs),
            max_polls: 32,
        }
    }
}

/// Validate the complete association before the sole version barrier. No
/// provenance fallback, content cache, or metadata/configuration rediscovery.
pub async fn acquire_geometry(
    db: &Database,
    api: &OnshapeApi,
    trusted: &TrustedAcquisitionPlan,
    retention_directory: &Path,
) -> Result<AcquiredGeometry, ApiFailure> {
    acquire_with_timing(
        db,
        api,
        trusted,
        retention_directory,
        AcquisitionTiming::default(),
    )
    .await
}

async fn acquire_with_timing(
    db: &Database,
    api: &OnshapeApi,
    trusted: &TrustedAcquisitionPlan,
    retention_directory: &Path,
    timing: AcquisitionTiming,
) -> Result<AcquiredGeometry, ApiFailure> {
    let checked = read_association(
        db,
        api,
        &trusted.record.plan.plan_identity,
        &trusted.record.provenance_identity,
    )
    .await?;
    if bounded_canonical(&checked.record)? != bounded_canonical(&trusted.record)? {
        return Err(invariant("provenance_handle_mismatch"));
    }
    let record = &checked.record;
    let start = Instant::now();
    let deadline = start + timing.plan;
    ensure_time(deadline)?;
    let microversion = tokio::time::timeout_at(
        deadline,
        api.resolve_version(&record.plan.root.document_id, &record.plan.root.version_id),
    )
    .await
    .map_err(|_| timeout())??;
    ensure_time(deadline)?;
    if microversion != record.plan.root.document_microversion
        || record.plan.objects.iter().any(|object| {
            object.configured_leaf.document_id != record.plan.root.document_id
                || object.configured_leaf.document_microversion != microversion
        })
    {
        return Err(ApiFailure::new(
            FailureKind::UnavailableSourceState,
            "acquisition_snapshot_mismatch",
        ));
    }
    let mut acquired: HashMap<ConfiguredLeaf, (RetainedPayload, AcquisitionEvidence)> =
        HashMap::new();
    let mut bindings = Vec::with_capacity(record.bindings.len());
    for binding in &record.bindings {
        ensure_time(deadline)?;
        if !acquired.contains_key(&binding.configured_leaf) {
            let result = acquire_leaf(
                api,
                &record.plan,
                binding,
                deadline,
                retention_directory,
                timing,
            )
            .await
            .map_err(|failure| failure.position(binding.position))?;
            acquired.insert(binding.configured_leaf.clone(), result);
        }
        let (retained_payload, acquisition_evidence) = acquired
            .get(&binding.configured_leaf)
            .ok_or_else(|| invariant("acquisition_binding_missing"))?;
        // Reuse depends on exact leaf + demonstrated request, never byte identity.
        if acquisition_evidence.configuration_request_value != binding.configuration_request_value
            || acquisition_evidence.representation != binding.representation
        {
            return Err(invariant("acquisition_reuse_conflict"));
        }
        bindings.push(OccurrencePayloadBinding {
            position: binding.position,
            plan_local_object_identity: binding.plan_local_object_identity.clone(),
            configured_leaf: binding.configured_leaf.clone(),
            retained_payload: retained_payload.clone(),
            acquisition_evidence: acquisition_evidence.clone(),
        });
    }
    ensure_time(deadline)?;
    validate_acquired(record, &bindings)?;
    let retention_proof = bindings
        .iter()
        .map(|binding| {
            Ok((
                Arc::clone(&binding.retained_payload.storage_reference),
                acquisition_evidence_identity(&binding.acquisition_evidence)?,
            ))
        })
        .collect::<Result<Vec<_>, ApiFailure>>()?;
    let result = AcquiredGeometry {
        plan: record.plan.clone(),
        provenance_identity: record.provenance_identity.clone(),
        bindings,
        retention_proof,
    };
    ensure_time(deadline)?;
    Ok(result)
}

fn validate_acquired(
    record: &ProvenanceRecord,
    bindings: &[OccurrencePayloadBinding],
) -> Result<(), ApiFailure> {
    if bindings.len() != record.bindings.len() {
        return Err(invariant("acquisition_handoff_cardinality"));
    }
    for (binding, planned) in bindings.iter().zip(&record.bindings) {
        let evidence = &binding.acquisition_evidence;
        if binding.position != planned.position
            || binding.plan_local_object_identity != planned.plan_local_object_identity
            || binding.configured_leaf != planned.configured_leaf
            || evidence.configured_leaf != planned.configured_leaf
            || evidence.configuration_request_value != planned.configuration_request_value
            || evidence.representation != planned.representation
            || evidence.request_contract != record.request_contract
            || evidence.document_id != record.plan.root.document_id
            || evidence.version_id != record.plan.root.version_id
            || evidence.download_document_id != record.plan.root.document_id
            || evidence.create_translation_id != evidence.terminal_translation_id
            || !onshape_api::bounded_visible_ascii(&evidence.external_data_id)
            || !onshape_api::bounded_visible_ascii(&evidence.create_translation_id)
            || evidence.create_request_path
                != onshape_api::geometry_create_path(
                    &evidence.document_id,
                    &evidence.version_id,
                    &binding.configured_leaf.element_id,
                )
            || !evidence.create_request_query.is_empty()
            || evidence.create_request_body
                != onshape_api::geometry_create_body(
                    &planned.configuration_request_value,
                    &planned.configured_leaf.part_id,
                )
            || evidence.byte_length != binding.retained_payload.byte_length
            || evidence.sha256 != binding.retained_payload.sha256
            || evidence.byte_length == 0
            || evidence.byte_length > 134_217_728
            || !onshape_api::is_sha256(&evidence.sha256)
        {
            return Err(invariant("acquisition_handoff_mismatch").position(planned.position));
        }
    }
    Ok(())
}

fn ensure_time(deadline: Instant) -> Result<(), ApiFailure> {
    if Instant::now() >= deadline {
        Err(timeout())
    } else {
        Ok(())
    }
}

fn translation(
    value: &Value,
    plan: &ResolvedSelectionPlan,
    leaf: &ConfiguredLeaf,
    expected_id: Option<&str>,
) -> Result<(String, String), ApiFailure> {
    let object = value
        .as_object()
        .ok_or_else(|| invariant("translation_shape"))?;
    let id = onshape_api::required_ascii(object, "id")?;
    let state = onshape_api::required_ascii(object, "requestState")?;
    if expected_id.is_some_and(|expected| expected != id)
        || !matches!(state, "ACTIVE" | "DONE" | "FAILED")
    {
        return Err(invariant("translation_identity_state"));
    }
    for (field, expected) in [
        ("documentId", plan.root.document_id.as_str()),
        ("requestElementId", leaf.element_id.as_str()),
        ("versionId", plan.root.version_id.as_str()),
        ("resultDocumentId", plan.root.document_id.as_str()),
    ] {
        if object.contains_key(field) && onshape_api::required_ascii(object, field)? != expected {
            return Err(invariant("translation_source_echo"));
        }
    }
    Ok((id.to_owned(), state.to_owned()))
}

fn terminal_external_data(value: &Value) -> Result<String, ApiFailure> {
    let object = value
        .as_object()
        .ok_or_else(|| invariant("translation_shape"))?;
    let results = object
        .get("resultExternalDataIds")
        .and_then(Value::as_array)
        .filter(|ids| ids.len() == 1)
        .ok_or_else(|| invariant("translation_result_cardinality"))?;
    let id = results[0]
        .as_str()
        .filter(|id| onshape_api::bounded_visible_ascii(id))
        .ok_or_else(|| invariant("translation_result_id"))?;
    if object.get("resultElementIds").is_some_and(|elements| {
        !elements.is_null() && !elements.as_array().is_some_and(Vec::is_empty)
    }) {
        return Err(invariant("translation_result_elements"));
    }
    Ok(id.to_owned())
}

async fn acquire_leaf(
    api: &OnshapeApi,
    plan: &ResolvedSelectionPlan,
    binding: &ProvenanceBinding,
    plan_deadline: Instant,
    directory: &Path,
    timing: AcquisitionTiming,
) -> Result<(RetainedPayload, AcquisitionEvidence), ApiFailure> {
    let started = Instant::now();
    let translation_deadline = (started + timing.translation).min(plan_deadline);
    let leaf_deadline = (started + timing.leaf).min(plan_deadline);
    ensure_time(translation_deadline)?;
    let leaf = &binding.configured_leaf;
    let mut value = api
        .create_geometry_translation(
            &plan.root.document_id,
            &plan.root.version_id,
            &leaf.element_id,
            &binding.configuration_request_value,
            &leaf.part_id,
            translation_deadline,
        )
        .await?;
    ensure_time(translation_deadline)?;
    let (create_id, mut state) = translation(&value, plan, leaf, None)?;
    let mut polls = 0;
    loop {
        if state == "FAILED" {
            return Err(ApiFailure::new(
                FailureKind::OperationalTranslationFailure,
                "translation_failed",
            ));
        }
        if state == "DONE" {
            break;
        }
        if polls >= timing.max_polls {
            return Err(timeout());
        }
        ensure_time(translation_deadline)?;
        let wake = (Instant::now() + timing.poll_intervals[polls.min(4)]).min(translation_deadline);
        tokio::time::sleep_until(wake).await;
        ensure_time(translation_deadline)?;
        value = api
            .poll_geometry_translation(&create_id, translation_deadline)
            .await?;
        ensure_time(translation_deadline)?;
        let (_, next) = translation(&value, plan, leaf, Some(&create_id))?;
        state = next;
        polls += 1;
    }
    let external_data_id = terminal_external_data(&value)?;
    ensure_time(leaf_deadline)?;
    let downloaded = api
        .download_geometry(&plan.root.document_id, &external_data_id, leaf_deadline)
        .await?;
    ensure_time(leaf_deadline)?;
    let byte_length = downloaded.bytes.len() as u64;
    let sha256 = cache_key::hex_sha256(&downloaded.bytes);
    ensure_time(leaf_deadline)?;
    let directory = directory.to_owned();
    let stored = tokio::task::spawn_blocking(move || {
        let mut file =
            tempfile::tempfile_in(directory).map_err(|_| invariant("retention_create"))?;
        file.write_all(&downloaded.bytes)
            .map_err(|_| invariant("retention_write"))?;
        file.sync_all().map_err(|_| invariant("retention_flush"))?;
        Ok::<_, ApiFailure>(RetainedBytes {
            file: Mutex::new(file),
        })
    });
    let retained = tokio::time::timeout_at(leaf_deadline, stored)
        .await
        .map_err(|_| timeout())?
        .map_err(|_| invariant("retention_task"))??;
    ensure_time(leaf_deadline)?;
    let evidence = AcquisitionEvidence {
        request_contract: CONTRACT.to_owned(),
        representation: binding.representation,
        configuration_request_value: binding.configuration_request_value.clone(),
        document_id: plan.root.document_id.clone(),
        version_id: plan.root.version_id.clone(),
        configured_leaf: leaf.clone(),
        create_request_path: onshape_api::geometry_create_path(
            &plan.root.document_id,
            &plan.root.version_id,
            &leaf.element_id,
        ),
        create_request_query: BTreeMap::new(),
        create_request_body: onshape_api::geometry_create_body(
            &binding.configuration_request_value,
            &leaf.part_id,
        ),
        create_translation_id: create_id.clone(),
        terminal_translation_id: create_id,
        external_data_id,
        download_document_id: plan.root.document_id.clone(),
        transport_media: downloaded.transport_media,
        byte_length,
        sha256: sha256.clone(),
    };
    let result = (
        RetainedPayload {
            storage_reference: Arc::new(retained),
            byte_length,
            sha256,
        },
        evidence,
    );
    ensure_time(leaf_deadline)?;
    Ok(result)
}

#[cfg(test)]
pub(crate) async fn generator_runner_test_inputs()
-> crate::generator_inputs::ConstructedGeneratorInputs {
    tests::runner_test_inputs().await
}

#[cfg(test)]
#[path = "onshape_geometry_tests.rs"]
mod tests;
