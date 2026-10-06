//! Direct invocation of one configured trusted CLI. No target interpretation,
//! cache writes, publication, or generator-controlled paths leave this module.

use std::{
    collections::HashSet,
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    os::unix::process::ExitStatusExt,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};

use sha2::{Digest, Sha256};
use tokio::{process::Command, sync::oneshot};

use crate::{
    cache_key,
    deployed_generator::{
        DeployedGenerator, DeployedGeneratorError, GeneratorCompatibilityRequest,
    },
    generator_inputs::ConstructedGeneratorInputs,
    generator_processing::{GeneratorCompatibilityDecision, PreparedGeneratorProcessing},
    generator_protocol::{self, GeneratorRequest, GeneratorResult, ResultStatus},
};

pub const DEFAULT_EXECUTION_TIMEOUT: Duration = Duration::from_secs(600);
const REQUEST_PATH: &str = "request.json";
const RESULT_PATH: &str = "result.json";

/// Service-owned operational choices. Neither belongs to processing identity.
pub struct GeneratorRunnerOptions {
    pub scratch_parent: PathBuf,
    pub timeout: Duration,
}

impl Default for GeneratorRunnerOptions {
    fn default() -> Self {
        Self {
            scratch_parent: PathBuf::from(
                std::env::var_os("TMPDIR").unwrap_or_else(|| "/tmp".into()),
            )
            .join("agents"),
            timeout: DEFAULT_EXECUTION_TIMEOUT,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GeneratorRunnerError {
    #[error("generator invocation does not match its trusted inputs and processing recipe")]
    InvalidInvocation,
    #[error("the configured generator does not support this invocation")]
    UnsupportedCombination,
    #[error("generator deployment integrity check failed: {0}")]
    DeploymentIntegrity(#[source] DeployedGeneratorError),
    #[error("generator process could not start")]
    Spawn,
    #[error("generator process crashed with signal {signal}")]
    Crash { signal: i32 },
    #[error("generator exit code {code} does not match the CLI contract")]
    UnexpectedExit { code: i32 },
    #[error("generator execution timed out")]
    Timeout,
    #[error("generator result is missing")]
    MissingResult,
    #[error("generator result is malformed or exceeds protocol bounds")]
    MalformedResult,
    #[error("generator reported identities do not match the configured invocation")]
    ResultIdentityMismatch,
    #[error("generator output declaration does not match the request")]
    OutputDeclarationMismatch,
    #[error("invocation contains missing, undeclared, temporary, or nonregular files")]
    UnexpectedFiles,
    #[error("generator candidate is missing")]
    CandidateMissing,
    #[error("generator candidate length or SHA-256 does not match its declaration")]
    CandidateMismatch,
    #[error("generator inputs could not be staged or verified")]
    Staging,
    #[error("generator candidate retention could not be read or written")]
    Io,
    #[error("generator child could not be terminated and reaped")]
    ProcessCleanup { invocation_root: PathBuf },
    #[error("generator invocation directory cleanup failed")]
    Cleanup {
        #[source]
        prior: Option<Box<GeneratorRunnerError>>,
    },
    #[error("generator invocation was cancelled")]
    Cancelled,
    #[error("generator supervisor did not complete")]
    Supervisor,
    #[error("generator reported structured failure")]
    StructuredFailure { result: Box<GeneratorResult> },
}

/// Only successful measurement plus explicit root cleanup constructs this type.
/// The private anonymous file contains copied service-owned accepted bytes.
pub struct VerifiedGeneratorOutput {
    result: GeneratorResult,
    invocation_identity: String,
    processing_hash: String,
    bytes: Mutex<File>,
}

impl VerifiedGeneratorOutput {
    pub fn result(&self) -> &GeneratorResult {
        &self.result
    }

    pub fn invocation_identity(&self) -> &str {
        &self.invocation_identity
    }

    pub fn processing_hash(&self) -> &str {
        &self.processing_hash
    }

    pub fn copy_to(&self, output: &mut impl Write) -> Result<(), GeneratorRunnerError> {
        let mut file = self.bytes.lock().map_err(|_| GeneratorRunnerError::Io)?;
        file.seek(SeekFrom::Start(0))
            .map_err(|_| GeneratorRunnerError::Io)?;
        let count = std::io::copy(&mut *file, output).map_err(|_| GeneratorRunnerError::Io)?;
        if Some(count) != self.result.output.as_ref().map(|output| output.byte_length) {
            return Err(GeneratorRunnerError::CandidateMismatch);
        }
        Ok(())
    }
}

struct CancelOnDrop(Option<oneshot::Sender<()>>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

/// Cancelling this caller signals a detached supervisor; it still owns and
/// reaps the child and explicitly cleans the root before releasing resources.
pub async fn run_generator(
    generator: &DeployedGenerator,
    inputs: ConstructedGeneratorInputs,
    prepared: &PreparedGeneratorProcessing,
    options: &GeneratorRunnerOptions,
) -> Result<VerifiedGeneratorOutput, GeneratorRunnerError> {
    let generator = generator.clone();
    let prepared = prepared.clone();
    let inputs = Arc::new(inputs);
    let scratch_parent = options.scratch_parent.clone();
    let timeout = options.timeout;
    let (sender, receiver) = oneshot::channel();
    let mut cancellation = CancelOnDrop(Some(sender));
    let supervisor = tokio::spawn(async move {
        let result = supervise(
            generator,
            inputs,
            prepared,
            scratch_parent,
            timeout,
            receiver,
        )
        .await;
        if let Err(GeneratorRunnerError::ProcessCleanup { invocation_root }) = &result {
            // Service-owned recovery location; no generator diagnostics/payloads.
            tracing::error!(
                ?invocation_root,
                "trusted generator child cleanup is unconfirmed; root retained"
            );
        } else if matches!(result, Err(GeneratorRunnerError::Cleanup { .. })) {
            tracing::error!("trusted generator cleanup did not complete");
        }
        result
    });
    let result = supervisor
        .await
        .map_err(|_| GeneratorRunnerError::Supervisor);
    cancellation.0.take();
    result?
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, GeneratorRunnerError> + Send + 'static,
) -> Result<T, GeneratorRunnerError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| GeneratorRunnerError::Supervisor)?
}

fn cancelled(receiver: &mut oneshot::Receiver<()>) -> bool {
    !matches!(
        receiver.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    )
}

async fn supervise(
    generator: DeployedGenerator,
    inputs: Arc<ConstructedGeneratorInputs>,
    prepared: PreparedGeneratorProcessing,
    parent: PathBuf,
    timeout: Duration,
    mut cancellation: oneshot::Receiver<()>,
) -> Result<VerifiedGeneratorOutput, GeneratorRunnerError> {
    let checked_generator = generator.clone();
    let checked_inputs = Arc::clone(&inputs);
    let checked_prepared = prepared.clone();
    let absolute_parent = parent.is_absolute();
    let (request, layout) = blocking(move || {
        if !absolute_parent
            || timeout.is_zero()
            || tokio::time::Instant::now().checked_add(timeout).is_none()
        {
            return Err(GeneratorRunnerError::InvalidInvocation);
        }
        preflight(&checked_generator, &checked_inputs, &checked_prepared)
    })
    .await?;
    if cancelled(&mut cancellation) {
        return Err(GeneratorRunnerError::Cancelled);
    }
    let root = blocking(move || {
        fs::create_dir_all(&parent).map_err(|_| GeneratorRunnerError::Staging)?;
        tempfile::Builder::new()
            .prefix("generator-")
            .tempdir_in(parent)
            .map_err(|_| GeneratorRunnerError::Staging)
    })
    .await?;
    let root_path = root.path().to_owned();
    let outcome = execute(
        generator,
        inputs,
        prepared,
        request,
        layout,
        root_path,
        timeout,
        &mut cancellation,
    )
    .await;
    finish_cleanup(root, outcome).await
}

async fn finish_cleanup(
    root: tempfile::TempDir,
    outcome: Result<VerifiedGeneratorOutput, GeneratorRunnerError>,
) -> Result<VerifiedGeneratorOutput, GeneratorRunnerError> {
    if matches!(outcome, Err(GeneratorRunnerError::ProcessCleanup { .. })) {
        // The OS has not confirmed the child's termination/reaping. Preserve
        // its working root rather than deleting files under a possibly live CLI.
        let _ = root.keep();
        return outcome;
    }
    let cleanup = blocking(move || {
        root.close()
            .map_err(|_| GeneratorRunnerError::Cleanup { prior: None })
    })
    .await;
    match cleanup {
        Ok(()) => outcome,
        Err(_) => Err(GeneratorRunnerError::Cleanup {
            prior: outcome.err().map(Box::new),
        }),
    }
}

struct Layout {
    files: HashSet<PathBuf>,
    directories: HashSet<PathBuf>,
    candidate: PathBuf,
}

impl Layout {
    fn new(
        inputs: &ConstructedGeneratorInputs,
        request: &GeneratorRequest,
    ) -> Result<Self, GeneratorRunnerError> {
        let mut files = HashSet::new();
        for path in std::iter::once(REQUEST_PATH)
            .chain(std::iter::once(RESULT_PATH))
            .chain(std::iter::once(request.input_manifest.path.as_str()))
            .chain(std::iter::once(request.settings.content.path.as_str()))
            .chain(std::iter::once(request.output.path.as_str()))
            .chain(
                inputs
                    .manifest()
                    .objects
                    .iter()
                    .map(|object| object.retained_content.path.as_str()),
            )
        {
            if !files.insert(PathBuf::from(path)) {
                return Err(GeneratorRunnerError::InvalidInvocation);
            }
        }
        let mut directories = HashSet::new();
        for file in &files {
            let mut parent = file.parent();
            while let Some(path) = parent.filter(|path| !path.as_os_str().is_empty()) {
                if files.contains(path) {
                    return Err(GeneratorRunnerError::InvalidInvocation);
                }
                directories.insert(path.to_owned());
                parent = path.parent();
            }
        }
        Ok(Self {
            files,
            directories,
            candidate: PathBuf::from(&request.output.path),
        })
    }

    fn verify(&self, root: &Path, result_required: bool) -> Result<(), GeneratorRunnerError> {
        let mut remaining = self.files.clone();
        remaining.remove(&self.candidate);
        if !result_required {
            remaining.remove(Path::new(RESULT_PATH));
        }
        let mut pending = vec![PathBuf::new()];
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(root.join(&directory))
                .map_err(|_| GeneratorRunnerError::UnexpectedFiles)?
            {
                let entry = entry.map_err(|_| GeneratorRunnerError::UnexpectedFiles)?;
                let relative = directory.join(entry.file_name());
                let kind = entry
                    .file_type()
                    .map_err(|_| GeneratorRunnerError::UnexpectedFiles)?;
                if kind.is_dir() && self.directories.contains(&relative) {
                    pending.push(relative);
                } else if kind.is_file() && self.files.contains(&relative) {
                    remaining.remove(&relative);
                } else {
                    return Err(GeneratorRunnerError::UnexpectedFiles);
                }
            }
        }
        if !remaining.is_empty() {
            return Err(GeneratorRunnerError::UnexpectedFiles);
        }
        if !result_required
            && (root.join(RESULT_PATH).exists() || root.join(&self.candidate).exists())
        {
            return Err(GeneratorRunnerError::UnexpectedFiles);
        }
        Ok(())
    }
}

fn preflight(
    generator: &DeployedGenerator,
    inputs: &ConstructedGeneratorInputs,
    prepared: &PreparedGeneratorProcessing,
) -> Result<(GeneratorRequest, Layout), GeneratorRunnerError> {
    let recipe = prepared.recipe();
    if !matches!(
        recipe.compatibility_decision,
        GeneratorCompatibilityDecision::Supported
    ) {
        return Err(GeneratorRunnerError::UnsupportedCombination);
    }
    let compatibility = &recipe.compatibility;
    generator
        .ensure_compatible(&GeneratorCompatibilityRequest {
            protocol_version: compatibility.protocol_version,
            dialect_identity: compatibility.dialect_identity.clone(),
            capability_identities: compatibility.capability_identities.clone(),
            input_kind_identity: compatibility.input_kind_identity.clone(),
            input_schema_identity: compatibility.input_schema_identity.clone(),
            settings_schema_identity: compatibility.settings_schema_identity.clone(),
        })
        .map_err(|_| GeneratorRunnerError::UnsupportedCombination)?;
    let request = &recipe.invocation;
    if recipe.deployed_generator_identity != generator.static_identity()
        || recipe.input_manifest != *inputs.manifest()
        || recipe.settings != *inputs.settings()
        || recipe.settings_identity != inputs.settings_identity()
        || recipe.settings_schema_identity != inputs.settings_schema_identity()
        || request.protocol_version != generator.document().protocol_version
        || request.expected_identities != generator.identity_bindings(inputs.settings_identity())
        || inputs.validate_request(request).is_err()
    {
        return Err(GeneratorRunnerError::InvalidInvocation);
    }
    let layout = Layout::new(inputs, request)?;
    Ok((request.clone(), layout))
}

struct MeasuringWriter<W> {
    output: W,
    count: u64,
    hasher: Sha256,
    limit: u64,
}

fn measured_sha256(hasher: Sha256) -> String {
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

impl<W: Write> Write for MeasuringWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() as u64 > self.limit.saturating_sub(self.count) {
            return Err(std::io::Error::other("declared byte limit"));
        }
        let count = self.output.write(bytes)?;
        self.hasher.update(&bytes[..count]);
        self.count += count as u64;
        Ok(count)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.output.flush()
    }
}

fn stage_file(
    root: &Path,
    relative: &Path,
    copy: impl FnOnce(&mut MeasuringWriter<&mut File>) -> Result<(), GeneratorRunnerError>,
    expected: Option<(u64, &str)>,
) -> Result<(), GeneratorRunnerError> {
    let destination = root.join(relative);
    let parent = destination.parent().ok_or(GeneratorRunnerError::Staging)?;
    fs::create_dir_all(parent).map_err(|_| GeneratorRunnerError::Staging)?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|_| GeneratorRunnerError::Staging)?;
    let mut writer = MeasuringWriter {
        output: temporary.as_file_mut(),
        count: 0,
        hasher: Sha256::new(),
        limit: expected.map_or(u64::MAX, |(length, _)| length),
    };
    copy(&mut writer)?;
    writer.flush().map_err(|_| GeneratorRunnerError::Staging)?;
    if let Some((length, hash)) = expected
        && (writer.count != length || measured_sha256(writer.hasher) != hash)
    {
        return Err(GeneratorRunnerError::Staging);
    }
    temporary
        .as_file()
        .sync_all()
        .map_err(|_| GeneratorRunnerError::Staging)?;
    let path = temporary.into_temp_path(); // Close before atomically installing.
    fs::rename(&path, destination).map_err(|_| GeneratorRunnerError::Staging)?;
    Ok(())
}

