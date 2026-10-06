//! Source-neutral publication of accepted generator bytes through artifact v2.

use std::{fs::File, io::Write, path::PathBuf, sync::Arc};

use anyhow::{Context, ensure};
use sha2::{Digest, Sha256};

use crate::{
    db::{Database, GeneratorArtifactFileInsert, GeneratorArtifactStage},
    deployed_generator::{DeployedGenerator, GeneratorCompatibilityRequest},
    generator_processing::{
        GeneratorCompatibilityDecision, PreparedGeneratorProcessing, generator_artifact_set_hash,
    },
    generator_protocol::{GeneratorResult, ResultStatus, parse_result},
    generator_runner::VerifiedGeneratorOutput,
    storage::StorageClient,
};

/// Service-owned artifact presentation and job metadata, never CLI paths.
pub struct GeneratorPublicationTarget {
    pub model_slug: String,
    pub output_kind: String,
    pub format: String,
    pub logical_path: String,
    pub producing_job_key: Option<String>,
    pub parameter_schema_version: i64,
    pub config_values_json: String,
}

fn validate_target(
    prepared: &PreparedGeneratorProcessing,
    target: &GeneratorPublicationTarget,
) -> anyhow::Result<()> {
    ensure!(
        target.output_kind
            == format!(
                "slicer_project:{}",
                prepared.recipe().compatibility.dialect_identity
            )
            && target.format == "project_3mf",
        "generator publication requires its exact dialect-qualified project output"
    );
    for value in [&target.logical_path, &target.model_slug] {
        ensure!(
            !value.is_empty()
                && value.len() <= 255
                && value != "."
                && value != ".."
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')),
            "generator artifact presentation must use safe single path segments"
        );
    }
    ensure!(
        target.parameter_schema_version > 0,
        "invalid parameter schema version"
    );
    let values: serde_json::Value = serde_json::from_str(&target.config_values_json)
        .context("invalid artifact configuration metadata")?;
    ensure!(
        values.is_object(),
        "artifact configuration metadata must be an object"
    );
    Ok(())
}

fn validate_deployment(
    generator: &DeployedGenerator,
    prepared: &PreparedGeneratorProcessing,
    target: &GeneratorPublicationTarget,
) -> anyhow::Result<()> {
    validate_target(prepared, target)?;
    let recipe = prepared.recipe();
    ensure!(
        recipe.deployed_generator_identity == generator.static_identity()
            && matches!(
                recipe.compatibility_decision,
                GeneratorCompatibilityDecision::Supported
            ),
        "generator publication requires the configured supported deployment"
    );
    let compatibility = &recipe.compatibility;
    generator.ensure_compatible(&GeneratorCompatibilityRequest {
        protocol_version: compatibility.protocol_version,
        dialect_identity: compatibility.dialect_identity.clone(),
        capability_identities: compatibility.capability_identities.clone(),
        input_kind_identity: compatibility.input_kind_identity.clone(),
        input_schema_identity: compatibility.input_schema_identity.clone(),
        settings_schema_identity: compatibility.settings_schema_identity.clone(),
    })?;
    ensure!(
        recipe.invocation.expected_identities
            == generator.identity_bindings(&recipe.settings_identity),
        "generator publication deployment bindings do not match"
    );
    Ok(())
}

pub fn generator_publication_object_key(
    prepared: &PreparedGeneratorProcessing,
    target: &GeneratorPublicationTarget,
) -> anyhow::Result<String> {
    validate_target(prepared, target)?;
    let hash = generator_artifact_set_hash(prepared, &target.output_kind, &target.format)?;
    Ok(format!("artifacts/v2/{hash}/{}", target.logical_path))
}

struct MeasuredCandidate {
    file: tempfile::NamedTempFile,
    result: GeneratorResult,
    byte_len: i64,
    sha256: String,
}

async fn record_verification_failure(
    db: &Database,
    hash: &str,
    ready_revision: Option<&str>,
) -> anyhow::Result<()> {
    if let Some(revision) = ready_revision {
        db.fail_ready_generator_verification(hash, revision).await?;
    } else {
        db.fail_generator_artifact(hash).await?;
    }
    Ok(())
}

