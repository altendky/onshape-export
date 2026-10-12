//! Durable private copies of already accepted output. The immutable receipt is
//! bound by a trusted database association, never supplied by a public request.
//! No archive or target semantics are inspected here.

use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    cache_key,
    generator_processing::PreparedGeneratorProcessing,
    generator_protocol::{GeneratorResult, ResultStatus},
    generator_runner::VerifiedGeneratorOutput,
};

const RECEIPT_LIMIT: usize = 1_048_576;
const DOMAIN: &str = "onshape-export-accepted-generator-output-v1";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receipt {
    schema_version: u32,
    processing_hash: String,
    invocation_identity: String,
    result: GeneratorResult,
}

/// Private fields and no Deserialize: this can only follow receipt and byte
/// verification against the trusted database identity and prepared recipe.
pub(crate) struct ValidatedCheckpoint {
    receipt: Receipt,
    bytes: File,
}

impl ValidatedCheckpoint {
    pub(crate) fn into_parts(self) -> (GeneratorResult, String, String, File) {
        (
            self.receipt.result,
            self.receipt.processing_hash,
            self.receipt.invocation_identity,
            self.bytes,
        )
    }
}

pub(crate) fn outside_repository(path: &Path) -> bool {
    !path.ancestors().any(|parent| {
        let marker = parent.join(".git");
        fs::symlink_metadata(&marker).is_ok_and(|metadata| {
            // Worktrees use a gitdir file; normal repositories contain HEAD.
            // An empty directory bearing this name is not a Git checkout.
            !metadata.is_dir() || marker.join("HEAD").exists()
        })
    })
}

fn root_check(root: &Path) -> anyhow::Result<()> {
    ensure!(
        root.is_absolute()
            && !root
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir)),
        "output retention requires an absolute path without parent traversal"
    );
    ensure!(
        outside_repository(root),
        "output retention must remain outside a repository"
    );
    let mut missing = Vec::new();
    for path in root.ancestors() {
        if path.exists() {
            ensure!(
                outside_repository(&fs::canonicalize(path)?.join(root.strip_prefix(path)?)),
                "output retention must remain outside a repository"
            );
            break;
        }
        missing.push(path);
    }
    for path in missing.iter().rev() {
        match fs::DirBuilder::new().mode(0o700).create(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    let metadata = fs::symlink_metadata(root)?;
    ensure!(
        metadata.is_dir() && metadata.permissions().mode() & 0o077 == 0,
        "output retention requires a private regular directory"
    );
    ensure!(
        outside_repository(&fs::canonicalize(root)?),
        "output retention must remain outside a repository"
    );
    // Also sync existing ancestry: another producer may have observed newly
    // created directories before the creator made their parent entries durable.
    // The later database association must not outlive that directory evidence.
    for path in root.ancestors() {
        File::open(path)?.sync_all()?;
    }
    Ok(())
}

fn regular_file(path: &Path) -> anyhow::Result<File> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file() && metadata.permissions().mode() & 0o077 == 0,
        "output checkpoint requires private regular files"
    );
    Ok(File::open(path)?)
}