fn stage(
    root: &Path,
    inputs: &ConstructedGeneratorInputs,
    request: &GeneratorRequest,
    layout: &Layout,
) -> Result<(), GeneratorRunnerError> {
    for directory in &layout.directories {
        fs::create_dir_all(root.join(directory)).map_err(|_| GeneratorRunnerError::Staging)?;
    }
    for (position, object) in inputs.manifest().objects.iter().enumerate() {
        let payload = inputs
            .payload(position)
            .ok_or(GeneratorRunnerError::Staging)?;
        stage_file(
            root,
            Path::new(&object.retained_content.path),
            |writer| {
                payload
                    .copy_to(writer)
                    .map_err(|_| GeneratorRunnerError::Staging)
            },
            Some((
                object.retained_content.byte_length,
                &object.retained_content.sha256,
            )),
        )?;
    }
    for (path, bytes) in [
        (
            request.input_manifest.path.as_str(),
            cache_key::canonical_json_bytes(inputs.manifest())
                .map_err(|_| GeneratorRunnerError::Staging)?,
        ),
        (
            request.settings.content.path.as_str(),
            inputs.settings_bytes().to_vec(),
        ),
        (
            REQUEST_PATH,
            cache_key::canonical_json_bytes(request).map_err(|_| GeneratorRunnerError::Staging)?,
        ),
    ] {
        stage_file(
            root,
            Path::new(path),
            |writer| {
                writer
                    .write_all(&bytes)
                    .map_err(|_| GeneratorRunnerError::Staging)
            },
            Some((bytes.len() as u64, &cache_key::hex_sha256(&bytes))),
        )?;
    }
    layout.verify(root, false)
}