struct MeasuringWriter<'a> {
    file: &'a mut File,
    count: u64,
    limit: u64,
    hash: Sha256,
}

impl Write for MeasuringWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() as u64 > self.limit.saturating_sub(self.count) {
            return Err(std::io::Error::other(
                "accepted candidate exceeds declared length",
            ));
        }
        let count = self.file.write(bytes)?;
        self.count += count as u64;
        self.hash.update(&bytes[..count]);
        Ok(count)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

fn retain_candidate(
    output: &VerifiedGeneratorOutput,
    prepared: &PreparedGeneratorProcessing,
) -> anyhow::Result<MeasuredCandidate> {
    let request = &prepared.recipe().invocation;
    ensure!(
        output.processing_hash() == prepared.processing_hash()
            && output.invocation_identity() == request.invocation_identity,
        "accepted generator output belongs to a different processing invocation"
    );
    let result = output.result().clone();
    ensure!(
        result.status == ResultStatus::Success,
        "generator output is not successful"
    );
    result.validate_against_request(request)?;
    let declaration = result
        .output
        .as_ref()
        .context("successful generator output is absent")?;
    let byte_len = i64::try_from(declaration.byte_length)
        .context("generator candidate length exceeds storage metadata bounds")?;
    let parent =
        PathBuf::from(std::env::var_os("TMPDIR").unwrap_or_else(|| "/tmp".into())).join("agents");
    ensure!(
        parent.is_absolute(),
        "generator retention parent must be absolute"
    );
    std::fs::create_dir_all(&parent)?;
    let mut file = tempfile::Builder::new()
        .prefix("generator-publication-")
        .tempfile_in(parent)?;
    let mut writer = MeasuringWriter {
        file: file.as_file_mut(),
        count: 0,
        limit: declaration.byte_length,
        hash: Sha256::new(),
    };
    output
        .copy_to(&mut writer)
        .context("copying accepted generator bytes")?;
    writer.flush()?;
    let sha256: String = writer
        .hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    ensure!(
        writer.count == declaration.byte_length && sha256 == declaration.sha256,
        "accepted generator candidate length or SHA-256 mismatch before upload"
    );
    file.as_file().sync_all()?;
    Ok(MeasuredCandidate {
        file,
        result,
        byte_len,
        sha256,
    })
}

/// Copy and remeasure only the runner's sealed accepted bytes. The shared handle
/// lets an exact retry retain the same immutable bytes without blocking Tokio.
pub async fn publish_generator_output(
    db: &Database,
    storage: &StorageClient,
    generator: &DeployedGenerator,
    prepared: &PreparedGeneratorProcessing,
    output: Arc<VerifiedGeneratorOutput>,
    target: &GeneratorPublicationTarget,
) -> anyhow::Result<String> {
    validate_deployment(generator, prepared, target)?;
    let checked_prepared = prepared.clone();
    let candidate =
        tokio::task::spawn_blocking(move || retain_candidate(&output, &checked_prepared))
            .await
            .context("generator publication retention task did not complete")??;
    let object_key = generator_publication_object_key(prepared, target)?;
    let content_type = &prepared.recipe().invocation.output.media_type;
    let metadata = serde_json::to_string(&candidate.result)?;
    let staged = db
        .stage_or_resume_generator_artifact(
            prepared,
            GeneratorArtifactStage {
                model_slug: &target.model_slug,
                output_kind: &target.output_kind,
                format: &target.format,
                object_key: &object_key,
                content_type,
                byte_len: candidate.byte_len,
                sha256: &candidate.sha256,
                producing_job_key: target.producing_job_key.as_deref(),
                parameter_schema_version: target.parameter_schema_version,
                config_values_json: &target.config_values_json,
            },
            &[GeneratorArtifactFileInsert {
                role: "generated_project",
                logical_path: &target.logical_path,
                original_path: None,
                object_key: &object_key,
                content_type,
                byte_len: candidate.byte_len,
                sha256: &candidate.sha256,
                metadata_json: &metadata,
            }],
        )
        .await
        .context("staging or resuming immutable generator artifact")?;
    let operation: anyhow::Result<()> = async {
        if staged.status != "ready" {
            storage
                .put_file_with_headers(
                    &object_key,
                    candidate.file.path(),
                    content_type,
                    Some(&format!("attachment; filename=\"{}\"", target.logical_path)),
                    Some("public, max-age=31536000, immutable"),
                )
                .await
                .context("uploading accepted generator bytes")?;
        }
        storage
            .verify_exact_object(
                &object_key,
                content_type,
                candidate.byte_len.try_into()?,
                &candidate.sha256,
            )
            .await
            .context("verifying stored generator bytes")
    }
    .await;
    // No public key ever refers to this private service-owned retention.
    let cleanup = tokio::task::spawn_blocking(move || candidate.file.close())
        .await
        .context("generator publication cleanup task did not complete")?;
    if let Err(error) = operation {
        record_verification_failure(
            db,
            &staged.artifact_set_hash,
            (staged.status == "ready").then_some(staged.updated_at.as_str()),
        )
        .await
        .with_context(|| format!("recording generator upload failure: {error}"))?;
        if let Err(cleanup_error) = cleanup {
            return Err(error.context(format!(
                "private generator retention cleanup also failed: {cleanup_error}"
            )));
        }
        return Err(error);
    }
    if let Err(error) = cleanup {
        db.fail_generator_artifact(&staged.artifact_set_hash)
            .await?;
        return Err(error).context("cleaning private generator retention");
    }
    ensure!(
        db.complete_generator_artifact(prepared, &target.output_kind, &target.format, &object_key)
            .await?,
        "generator artifact was superseded before publication completed"
    );
    Ok(object_key)
}