fn validate(receipt: &Receipt, prepared: &PreparedGeneratorProcessing) -> anyhow::Result<()> {
    ensure!(
        receipt.schema_version == 1
            && receipt.processing_hash == prepared.processing_hash()
            && receipt.invocation_identity == prepared.recipe().invocation.invocation_identity
            && receipt.result.status == ResultStatus::Success,
        "output checkpoint does not match its successful invocation"
    );
    receipt
        .result
        .validate_against_request(&prepared.recipe().invocation)?;
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
            return Err(std::io::Error::other("output checkpoint length mismatch"));
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

fn measure_copy(
    file: &mut File,
    receipt: &Receipt,
    copy: impl FnOnce(&mut MeasuringWriter<'_>) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let declaration = receipt
        .result
        .output
        .as_ref()
        .context("checkpoint output missing")?;
    let mut writer = MeasuringWriter {
        file,
        count: 0,
        limit: declaration.byte_length,
        hash: Sha256::new(),
    };
    copy(&mut writer)?;
    writer.flush()?;
    let hash: String = writer
        .hash
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    ensure!(
        writer.count == declaration.byte_length && hash == declaration.sha256,
        "output checkpoint byte evidence mismatch"
    );
    Ok(())
}

fn reopen_blocking(
    root: &Path,
    identity: &str,
    prepared: &PreparedGeneratorProcessing,
) -> anyhow::Result<Arc<VerifiedGeneratorOutput>> {
    root_check(root)?;
    ensure!(
        crate::onshape_api::is_sha256(identity),
        "invalid output checkpoint identity"
    );
    let directory = root.join(identity);
    let metadata = fs::symlink_metadata(&directory)?;
    ensure!(
        metadata.is_dir() && metadata.permissions().mode() & 0o077 == 0,
        "output checkpoint requires a private regular directory"
    );
    let mut bytes = Vec::new();
    regular_file(&directory.join("receipt.json"))?
        .take(RECEIPT_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= RECEIPT_LIMIT,
        "output checkpoint receipt exceeds its bound"
    );
    let value = crate::onshape_api::parse_json(&bytes, RECEIPT_LIMIT)?;
    let receipt: Receipt = serde_json::from_value(value)?;
    ensure!(
        cache_key::canonical_json_bytes(&receipt)? == bytes
            && cache_key::hash_json(DOMAIN, &receipt)? == identity,
        "output checkpoint receipt integrity mismatch"
    );
    validate(&receipt, prepared)?;
    let mut source = regular_file(&directory.join("output.bin"))?;
    let mut retained = tempfile::tempfile_in(root)?;
    measure_copy(&mut retained, &receipt, |writer| {
        std::io::copy(&mut source, writer)?;
        Ok(())
    })?;
    retained.seek(SeekFrom::Start(0))?;
    Ok(Arc::new(VerifiedGeneratorOutput::from_checkpoint(
        ValidatedCheckpoint {
            receipt,
            bytes: retained,
        },
    )))
}

fn persist_blocking(
    root: &Path,
    output: &VerifiedGeneratorOutput,
    prepared: &PreparedGeneratorProcessing,
) -> anyhow::Result<String> {
    root_check(root)?;
    let receipt = Receipt {
        schema_version: 1,
        processing_hash: output.processing_hash().to_owned(),
        invocation_identity: output.invocation_identity().to_owned(),
        result: output.result().clone(),
    };
    validate(&receipt, prepared)?;
    let bytes = cache_key::canonical_json_bytes(&receipt)?;
    ensure!(
        bytes.len() <= RECEIPT_LIMIT,
        "output checkpoint receipt exceeds its bound"
    );
    let identity = cache_key::hash_json(DOMAIN, &receipt)?;
    let temporary = tempfile::Builder::new()
        .prefix("accepted-")
        .tempdir_in(root)?;
    fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700))?;
    let mut payload = File::create(temporary.path().join("output.bin"))?;
    payload.set_permissions(fs::Permissions::from_mode(0o600))?;
    measure_copy(&mut payload, &receipt, |writer| {
        output.copy_to(writer)?;
        Ok(())
    })?;
    payload.sync_all()?;
    let mut metadata = File::create(temporary.path().join("receipt.json"))?;
    metadata.set_permissions(fs::Permissions::from_mode(0o600))?;
    metadata.write_all(&bytes)?;
    metadata.sync_all()?;
    File::open(temporary.path())?.sync_all()?;
    let destination = root.join(&identity);
    if fs::symlink_metadata(&destination).is_ok() {
        reopen_blocking(root, &identity, prepared)?;
    } else if let Err(error) = fs::rename(temporary.path(), &destination) {
        // Another identical producer may commit first. It is only reusable after
        // full receipt and byte validation; malformed evidence is never replaced.
        if fs::symlink_metadata(&destination).is_err() {
            return Err(error.into());
        }
        reopen_blocking(root, &identity, prepared)?;
    }
    File::open(root)?.sync_all()?;
    Ok(identity)
}

pub(crate) async fn persist(
    root: &Path,
    output: Arc<VerifiedGeneratorOutput>,
    prepared: &PreparedGeneratorProcessing,
) -> anyhow::Result<String> {
    let root: PathBuf = root.to_owned();
    let prepared = prepared.clone();
    tokio::task::spawn_blocking(move || persist_blocking(&root, &output, &prepared)).await?
}