#[allow(clippy::too_many_arguments)]
async fn execute(
    generator: DeployedGenerator,
    inputs: Arc<ConstructedGeneratorInputs>,
    prepared: PreparedGeneratorProcessing,
    request: GeneratorRequest,
    layout: Layout,
    root: PathBuf,
    timeout: Duration,
    cancellation: &mut oneshot::Receiver<()>,
) -> Result<VerifiedGeneratorOutput, GeneratorRunnerError> {
    let staged_root = root.clone();
    let staged_request = request.clone();
    let layout = Arc::new(layout);
    let staged_layout = Arc::clone(&layout);
    blocking(move || stage(&staged_root, &inputs, &staged_request, &staged_layout)).await?;
    if cancelled(cancellation) {
        return Err(GeneratorRunnerError::Cancelled);
    }
    let checked_generator = generator.clone();
    blocking(move || {
        checked_generator
            .verify_executable()
            .map_err(GeneratorRunnerError::DeploymentIntegrity)
    })
    .await?;
    if cancelled(cancellation) {
        return Err(GeneratorRunnerError::Cancelled);
    }
    let mut child = Command::new(&generator.document().executable_path)
        .args(["--request", REQUEST_PATH, "--result", RESULT_PATH])
        .current_dir(&root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| GeneratorRunnerError::Spawn)?;
    let status = tokio::select! {
        status = child.wait() => status.map_err(|_| GeneratorRunnerError::ProcessCleanup {
            invocation_root: root.clone(),
        })?,
        _ = tokio::time::sleep(timeout) => {
            child.kill().await.map_err(|_| GeneratorRunnerError::ProcessCleanup {
                invocation_root: root.clone(),
            })?;
            return Err(GeneratorRunnerError::Timeout);
        }
        _ = cancellation => {
            child.kill().await.map_err(|_| GeneratorRunnerError::ProcessCleanup {
                invocation_root: root.clone(),
            })?;
            return Err(GeneratorRunnerError::Cancelled);
        }
    };
    if let Some(signal) = status.signal() {
        return Err(GeneratorRunnerError::Crash { signal });
    }
    let code = status
        .code()
        .ok_or(GeneratorRunnerError::UnexpectedExit { code: -1 })?;
    if !matches!(code, 0 | 1) {
        return Err(GeneratorRunnerError::UnexpectedExit { code });
    }
    blocking(move || accept_final(&root, &layout, &request, &prepared, code)).await
}