/// Reconcile an existing immutable record after interruption, with no generator
/// invocation and no upload. Missing or corrupt storage never advances readiness.
pub async fn reconcile_generator_artifact(
    db: &Database,
    storage: &StorageClient,
    generator: &DeployedGenerator,
    prepared: &PreparedGeneratorProcessing,
    target: &GeneratorPublicationTarget,
) -> anyhow::Result<Option<String>> {
    validate_deployment(generator, prepared, target)?;
    let Some((set, file)) = db
        .generator_artifact_for_reconciliation(prepared, &target.output_kind, &target.format)
        .await?
    else {
        return Ok(None);
    };
    let object_key = generator_publication_object_key(prepared, target)?;
    let presentation: serde_json::Value = serde_json::from_str(&set.metadata_json)?;
    ensure!(
        presentation
            == serde_json::json!({
                "modelSlug": target.model_slug,
                "producingJobKey": target.producing_job_key,
                "parameterSchemaVersion": target.parameter_schema_version,
                "configValuesJson": target.config_values_json,
            }),
        "stored generator artifact presentation metadata does not match"
    );
    let metadata = parse_result(file.metadata_json.as_bytes())
        .context("invalid stored generator result metadata")?;
    metadata.validate_against_request(&prepared.recipe().invocation)?;
    let output = metadata
        .output
        .as_ref()
        .context("stored generator result has no output")?;
    ensure!(
        metadata.status == ResultStatus::Success
            && file.role == "generated_project"
            && file.logical_path == target.logical_path
            && file.original_path.is_none()
            && file.object_key == object_key
            && file.content_type == prepared.recipe().invocation.output.media_type
            && u64::try_from(file.byte_len)? == output.byte_length
            && file.sha256 == output.sha256,
        "stored generator artifact declarations do not match publication"
    );
    let verification = storage
        .verify_exact_object(
            &object_key,
            &file.content_type,
            file.byte_len.try_into()?,
            &file.sha256,
        )
        .await;
    if let Err(error) = verification {
        record_verification_failure(
            db,
            &set.artifact_set_hash,
            (set.status == "ready").then_some(set.updated_at.as_str()),
        )
        .await
        .with_context(|| format!("recording generator reconciliation failure: {error}"))?;
        return Err(error.context("verifying stored generator bytes during reconciliation"));
    }
    ensure!(
        db.complete_generator_artifact(prepared, &target.output_kind, &target.format, &object_key)
            .await?,
        "generator artifact was superseded before reconciliation completed"
    );
    Ok(Some(object_key))
}

#[cfg(test)]
#[path = "generator_publication_tests.rs"]
mod tests;