pub(crate) async fn reopen(
    root: &Path,
    identity: &str,
    prepared: &PreparedGeneratorProcessing,
) -> anyhow::Result<Arc<VerifiedGeneratorOutput>> {
    let root = root.to_owned();
    let identity = identity.to_owned();
    let prepared = prepared.clone();
    tokio::task::spawn_blocking(move || reopen_blocking(&root, &identity, &prepared)).await?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator_runner::tests::RunnerFixture;

    fn temporary_directory() -> tempfile::TempDir {
        let parent = PathBuf::from(std::env::var_os("TMPDIR").unwrap_or_else(|| "/tmp".into()))
            .join("agents");
        fs::create_dir_all(&parent).unwrap();
        tempfile::tempdir_in(parent).unwrap()
    }

    #[tokio::test]
    async fn sealed_output_survives_restart_and_rejects_corruption_or_recipe_changes() {
        let mut fixture = RunnerFixture::new("success").await;
        let output = Arc::new(fixture.run().await.unwrap());
        fixture.assert_clean();
        let temporary = temporary_directory();
        let root = temporary.path().join("checkpoints");
        let id = persist(&root, Arc::clone(&output), &fixture.prepared)
            .await
            .unwrap();
        assert_eq!(persist(&root, output, &fixture.prepared).await.unwrap(), id);
        let reopened = reopen(&root, &id, &fixture.prepared).await.unwrap();
        let mut measured = Vec::new();
        reopened.copy_to(&mut measured).unwrap();
        assert_eq!(measured, b"synthetic generated project");
        let changed = RunnerFixture::new("malformed").await;
        assert!(reopen(&root, &id, &changed.prepared).await.is_err());
        fs::write(root.join(&id).join("output.bin"), b"wrong bytes").unwrap();
        assert!(reopen(&root, &id, &fixture.prepared).await.is_err());
    }

    #[tokio::test]
    async fn repository_and_nonprivate_or_traversal_checkpoint_paths_are_rejected() {
        let fixture = RunnerFixture::new("success").await;
        let temporary = temporary_directory();
        let root = temporary.path().join("checkpoints");
        assert!(
            reopen(&root, "../untrusted", &fixture.prepared)
                .await
                .is_err()
        );
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            reopen(&root, &"a".repeat(64), &fixture.prepared)
                .await
                .is_err()
        );
        let repository = temporary.path().join("checkout");
        fs::create_dir_all(repository.join(".git")).unwrap();
        fs::write(repository.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        assert!(
            reopen(
                &repository.join("checkpoints"),
                &"a".repeat(64),
                &fixture.prepared
            )
            .await
            .is_err()
        );
        let alias = temporary.path().join("alias");
        std::os::unix::fs::symlink(&repository, &alias).unwrap();
        assert!(
            reopen(
                &alias.join("checkpoints"),
                &"a".repeat(64),
                &fixture.prepared
            )
            .await
            .is_err()
        );
        assert!(!repository.join("checkpoints").exists());
    }

    #[tokio::test]
    async fn tampered_receipt_and_symlinked_output_are_rejected() {
        let mut fixture = RunnerFixture::new("success").await;
        let output = Arc::new(fixture.run().await.unwrap());
        let temporary = temporary_directory();
        let root = temporary.path().join("checkpoints");
        let id = persist(&root, output, &fixture.prepared).await.unwrap();
        let receipt_path = root.join(&id).join("receipt.json");
        let original = fs::read(&receipt_path).unwrap();
        let mut receipt: Receipt = serde_json::from_slice(&original).unwrap();
        receipt.invocation_identity = "changed".to_owned();
        fs::write(
            &receipt_path,
            cache_key::canonical_json_bytes(&receipt).unwrap(),
        )
        .unwrap();
        assert!(reopen(&root, &id, &fixture.prepared).await.is_err());
        fs::write(&receipt_path, original).unwrap();
        let payload = root.join(&id).join("output.bin");
        fs::rename(&payload, temporary.path().join("outside.bin")).unwrap();
        std::os::unix::fs::symlink(temporary.path().join("outside.bin"), &payload).unwrap();
        assert!(reopen(&root, &id, &fixture.prepared).await.is_err());
    }

    #[tokio::test]
    async fn concurrent_producers_reuse_one_complete_immutable_checkpoint() {
        let mut fixture = RunnerFixture::new("success").await;
        let output = Arc::new(fixture.run().await.unwrap());
        let temporary = temporary_directory();
        let root = temporary.path().join("new-base/namespace");
        let (first, second) = tokio::join!(
            persist(&root, Arc::clone(&output), &fixture.prepared),
            persist(&root, output, &fixture.prepared),
        );
        let identity = first.unwrap();
        assert_eq!(second.unwrap(), identity);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        let reopened = reopen(&root, &identity, &fixture.prepared).await.unwrap();
        let mut bytes = Vec::new();
        reopened.copy_to(&mut bytes).unwrap();
        assert_eq!(bytes, b"synthetic generated project");
    }
}
