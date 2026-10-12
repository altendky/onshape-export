//! Service-owned Bambu project dispatch. Ordered selectors, frozen deployment
//! policy, verified neutral recipes, and ready artifacts remain distinct from
//! ordinary Onshape geometry downloads.

use std::{collections::HashMap, sync::Arc};

use anyhow::ensure;
use serde::{Deserialize, Serialize};

use crate::{
    AppState, JobPayload, ValidatedConfiguration, bambu_deployment, cache_key, catalog,
    db::GeneratorArtifactLookup,
    deployed_generator::DeployedGenerator,
    generator_inputs::construct_generator_inputs,
    generator_processing::{PreparedGeneratorProcessing, prepare_generator_processing},
    generator_publication::{
        GeneratorPublicationTarget, publish_generator_output, reconcile_generator_artifact,
    },
    generator_runner::{GeneratorRunnerError, GeneratorRunnerOptions, run_generator},
    onshape_api::{ApiFailure, FailureKind},
    onshape_geometry::{acquire_geometry, plan_for_acquisition},
    onshape_selection::{SelectionElementKind, SelectionRequest, SelectionSelector},
};

const JOB_KIND: &str = "bambu_project";
const REQUEST_LIMIT: usize = 131_072;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProjectJob {
    pub model_slug: String,
    pub source_hash: String,
    pub config_hash: String,
    pub values: HashMap<String, String>,
    pub selectors: Vec<SelectionSelector>,
    pub generator_static_identity: String,
    pub policy_identity: String,
    pub catalog_identity: String,
    pub request_identity: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectStatus {
    pub request_identity: String,
    pub status: String,
    pub message: String,
    pub public_url: Option<String>,
    pub object_key: Option<String>,
    pub sha256: Option<String>,
    pub byte_length: Option<i64>,
    pub media_type: Option<String>,
    pub failure_category: Option<String>,
    pub failure_code: Option<String>,
}

#[derive(Debug, thiserror::Error)]
#[error("project generation failed: {category} ({code})")]
struct ProjectFailure {
    category: &'static str,
    code: &'static str,
}

fn failure(category: &'static str, code: &'static str) -> anyhow::Error {
    ProjectFailure { category, code }.into()
}

fn api_failure(error: ApiFailure) -> anyhow::Error {
    let (category, code) = match error.kind {
        FailureKind::InvalidSelection => ("invalid_selection", "selection_invalid"),
        FailureKind::UnavailableSourceState => {
            ("unavailable_source_state", "selected_source_unavailable")
        }
        FailureKind::AuthenticationFailure => ("operational_failure", "onshape_authentication"),
        FailureKind::TransportFailure => ("operational_failure", "onshape_transport"),
        FailureKind::OperationalTimeoutFailure => ("operational_failure", "acquisition_timeout"),
        FailureKind::OperationalTranslationFailure => ("operational_failure", "translation_failed"),
        FailureKind::OperationalHttpFailure => ("operational_failure", "onshape_http"),
        FailureKind::OperationalApiContractFailure => ("operational_failure", "onshape_contract"),
    };
    failure(category, code)
}

fn runner_failure(error: GeneratorRunnerError) -> anyhow::Error {
    let code = match error {
        GeneratorRunnerError::UnsupportedCombination => "generator_unsupported",
        GeneratorRunnerError::DeploymentIntegrity(_) => "deployment_integrity",
        GeneratorRunnerError::Timeout => "generator_timeout",
        GeneratorRunnerError::StructuredFailure { .. } => "generator_reported_failure",
        GeneratorRunnerError::Cancelled => "generator_cancelled",
        _ => "generator_execution_failed",
    };
    failure("operational_failure", code)
}

fn configured(state: &AppState) -> anyhow::Result<&DeployedGenerator> {
    bambu_deployment::configured_generator(state)
        .map_err(|_| failure("deployment_unavailable", "approved_generator_unavailable"))
}

fn profile_identity(generator: &DeployedGenerator) -> anyhow::Result<String> {
    let policy = bambu_deployment::input_policy(generator);
    let compatibility = bambu_deployment::compatibility(generator);
    let protocol = bambu_deployment::protocol_inputs(generator);
    cache_key::hash_json(
        "onshape-export-bambu-project-policy-v1",
        &serde_json::json!({
            "requirementsIdentity": policy.requirements_identity,
            "inputKindIdentity": policy.input_kind_identity,
            "inputSchemaIdentity": policy.input_schema_identity,
            "detectedKindIdentity": policy.detected_kind_identity,
            "mediaType": policy.media_type,
            "protocolVersion": compatibility.protocol_version,
            "dialectIdentity": compatibility.dialect_identity,
            "capabilityIdentities": compatibility.capability_identities,
            "settingsSchemaIdentity": compatibility.settings_schema_identity,
            "manifestPath": protocol.manifest_path,
            "settingsContentIdentity": protocol.settings_content_identity,
            "settingsPath": protocol.settings_path,
            "settingsMediaType": protocol.settings_media_type,
            "settingsDetectedKindIdentity": protocol.settings_detected_kind_identity,
            "output": protocol.output,
        }),
    )
}

fn catalog_identity(model: &catalog::Model) -> anyhow::Result<String> {
    cache_key::hash_json(
        "onshape-export-bambu-project-catalog-v1",
        &serde_json::json!({
            "slug": model.slug,
            "published": model.published,
            "source": model.onshape,
            "project": model.exports.bambu_project,
            "parameterPolicy": model.parameter_policy,
            "parameterOverrides": model.parameter_overrides,
        }),
    )
}

impl ProjectJob {
    fn identity(&self) -> anyhow::Result<String> {
        let mut payload = self.clone();
        payload.request_identity.clear();
        cache_key::hash_json("onshape-export-bambu-project-request-v1", &payload)
    }

    fn canonical_json(&self) -> anyhow::Result<String> {
        let bytes = cache_key::canonical_json_bytes(self)?;
        ensure!(
            bytes.len() <= REQUEST_LIMIT,
            "project request exceeds its bound"
        );
        Ok(String::from_utf8(bytes)?)
    }

    fn from_record(json: &str) -> anyhow::Result<Self> {
        let value = crate::onshape_api::parse_json(json.as_bytes(), REQUEST_LIMIT)?;
        let request: Self = serde_json::from_value(value)?;
        ensure!(
            request.canonical_json()? == json && request.identity()? == request.request_identity,
            "stored project request integrity mismatch"
        );
        Ok(request)
    }

    fn validate_profile(&self, generator: &DeployedGenerator) -> anyhow::Result<()> {
        ensure!(
            self.identity()? == self.request_identity
                && self.generator_static_identity == generator.static_identity()
                && self.policy_identity == profile_identity(generator)?,
            "project request profile integrity mismatch"
        );
        Ok(())
    }
}

pub(crate) fn work_key(request_identity: &str) -> String {
    format!("bambu-project-v1:{request_identity}")
}

fn validate_selectors(
    model: &catalog::Model,
    selectors: &[SelectionSelector],
) -> anyhow::Result<()> {
    let selected = model
        .exports
        .bambu_project
        .as_ref()
        .ok_or_else(|| failure("unavailable_source_state", "project_not_enabled"))?;
    if selected.selectors != selectors {
        return Err(failure("invalid_selection", "catalog_selection_mismatch"));
    }
    let request = SelectionRequest {
        document_id: model.onshape.document_id.clone(),
        version_id: model.onshape.version_id.clone(),
        element_id: model.onshape.element_id.clone(),
        element_kind: match model.onshape.element_kind {
            catalog::ElementKind::PartStudio => SelectionElementKind::PartStudio,
            catalog::ElementKind::Assembly => SelectionElementKind::Assembly,
        },
        configuration_encoding: crate::configuration_encoding::EncodingHandoff {
            source_hash: "0".repeat(64),
            config_hash: "0".repeat(64),
            encoding_context_hash: "0".repeat(64),
            encoded_id: "preflight".to_owned(),
        },
        selectors: selectors.to_vec(),
    };
    request.preflight().map_err(api_failure)?;
    if model.onshape.link_document_id.is_some() {
        return Err(failure(
            "unavailable_source_state",
            "linked_source_unsupported",
        ));
    }
    Ok(())
}

pub(crate) async fn enqueue(
    state: &AppState,
    model: &catalog::Model,
    validated: &ValidatedConfiguration,
    selectors: &[SelectionSelector],
) -> anyhow::Result<ProjectJob> {
    let generator = configured(state)?;
    validate_selectors(model, selectors)?;
    state
        .db
        .generator_output_root()
        .map_err(|_| failure("deployment_unavailable", "output_retention_unavailable"))?;
    let source_hash = crate::resolve_source_hash(state, model)
        .await
        .map_err(|_| failure("operational_failure", "source_resolution_failed"))?;
    let config_hash = crate::persist_configuration_selection(state, &source_hash, validated)
        .await
        .map_err(|_| failure("operational_failure", "configuration_persistence_failed"))?;
    let mut request = ProjectJob {
        model_slug: model.slug.clone(),
        source_hash,
        config_hash,
        values: validated.values.clone(),
        selectors: selectors.to_vec(),
        generator_static_identity: generator.static_identity().to_owned(),
        policy_identity: profile_identity(generator)?,
        catalog_identity: catalog_identity(model)?,
        request_identity: String::new(),
    };
    request.request_identity = request.identity()?;
    let json = request.canonical_json()?;
    state
        .db
        .insert_bambu_project_request(&request.request_identity, &json)
        .await
        .map_err(|_| failure("operational_failure", "request_persistence_failed"))?;
    let current = status(state, model, &request.request_identity).await?;
    if current.status != "ready" && current.status != "unavailable" {
        crate::enqueue_job(
            state,
            &work_key(&request.request_identity),
            JOB_KIND,
            &JobPayload::BambuProject {
                request: request.clone(),
            },
            true,
        )
        .await
        .map_err(|_| failure("operational_failure", "project_enqueue_failed"))?;
    }
    Ok(request)
}

fn target(
    request: &ProjectJob,
    generator: &DeployedGenerator,
    producing_job_key: Option<&str>,
) -> anyhow::Result<GeneratorPublicationTarget> {
    Ok(GeneratorPublicationTarget {
        model_slug: request.model_slug.clone(),
        output_kind: format!("slicer_project:{}", generator.document().dialect_identity),
        format: "project_3mf".to_owned(),
        logical_path: format!("{}-bambu-project.3mf", request.model_slug),
        producing_job_key: producing_job_key.map(ToOwned::to_owned),
        parameter_schema_version: i64::from(crate::parameters::SCHEMA_VERSION),
        config_values_json: crate::config_values_json(&request.values)?,
    })
}

fn validate_prepared(
    request: &ProjectJob,
    generator: &DeployedGenerator,
    prepared: &PreparedGeneratorProcessing,
) -> anyhow::Result<()> {
    let recipe = prepared.recipe();
    let policy = bambu_deployment::input_policy(generator);
    let protocol = bambu_deployment::protocol_inputs(generator);
    ensure!(
        recipe.input_manifest.source_identity == request.source_hash
            && recipe.input_manifest.objects.len() == request.selectors.len()
            && recipe.input_manifest.requirements_identity == policy.requirements_identity
            && recipe.input_manifest.export.kind_identity == policy.input_kind_identity
            && recipe.input_manifest.export.schema_identity == policy.input_schema_identity
            && recipe.input_manifest.objects.iter().all(|object| object
                .retained_content
                .media_type
                == policy.media_type
                && object.retained_content.detected_kind_identity == policy.detected_kind_identity)
            && recipe.invocation.input_manifest.path == protocol.manifest_path
            && recipe.invocation.settings.content.content_identity
                == protocol.settings_content_identity
            && recipe.invocation.settings.content.path == protocol.settings_path
            && recipe.invocation.settings.content.media_type == protocol.settings_media_type
            && recipe.invocation.settings.content.detected_kind_identity
                == protocol.settings_detected_kind_identity
            && recipe.invocation.output == protocol.output,
        "project recipe does not match its frozen request policy"
    );
    Ok(())
}

async fn execute_inner(
    state: &AppState,
    request: &ProjectJob,
    producing_job_key: &str,
) -> anyhow::Result<()> {
    let generator = configured(state)?;
    request
        .validate_profile(generator)
        .map_err(|_| failure("stale_request", "project_profile_changed"))?;
    ensure!(
        producing_job_key == work_key(&request.request_identity),
        "project work identity mismatch"
    );
    let stored = state
        .db
        .bambu_project_request(&request.request_identity)
        .await?
        .ok_or_else(|| failure("operational_failure", "request_record_missing"))?;
    ensure!(
        stored.request_json == request.canonical_json()?,
        "project request association mismatch"
    );
    if invalid_output_association(&stored) {
        return Err(failure("operational_failure", "stored_output_association"));
    }
    let model = state
        .db
        .published_catalog_model(&request.model_slug)
        .await?
        .ok_or_else(|| failure("stale_request", "model_unavailable"))?;
    validate_selectors(&model, &request.selectors)?;
    if request.catalog_identity != catalog_identity(&model)? {
        return Err(failure("stale_request", "catalog_policy_changed"));
    }
    let parameters = crate::load_or_refresh_parameters(state, &model)
        .await
        .map_err(|_| failure("operational_failure", "parameter_metadata_failed"))?
        .ok_or_else(|| failure("operational_failure", "parameter_metadata_pending"))?;
    let validated = crate::validate_values(
        &parameters,
        &request.values,
        model.parameter_policy.allow_unknown,
    )
    .map_err(|_| failure("stale_request", "parameters_invalid"))?;
    let source_hash = crate::resolve_source_hash(state, &model)
        .await
        .map_err(|_| failure("operational_failure", "source_resolution_failed"))?;
    if source_hash != request.source_hash
        || crate::configuration_hash(&source_hash, &validated)? != request.config_hash
    {
        return Err(failure("stale_request", "source_or_configuration_changed"));
    }
    let publication = target(request, generator, Some(producing_job_key))?;
    if let Some(hash) = stored.processing_hash {
        let prepared = state
            .db
            .reopen_generator_processing(generator, &hash)
            .await
            .map_err(|_| failure("operational_failure", "stored_recipe_integrity"))?;
        validate_prepared(request, generator, &prepared)
            .map_err(|_| failure("operational_failure", "stored_recipe_policy"))?;
        let reconciled = reconcile_generator_artifact(
            &state.db,
            &state.storage,
            generator,
            &prepared,
            &publication,
        )
        .await;
        if matches!(reconciled, Ok(Some(_))) {
            return Ok(());
        }
        // Reconciliation never reruns acquisition. A durable snapshot of the
        // already accepted output can repair an absent or corrupt stored object.
        // Publication still enforces every immutable declaration and supersession
        // guard; a snapshot cannot replace a malformed or superseded artifact.
        let snapshot = stored
            .output_snapshot_identity
            .ok_or_else(|| failure("operational_failure", "project_input_recovery_required"))?;
        let root = state
            .db
            .generator_output_root()
            .map_err(|_| failure("deployment_unavailable", "output_retention_unavailable"))?;
        let output = crate::generator_output_checkpoint::reopen(&root, &snapshot, &prepared)
            .await
            .map_err(|_| failure("operational_failure", "output_snapshot_integrity"))?;
        publish_generator_output(
            &state.db,
            &state.storage,
            generator,
            &prepared,
            output,
            &publication,
        )
        .await
        .map_err(|_| failure("operational_failure", "project_publication_failed"))?;
        return Ok(());
    }
    let output_root = state
        .db
        .generator_output_root()
        .map_err(|_| failure("deployment_unavailable", "output_retention_unavailable"))?;
    let api = state.onshape.strict_api().map_err(api_failure)?;
    let trusted = plan_for_acquisition(
        &state.db,
        &api,
        &model.onshape,
        &validated.typed_values,
        request.selectors.clone(),
    )
    .await
    .map_err(api_failure)?;
    if trusted.request_source_hash() != request.source_hash
        || trusted.request_config_hash() != request.config_hash
    {
        return Err(failure("stale_request", "acquisition_request_changed"));
    }
    let scratch = GeneratorRunnerOptions::default();
    std::fs::create_dir_all(&scratch.scratch_parent)
        .map_err(|_| failure("operational_failure", "retention_directory_failed"))?;
    let acquired = acquire_geometry(&state.db, &api, &trusted, &scratch.scratch_parent)
        .await
        .map_err(api_failure)?;
    let inputs = construct_generator_inputs(
        &trusted,
        acquired,
        &bambu_deployment::input_policy(generator),
    )
    .map_err(|_| failure("operational_failure", "generator_inputs_invalid"))?;
    let prepared = prepare_generator_processing(
        generator,
        &bambu_deployment::compatibility(generator),
        inputs.manifest(),
        inputs.settings(),
        inputs.expected_placements(),
        &bambu_deployment::protocol_inputs(generator),
    )
    .map_err(|_| failure("operational_failure", "generator_recipe_invalid"))?;
    validate_prepared(request, generator, &prepared)
        .map_err(|_| failure("operational_failure", "recipe_request_mismatch"))?;
    let result = run_generator(generator, inputs, &prepared, &scratch).await;
    let output = persist_accepted_output(state, request, &prepared, &output_root, result).await?;
    publish_generator_output(
        &state.db,
        &state.storage,
        generator,
        &prepared,
        output,
        &publication,
    )
    .await
    .map_err(|_| failure("operational_failure", "project_publication_failed"))?;
    Ok(())
}

async fn persist_accepted_output(
    state: &AppState,
    request: &ProjectJob,
    prepared: &PreparedGeneratorProcessing,
    root: &std::path::Path,
    result: Result<crate::generator_runner::VerifiedGeneratorOutput, GeneratorRunnerError>,
) -> anyhow::Result<Arc<crate::generator_runner::VerifiedGeneratorOutput>> {
    // Failure returns before any snapshot or immutable request binding. A retry
    // is free to make a new causal acquisition until accepted bytes exist.
    let output = Arc::new(result.map_err(runner_failure)?);
    let snapshot = crate::generator_output_checkpoint::persist(root, output.clone(), prepared)
        .await
        .map_err(|_| failure("operational_failure", "output_snapshot_persistence"))?;
    state
        .db
        .bind_bambu_project_output(
            &request.request_identity,
            &request.canonical_json()?,
            prepared,
            &snapshot,
        )
        .await
        .map_err(|_| failure("operational_failure", "recipe_association_failed"))?;
    Ok(output)
}

pub(crate) async fn execute_job(
    state: &AppState,
    request: &ProjectJob,
    producing_job_key: &str,
) -> anyhow::Result<()> {
    let result = execute_inner(state, request, producing_job_key).await;
    match result {
        Ok(()) => {
            state
                .db
                .record_bambu_project_failure(&request.request_identity, None, None)
                .await?;
            Ok(())
        }
        Err(error) => {
            let safe = error.downcast_ref::<ProjectFailure>();
            let category = safe.map_or("operational_failure", |error| error.category);
            let code = safe.map_or("project_operation_failed", |error| error.code);
            state
                .db
                .record_bambu_project_failure(&request.request_identity, Some(category), Some(code))
                .await?;
            if category == "unavailable_source_state" {
                Ok(())
            } else {
                Err(failure(category, code))
            }
        }
    }
}

pub(crate) async fn status(
    state: &AppState,
    model: &catalog::Model,
    request_identity: &str,
) -> anyhow::Result<ProjectStatus> {
    let mut status = ProjectStatus {
        request_identity: request_identity.to_owned(),
        status: "missing".to_owned(),
        message: "No Bambu project request is available.".to_owned(),
        public_url: None,
        object_key: None,
        sha256: None,
        byte_length: None,
        media_type: None,
        failure_category: None,
        failure_code: None,
    };
    if !crate::onshape_api::is_sha256(request_identity) {
        return Ok(status);
    }
    let Some(stored) = state.db.bambu_project_request(request_identity).await? else {
        return Ok(status);
    };
    let request = ProjectJob::from_record(&stored.request_json)
        .map_err(|_| failure("operational_failure", "request_record_integrity"))?;
    if request.request_identity != request_identity || request.model_slug != model.slug {
        return Ok(status);
    }
    if invalid_output_association(&stored) {
        status.status = "failed".to_owned();
        status.message = "Bambu project recovery evidence could not be verified.".to_owned();
        status.failure_category = Some("operational_failure".to_owned());
        status.failure_code = Some("stored_output_association".to_owned());
        return Ok(status);
    }
    if model
        .exports
        .bambu_project
        .as_ref()
        .is_none_or(|project| project.selectors != request.selectors)
        || request.catalog_identity != catalog_identity(model)?
        || crate::resolve_source_hash(state, model).await? != request.source_hash
    {
        status.status = "superseded".to_owned();
        status.message = "The catalog project selection has changed or is unavailable.".to_owned();
        return Ok(status);
    }
    let generator = match configured(state) {
        Ok(generator) => generator,
        Err(_) => {
            status.status = "unavailable".to_owned();
            status.message = "The approved Bambu project generator is unavailable.".to_owned();
            status.failure_category = Some("deployment_unavailable".to_owned());
            status.failure_code = Some("approved_generator_unavailable".to_owned());
            return Ok(status);
        }
    };
    if request.validate_profile(generator).is_err() {
        status.status = "superseded".to_owned();
        status.message =
            "The Bambu project request requires the current generator deployment.".to_owned();
        return Ok(status);
    }
    if let Some(hash) = stored.processing_hash {
        let prepared = state
            .db
            .reopen_generator_processing(generator, &hash)
            .await
            .map_err(|_| failure("operational_failure", "stored_recipe_integrity"))?;
        validate_prepared(&request, generator, &prepared)
            .map_err(|_| failure("operational_failure", "stored_recipe_policy"))?;
        let presentation = target(&request, generator, None)?;
        if let Some(artifact) = state
            .db
            .latest_ready_artifact_for_generator_recipe(GeneratorArtifactLookup {
                prepared: &prepared,
                output_kind: &presentation.output_kind,
                format: &presentation.format,
                primary_role: "generated_project",
                primary_logical_path: &presentation.logical_path,
                primary_content_type: &prepared.recipe().invocation.output.media_type,
            })
            .await?
        {
            status.status = "ready".to_owned();
            status.message = "Bambu project 3MF is ready.".to_owned();
            status.public_url = state.storage.public_url(&artifact.object_key);
            status.object_key = Some(artifact.object_key);
            status.sha256 = artifact.sha256;
            status.byte_length = artifact.byte_len;
            status.media_type = Some(artifact.content_type);
            return Ok(status);
        }
    }
    let job = state.db.job(&work_key(request_identity)).await?;
    if let Some(job) = job
        .as_ref()
        .filter(|job| matches!(job.status.as_str(), "queued" | "running"))
    {
        status.status = job.status.clone();
        status.message = if job.status == "queued" {
            "Bambu project generation is queued."
        } else {
            "Bambu project generation is running."
        }
        .to_owned();
        return Ok(status);
    }
    if let Some(category) = stored.failure_category {
        status.status = if category == "unavailable_source_state" {
            "unavailable"
        } else {
            "failed"
        }
        .to_owned();
        status.message = if status.status == "unavailable" {
            "The selected source state cannot produce this Bambu project."
        } else {
            "Bambu project generation failed."
        }
        .to_owned();
        // These fields are allowlisted even though database state is service-owned.
        status.failure_category = Some(safe_category(&category).to_owned());
        status.failure_code = Some(safe_code(stored.failure_code.as_deref()).to_owned());
    } else if let Some(job) = job {
        status.status = if job.status == "ready" {
            "pending"
        } else {
            &job.status
        }
        .to_owned();
        status.message = "Bambu project is not ready.".to_owned();
    }
    Ok(status)
}

fn safe_category(category: &str) -> &str {
    match category {
        "invalid_selection"
        | "unavailable_source_state"
        | "deployment_unavailable"
        | "stale_request" => category,
        _ => "operational_failure",
    }
}

fn invalid_output_association(stored: &crate::db::BambuProjectRequestRecord) -> bool {
    (stored.processing_hash.is_none() && stored.output_snapshot_identity.is_some())
        || stored
            .processing_hash
            .as_deref()
            .is_some_and(|hash| !crate::onshape_api::is_sha256(hash))
        || stored
            .output_snapshot_identity
            .as_deref()
            .is_some_and(|hash| !crate::onshape_api::is_sha256(hash))
}

fn safe_code(code: Option<&str>) -> &str {
    match code {
        Some(
            code @ ("selection_invalid"
            | "selected_source_unavailable"
            | "onshape_authentication"
            | "onshape_transport"
            | "acquisition_timeout"
            | "translation_failed"
            | "onshape_http"
            | "onshape_contract"
            | "generator_unsupported"
            | "deployment_integrity"
            | "generator_timeout"
            | "generator_reported_failure"
            | "generator_cancelled"
            | "generator_execution_failed"
            | "approved_generator_unavailable"
            | "project_timeout"
            | "project_input_recovery_required"
            | "project_reconciliation_failed"
            | "project_publication_failed"
            | "catalog_policy_changed"
            | "project_not_enabled"
            | "linked_source_unsupported"
            | "output_retention_unavailable"
            | "output_snapshot_integrity"
            | "output_snapshot_persistence"
            | "stored_output_association"),
        ) => code,
        _ => "project_operation_failed",
    }
}

#[cfg(test)]
#[path = "bambu_project_tests.rs"]
mod tests;