fn accept_final(
    root: &Path,
    layout: &Layout,
    request: &GeneratorRequest,
    prepared: &PreparedGeneratorProcessing,
    code: i32,
) -> Result<VerifiedGeneratorOutput, GeneratorRunnerError> {
    let result_path = root.join(RESULT_PATH);
    let metadata = fs::symlink_metadata(&result_path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            GeneratorRunnerError::MissingResult
        } else {
            GeneratorRunnerError::MalformedResult
        }
    })?;
    if !metadata.is_file() {
        return Err(GeneratorRunnerError::UnexpectedFiles);
    }
    let mut bytes = Vec::new();
    File::open(result_path)
        .map_err(|_| GeneratorRunnerError::MalformedResult)?
        .take(generator_protocol::MAX_RESULT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| GeneratorRunnerError::MalformedResult)?;
    if bytes.len() > generator_protocol::MAX_RESULT_BYTES {
        return Err(GeneratorRunnerError::MalformedResult);
    }
    let result: GeneratorResult =
        serde_json::from_slice(&bytes).map_err(|_| GeneratorRunnerError::MalformedResult)?;
    if result.protocol_version != request.protocol_version
        || result
            .invocation_identity
            .as_ref()
            .is_some_and(|identity| identity != &request.invocation_identity)
        || result
            .reported_identities
            .as_ref()
            .is_some_and(|identities| identities != &request.expected_identities)
    {
        return Err(GeneratorRunnerError::ResultIdentityMismatch);
    }
    generator_protocol::parse_result(&bytes).map_err(|_| GeneratorRunnerError::MalformedResult)?;
    result
        .validate_against_request(request)
        .map_err(|_| GeneratorRunnerError::OutputDeclarationMismatch)?;
    if (code == 0) != (result.status == ResultStatus::Success) {
        return Err(GeneratorRunnerError::UnexpectedExit { code });
    }
    layout.verify(root, true)?;
    if result.status == ResultStatus::Failure {
        return Err(GeneratorRunnerError::StructuredFailure {
            result: Box::new(result),
        });
    }
    let output = result
        .output
        .as_ref()
        .ok_or(GeneratorRunnerError::MalformedResult)?;
    let candidate = root.join(&layout.candidate);
    let metadata = fs::symlink_metadata(&candidate).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            GeneratorRunnerError::CandidateMissing
        } else {
            GeneratorRunnerError::Io
        }
    })?;
    if !metadata.is_file() {
        return Err(GeneratorRunnerError::UnexpectedFiles);
    }
    if metadata.len() != output.byte_length || metadata.len() > request.output.max_byte_length {
        return Err(GeneratorRunnerError::CandidateMismatch);
    }
    let mut candidate = File::open(candidate).map_err(|_| GeneratorRunnerError::Io)?;
    let retained = tempfile::tempfile_in(root.parent().ok_or(GeneratorRunnerError::Io)?)
        .map_err(|_| GeneratorRunnerError::Io)?;
    let mut writer = MeasuringWriter {
        output: retained,
        count: 0,
        hasher: Sha256::new(),
        limit: output.byte_length,
    };
    std::io::copy(&mut candidate, &mut writer)
        .map_err(|_| GeneratorRunnerError::CandidateMismatch)?;
    writer.flush().map_err(|_| GeneratorRunnerError::Io)?;
    if writer.count != output.byte_length || measured_sha256(writer.hasher) != output.sha256 {
        return Err(GeneratorRunnerError::CandidateMismatch);
    }
    Ok(VerifiedGeneratorOutput {
        result,
        invocation_identity: request.invocation_identity.clone(),
        processing_hash: prepared.processing_hash().to_owned(),
        bytes: Mutex::new(writer.output),
    })
}

#[cfg(test)]
#[path = "generator_runner_tests.rs"]
mod tests;
