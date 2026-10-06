use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

use serde_json::Value;

use super::*;
use crate::{
    cache_key,
    deployed_generator::{DeployedGeneratorDocument, GeneratorCompatibilityRequest},
    generator_inputs::ConstructedGeneratorInputs,
    generator_processing::{
        GeneratorProcessingProtocolInputs, PreparedGeneratorProcessing,
        prepare_generator_processing,
    },
    generator_protocol::{OutputDeclaration, OutputRole, ResultStatus},
};

const CANDIDATE: &[u8] = b"synthetic generated project";

// This fixture interprets only the neutral service protocol. Its executable
// filename and argument assertions exercise direct argv invocation; its output
// deliberately has no target archive, project, or geometry semantics.
const FIXTURE: &str = r#"#!/usr/bin/env python3
import hashlib
import json
import os
from pathlib import Path
import signal
import sys
import time

mode = __MODE__
marker = Path(__MARKER__)
marker.write_text(json.dumps({'pid': os.getpid(), 'root': os.getcwd()}))
assert sys.argv[1:] == ['--request', 'request.json', '--result', 'result.json']
assert not Path('result.json').exists()
request = json.loads(Path('request.json').read_bytes())
manifest = json.loads(Path(request['inputManifest']['path']).read_bytes())
objects = manifest['objects']
assert len(objects) == 3
paths = [obj['retainedContent']['path'] for obj in objects]
assert len(set(paths)) == len(paths)
contents = []
for obj in objects:
    content = obj['retainedContent']
    raw = Path(content['path']).read_bytes()
    assert len(raw) == content['byteLength']
    assert hashlib.sha256(raw).hexdigest() == content['sha256']
    contents.append(raw)
assert contents[0] == contents[1] == contents[2]
assert objects[0]['objectIdentity'] != objects[2]['objectIdentity']
settings = Path(request['settings']['content']['path']).read_bytes()
assert hashlib.sha256(settings).hexdigest() == request['settings']['content']['sha256']
assert len(settings) == request['settings']['content']['byteLength']
assert request['expectedIdentities']['settingsIdentity'] == request['settings']['settingsIdentity']
output = Path(request['output']['path'])
assert not output.exists()

def atomic_write(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + '.partial')
    with temporary.open('xb') as handle:
        handle.write(data)
    temporary.replace(path)

if mode == 'timeout':
    time.sleep(60)
if mode == 'crash':
    os.kill(os.getpid(), signal.SIGTERM)
if mode == 'unexpected-exit':
    sys.exit(7)
if mode == 'missing-result':
    sys.exit(0)
if mode == 'candidate-without-result':
    atomic_write(output, b'synthetic generated project')
    sys.exit(0)
if mode == 'malformed':
    atomic_write(Path('result.json'), b'{')
    sys.exit(0)
if mode == 'oversize-result':
    atomic_write(Path('result.json'), b' ' * 262145)
    sys.exit(0)

result = {
    'documentType': 'generatorResult',
    'protocolVersion': request['protocolVersion'],
    'status': 'success',
    'invocationIdentity': request['invocationIdentity'],
    'reportedIdentities': dict(request['expectedIdentities']),
    'output': {
        'outputIdentity': request['output']['outputIdentity'],
        'role': request['output']['role'],
        'path': request['output']['path'],
        'mediaType': request['output']['mediaType'],
        'byteLength': len(b'synthetic generated project'),
        'sha256': hashlib.sha256(b'synthetic generated project').hexdigest(),
    },
    'diagnostics': [],
    'errors': [],
}
if mode in ['structured', 'failure-zero', 'failure-unbound', 'failure-with-candidate']:
    result['status'] = 'failure'
    del result['output']
    result['errors'] = [{'category': 'invalidInput', 'code': 'synthetic-failure', 'message': 'Synthetic failure', 'context': []}]
    if mode == 'failure-unbound':
        del result['invocationIdentity']
        del result['reportedIdentities']
    if mode == 'failure-with-candidate':
        atomic_write(output, b'synthetic generated project')
    atomic_write(Path('result.json'), json.dumps(result).encode())
    sys.exit(0 if mode == 'failure-zero' else 1)

if mode.startswith('identity:'):
    field = mode.split(':', 1)[1]
    result['reportedIdentities'][field] = ['different-capability'] if field == 'capabilityIdentities' else 'different-identity'
if mode == 'invocation-identity':
    result['invocationIdentity'] = 'f' * 64
if mode == 'protocol-version':
    result['protocolVersion'] = 2
if mode == 'wrong-output-identity':
    result['output']['outputIdentity'] = 'different-output'
if mode == 'wrong-output-path':
    result['output']['path'] = 'outputs/other.bin'
if mode == 'wrong-output-media':
    result['output']['mediaType'] = 'application/x-different'
if mode == 'unsafe-output-path':
    result['output']['path'] = '../escape.bin'
if mode == 'wrong-hash':
    result['output']['sha256'] = 'f' * 64
if mode == 'wrong-length':
    result['output']['byteLength'] += 1
if mode == 'declared-over-limit':
    result['output']['byteLength'] = request['output']['maxByteLength'] + 1
if mode in ['diagnostic-bound', 'diagnostic-max']:
    result['diagnostics'] = [{'severity': 'warning', 'code': 'synthetic', 'message': 'Synthetic warning', 'context': []}] * (64 if mode == 'diagnostic-max' else 65)
if mode == 'unknown-field':
    result['unknown'] = True
if mode == 'extra-file':
    Path('undeclared.bin').write_bytes(b'extra')
if mode == 'extra-directory':
    Path('undeclared').mkdir()
if mode == 'leftover':
    output.parent.mkdir(parents=True, exist_ok=True)
    output.with_name(output.name + '.abandoned').write_bytes(b'uncommitted')
if mode == 'candidate-symlink':
    output.parent.mkdir(parents=True, exist_ok=True)
    output.symlink_to(Path(request['settings']['content']['path']).resolve())
elif mode == 'candidate-directory':
    output.mkdir(parents=True)
elif mode == 'actual-over-limit':
    atomic_write(output, b'x' * (request['output']['maxByteLength'] + 1))
elif mode != 'missing-candidate':
    atomic_write(output, b'synthetic generated project')
if mode == 'result-symlink':
    Path('result.json').symlink_to(output)
elif mode == 'result-directory':
    Path('result.json').mkdir()
else:
    serialized = json.dumps(result).encode()
    if mode == 'result-at-bound':
        serialized += b' ' * (262144 - len(serialized))
    atomic_write(Path('result.json'), serialized)
if mode == 'success-one':
    sys.exit(1)
"#;

pub(crate) struct RunnerFixture {
    directory: tempfile::TempDir,
    pub(crate) generator: DeployedGenerator,
    inputs: Option<ConstructedGeneratorInputs>,
    pub(crate) prepared: PreparedGeneratorProcessing,
    options: GeneratorRunnerOptions,
}

fn scratch() -> tempfile::TempDir {
    let parent =
        PathBuf::from(std::env::var_os("TMPDIR").unwrap_or_else(|| "/tmp".into())).join("agents");
    fs::create_dir_all(&parent).unwrap();
    tempfile::tempdir_in(parent).unwrap()
}

impl RunnerFixture {
    pub(crate) async fn new(mode: &str) -> Self {
        let directory = scratch();
        let marker = directory.path().join("started.json");
        let source = FIXTURE
            .replace("__MODE__", &serde_json::to_string(mode).unwrap())
            .replace("__MARKER__", &serde_json::to_string(&marker).unwrap());
        let executable = directory.path().join("generator ; literal $(name)");
        fs::write(&executable, source.as_bytes()).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let inputs = crate::onshape_geometry::generator_runner_test_inputs().await;
        let document = DeployedGeneratorDocument {
            executable_path: executable,
            package_identity: "synthetic-package-v1".to_owned(),
            package_sha256: "a".repeat(64),
            build_identity: "synthetic-build-v1".to_owned(),
            binary_identity: "synthetic-binary-v1".to_owned(),
            binary_sha256: cache_key::hex_sha256(source.as_bytes()),
            protocol_version: 1,
            dialect_identity: "synthetic-dialect-v1".to_owned(),
            capability_identities: vec!["synthetic-capability-v1".to_owned()],
            input_kind_identity: inputs.manifest().export.kind_identity.clone(),
            input_schema_identity: inputs.manifest().export.schema_identity.clone(),
            settings_schema_identity: inputs.settings_schema_identity().to_owned(),
            provenance_set_identity: "synthetic-provenance-v1".to_owned(),
            normalization_identity: "synthetic-normalization-v1".to_owned(),
            validation_identity: "synthetic-validation-v1".to_owned(),
        };
        let generator =
            DeployedGenerator::from_bytes(&serde_json::to_vec(&document).unwrap()).unwrap();
        let compatibility = GeneratorCompatibilityRequest {
            protocol_version: document.protocol_version,
            dialect_identity: document.dialect_identity.clone(),
            capability_identities: document.capability_identities.clone(),
            input_kind_identity: document.input_kind_identity.clone(),
            input_schema_identity: document.input_schema_identity.clone(),
            settings_schema_identity: document.settings_schema_identity.clone(),
        };
        let prepared = prepare_generator_processing(
            &generator,
            &compatibility,
            inputs.manifest(),
            inputs.settings(),
            inputs.expected_placements(),
            &GeneratorProcessingProtocolInputs {
                manifest_path: "inputs/manifest.json".to_owned(),
                settings_content_identity: "synthetic-settings-content-v1".to_owned(),
                settings_path: "inputs/settings.json".to_owned(),
                settings_media_type: "application/json".to_owned(),
                settings_detected_kind_identity: "synthetic-settings-v2".to_owned(),
                output: OutputDeclaration {
                    output_identity: "synthetic-output-v1".to_owned(),
                    role: OutputRole::GeneratedProject,
                    path: "outputs/project.bin".to_owned(),
                    media_type: "application/octet-stream".to_owned(),
                    max_byte_length: 1024,
                },
            },
        )
        .unwrap();
        let scratch_parent = directory.path().join("invocations");
        fs::create_dir(&scratch_parent).unwrap();
        Self {
            directory,
            generator,
            inputs: Some(inputs),
            prepared,
            options: GeneratorRunnerOptions {
                scratch_parent,
                timeout: Duration::from_secs(5),
            },
        }
    }

    pub(crate) async fn run(&mut self) -> Result<VerifiedGeneratorOutput, GeneratorRunnerError> {
        run_generator(
            &self.generator,
            self.inputs.take().unwrap(),
            &self.prepared,
            &self.options,
        )
        .await
    }

    fn started(&self) -> bool {
        self.directory.path().join("started.json").exists()
    }

    pub(crate) fn assert_clean(&self) {
        assert_eq!(
            fs::read_dir(&self.options.scratch_parent).unwrap().count(),
            0
        );
    }
}

#[tokio::test]
async fn success_preserves_all_occurrences_direct_argv_and_private_verified_bytes() {
    let mut fixture = RunnerFixture::new("success").await;
    let invocation = fixture
        .prepared
        .recipe()
        .invocation
        .invocation_identity
        .clone();
    let processing = fixture.prepared.processing_hash().to_owned();
    let settings = fixture
        .inputs
        .as_ref()
        .unwrap()
        .settings_identity()
        .to_owned();
    let paths: Vec<_> = fixture
        .inputs
        .as_ref()
        .unwrap()
        .manifest()
        .objects
        .iter()
        .map(|object| object.retained_content.path.clone())
        .collect();
    assert_eq!(paths.len(), 3);
    assert_ne!(paths[0], paths[2]);
    let verified = fixture.run().await.unwrap();
    assert_eq!(verified.result().status, ResultStatus::Success);
    assert_eq!(verified.invocation_identity(), invocation);
    assert_eq!(verified.processing_hash(), processing);
    assert_eq!(
        verified
            .result()
            .reported_identities
            .as_ref()
            .unwrap()
            .settings_identity,
        settings
    );
    assert!(fixture.started());
    fixture.assert_clean();
    let mut bytes = Vec::new();
    verified.copy_to(&mut bytes).unwrap();
    assert_eq!(bytes, CANDIDATE);
    let mut second_copy = Vec::new();
    verified.copy_to(&mut second_copy).unwrap();
    assert_eq!(second_copy, bytes);
    drop(fixture);
    let mut after_cleanup = Vec::new();
    verified.copy_to(&mut after_cleanup).unwrap();
    assert_eq!(after_cleanup, CANDIDATE);
}

#[tokio::test]
async fn invocation_roots_are_fresh_and_do_not_enter_processing_identity() {
    let mut fixture = RunnerFixture::new("success").await;
    let inputs = crate::onshape_geometry::generator_runner_test_inputs().await;
    let first = fixture.run().await.unwrap();
    let first_marker: Value =
        serde_json::from_slice(&fs::read(fixture.directory.path().join("started.json")).unwrap())
            .unwrap();
    fixture.inputs = Some(inputs);
    let second = fixture.run().await.unwrap();
    let second_marker: Value =
        serde_json::from_slice(&fs::read(fixture.directory.path().join("started.json")).unwrap())
            .unwrap();
    assert_ne!(first_marker["root"], second_marker["root"]);
    assert_eq!(first.processing_hash(), second.processing_hash());
    assert_eq!(first.invocation_identity(), second.invocation_identity());
    fixture.assert_clean();
}

#[tokio::test]
async fn binary_replacement_is_detected_before_the_process_starts() {
    let mut fixture = RunnerFixture::new("success").await;
    fs::write(
        &fixture.generator.document().executable_path,
        b"changed executable bytes",
    )
    .unwrap();
    assert!(matches!(
        fixture.run().await,
        Err(GeneratorRunnerError::DeploymentIntegrity(
            crate::deployed_generator::DeployedGeneratorError::BinaryDigestMismatch { .. }
        ))
    ));
    assert!(!fixture.started());
    fixture.assert_clean();
}

#[tokio::test]
async fn non_executable_and_missing_configured_files_never_spawn() {
    for remove in [false, true] {
        let mut fixture = RunnerFixture::new("success").await;
        let path = &fixture.generator.document().executable_path;
        if remove {
            fs::remove_file(path).unwrap();
        } else {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        assert!(matches!(
            fixture.run().await,
            Err(GeneratorRunnerError::DeploymentIntegrity(_))
        ));
        assert!(!fixture.started());
        fixture.assert_clean();
    }
}

#[tokio::test]
async fn fifo_executable_replacement_is_rejected_without_waiting_for_a_writer() {
    let mut fixture = RunnerFixture::new("success").await;
    let executable = &fixture.generator.document().executable_path;
    fs::remove_file(executable).unwrap();
    assert!(
        std::process::Command::new("mkfifo")
            .arg(executable)
            .status()
            .unwrap()
            .success()
    );
    let result = tokio::time::timeout(Duration::from_secs(2), fixture.run())
        .await
        .unwrap();
    assert!(matches!(
        result,
        Err(GeneratorRunnerError::DeploymentIntegrity(
            crate::deployed_generator::DeployedGeneratorError::ExecutableNotRegularFile { .. }
        ))
    ));
    assert!(!fixture.started());
    fixture.assert_clean();
}

#[tokio::test]
async fn structured_failures_are_distinct_and_never_offer_candidate_bytes() {
    for mode in ["structured", "failure-unbound", "failure-with-candidate"] {
        let mut fixture = RunnerFixture::new(mode).await;
        let error = fixture.run().await.err().unwrap();
        let GeneratorRunnerError::StructuredFailure { result } = error else {
            panic!("wrong outcome for {mode}: {error:?}");
        };
        assert_eq!(result.status, ResultStatus::Failure);
        assert_eq!(result.errors[0].code, "synthetic-failure");
        assert!(result.output.is_none());
        fixture.assert_clean();
    }
}

#[tokio::test]
async fn crash_and_exit_status_disagreements_are_distinct_failures() {
    let mut crash = RunnerFixture::new("crash").await;
    assert!(matches!(
        crash.run().await,
        Err(GeneratorRunnerError::Crash { signal: 15 })
    ));
    crash.assert_clean();
    for (mode, code) in [
        ("unexpected-exit", 7),
        ("failure-zero", 0),
        ("success-one", 1),
    ] {
        let mut fixture = RunnerFixture::new(mode).await;
        assert!(matches!(
            fixture.run().await,
            Err(GeneratorRunnerError::UnexpectedExit { code: actual }) if actual == code
        ));
        fixture.assert_clean();
    }
}

#[tokio::test]
async fn timeout_kills_and_cleans_the_invocation() {
    let mut fixture = RunnerFixture::new("timeout").await;
    fixture.options.timeout = Duration::from_secs(1);
    assert!(matches!(
        fixture.run().await,
        Err(GeneratorRunnerError::Timeout)
    ));
    assert!(fixture.started());
    assert_process_gone(fixture.directory.path());
    fixture.assert_clean();
}

fn assert_process_gone(directory: &Path) {
    let marker: Value =
        serde_json::from_slice(&fs::read(directory.join("started.json")).unwrap()).unwrap();
    let pid = marker["pid"].as_u64().unwrap();
    assert!(!PathBuf::from(format!("/proc/{pid}")).exists());
}

#[tokio::test]
async fn cancellation_reaps_the_process_and_eventually_cleans_staging() {
    let fixture = RunnerFixture::new("timeout").await;
    let RunnerFixture {
        directory,
        generator,
        inputs,
        prepared,
        options,
    } = fixture;
    let scratch_parent = options.scratch_parent.clone();
    let task = tokio::spawn(async move {
        run_generator(&generator, inputs.unwrap(), &prepared, &options).await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while !fs::read(directory.path().join("started.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .is_some_and(|marker| marker["pid"].is_u64())
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    task.abort();
    assert!(task.await.err().unwrap().is_cancelled());
    tokio::time::timeout(Duration::from_secs(5), async {
        while fs::read_dir(&scratch_parent).unwrap().next().is_some() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_process_gone(directory.path());
}

#[tokio::test]
async fn missing_malformed_and_bounded_results_fail_closed() {
    for mode in ["missing-result", "candidate-without-result"] {
        let mut missing = RunnerFixture::new(mode).await;
        assert!(matches!(
            missing.run().await,
            Err(GeneratorRunnerError::MissingResult)
        ));
        missing.assert_clean();
    }
    for mode in [
        "malformed",
        "oversize-result",
        "diagnostic-bound",
        "unknown-field",
        "unsafe-output-path",
    ] {
        let mut fixture = RunnerFixture::new(mode).await;
        assert!(
            matches!(
                fixture.run().await,
                Err(GeneratorRunnerError::MalformedResult)
            ),
            "mode {mode}"
        );
        fixture.assert_clean();
    }
}

#[tokio::test]
async fn every_reported_static_and_invocation_specific_identity_is_compared() {
    for field in [
        "packageIdentity",
        "buildIdentity",
        "binaryIdentity",
        "dialectIdentity",
        "capabilityIdentities",
        "inputKindIdentity",
        "inputSchemaIdentity",
        "settingsIdentity",
        "settingsSchemaIdentity",
        "provenanceSetIdentity",
        "normalizationIdentity",
        "validationIdentity",
    ] {
        let mut fixture = RunnerFixture::new(&format!("identity:{field}")).await;
        assert!(
            matches!(
                fixture.run().await,
                Err(GeneratorRunnerError::ResultIdentityMismatch)
            ),
            "field {field}"
        );
        fixture.assert_clean();
    }
    for mode in ["invocation-identity", "protocol-version"] {
        let mut invocation = RunnerFixture::new(mode).await;
        assert!(matches!(
            invocation.run().await,
            Err(GeneratorRunnerError::ResultIdentityMismatch)
        ));
        invocation.assert_clean();
    }
}

#[tokio::test]
async fn changed_output_declarations_and_declared_size_limit_are_rejected() {
    for mode in [
        "wrong-output-identity",
        "wrong-output-path",
        "wrong-output-media",
        "declared-over-limit",
    ] {
        let mut fixture = RunnerFixture::new(mode).await;
        assert!(
            matches!(
                fixture.run().await,
                Err(GeneratorRunnerError::OutputDeclarationMismatch)
            ),
            "mode {mode}"
        );
        fixture.assert_clean();
    }
}

#[tokio::test]
async fn candidate_existence_and_measured_hash_and_length_are_required() {
    let mut missing = RunnerFixture::new("missing-candidate").await;
    assert!(matches!(
        missing.run().await,
        Err(GeneratorRunnerError::CandidateMissing)
    ));
    missing.assert_clean();
    for mode in ["wrong-hash", "wrong-length", "actual-over-limit"] {
        let mut fixture = RunnerFixture::new(mode).await;
        assert!(
            matches!(
                fixture.run().await,
                Err(GeneratorRunnerError::CandidateMismatch)
            ),
            "mode {mode}"
        );
        fixture.assert_clean();
    }
}

#[tokio::test]
async fn undeclared_files_directories_leftovers_and_nonregular_finals_are_rejected() {
    for mode in [
        "extra-file",
        "extra-directory",
        "leftover",
        "candidate-symlink",
        "candidate-directory",
        "result-symlink",
        "result-directory",
    ] {
        let mut fixture = RunnerFixture::new(mode).await;
        assert!(
            matches!(
                fixture.run().await,
                Err(GeneratorRunnerError::UnexpectedFiles)
                    | Err(GeneratorRunnerError::CandidateMissing)
                    | Err(GeneratorRunnerError::MalformedResult)
            ),
            "mode {mode}"
        );
        fixture.assert_clean();
    }
}

#[tokio::test]
async fn staging_failure_does_not_start_the_generator() {
    let mut fixture = RunnerFixture::new("success").await;
    fs::remove_dir(&fixture.options.scratch_parent).unwrap();
    fs::write(&fixture.options.scratch_parent, b"not a directory").unwrap();
    assert!(matches!(
        fixture.run().await,
        Err(GeneratorRunnerError::Staging) | Err(GeneratorRunnerError::Io)
    ));
    assert!(!fixture.started());
}

fn protocol_inputs(prepared: &PreparedGeneratorProcessing) -> GeneratorProcessingProtocolInputs {
    let request = &prepared.recipe().invocation;
    GeneratorProcessingProtocolInputs {
        manifest_path: request.input_manifest.path.clone(),
        settings_content_identity: request.settings.content.content_identity.clone(),
        settings_path: request.settings.content.path.clone(),
        settings_media_type: request.settings.content.media_type.clone(),
        settings_detected_kind_identity: request.settings.content.detected_kind_identity.clone(),
        output: request.output.clone(),
    }
}

fn compatibility(generator: &DeployedGenerator) -> GeneratorCompatibilityRequest {
    let document = generator.document();
    GeneratorCompatibilityRequest {
        protocol_version: document.protocol_version,
        dialect_identity: document.dialect_identity.clone(),
        capability_identities: document.capability_identities.clone(),
        input_kind_identity: document.input_kind_identity.clone(),
        input_schema_identity: document.input_schema_identity.clone(),
        settings_schema_identity: document.settings_schema_identity.clone(),
    }
}

#[tokio::test]
async fn unsupported_preparation_never_dispatches_a_fallback() {
    let mut fixture = RunnerFixture::new("success").await;
    let inputs = fixture.inputs.as_ref().unwrap();
    let mut request = compatibility(&fixture.generator);
    request.dialect_identity = "unsupported-dialect".to_owned();
    fixture.prepared = prepare_generator_processing(
        &fixture.generator,
        &request,
        inputs.manifest(),
        inputs.settings(),
        inputs.expected_placements(),
        &protocol_inputs(&fixture.prepared),
    )
    .unwrap();
    assert!(matches!(
        fixture.run().await,
        Err(GeneratorRunnerError::UnsupportedCombination)
    ));
    assert!(!fixture.started());
    fixture.assert_clean();
}

#[tokio::test]
async fn valid_preparation_for_different_settings_does_not_replace_the_trusted_bundle() {
    let mut fixture = RunnerFixture::new("success").await;
    let inputs = fixture.inputs.as_ref().unwrap();
    let mut settings = inputs.settings().clone();
    let mut expected = inputs.expected_placements().to_vec();
    settings.placements[0].matrix[3] += 1.0;
    expected[0].expected_neutral_placement_matrix[3] += 1.0;
    fixture.prepared = prepare_generator_processing(
        &fixture.generator,
        &compatibility(&fixture.generator),
        inputs.manifest(),
        &settings,
        &expected,
        &protocol_inputs(&fixture.prepared),
    )
    .unwrap();
    assert!(matches!(
        fixture.run().await,
        Err(GeneratorRunnerError::InvalidInvocation)
    ));
    assert!(!fixture.started());
    fixture.assert_clean();
}

#[tokio::test]
async fn spawn_failure_is_distinct_after_successful_binary_verification() {
    let mut fixture = RunnerFixture::new("success").await;
    let bytes = b"#!/nonexistent/synthetic-interpreter\n";
    let mut document = fixture.generator.document().clone();
    fs::write(&document.executable_path, bytes).unwrap();
    document.binary_sha256 = cache_key::hex_sha256(bytes);
    fixture.generator =
        DeployedGenerator::from_bytes(&serde_json::to_vec(&document).unwrap()).unwrap();
    // Bind the updated immutable binary digest before testing spawn itself.
    let inputs = fixture.inputs.as_ref().unwrap();
    fixture.prepared = prepare_generator_processing(
        &fixture.generator,
        &compatibility(&fixture.generator),
        inputs.manifest(),
        inputs.settings(),
        inputs.expected_placements(),
        &protocol_inputs(&fixture.prepared),
    )
    .unwrap();
    assert!(matches!(
        fixture.run().await,
        Err(GeneratorRunnerError::Spawn)
    ));
    assert!(!fixture.started());
    fixture.assert_clean();
}

#[tokio::test]
async fn result_and_diagnostic_bounds_accept_the_exact_maximum() {
    for mode in ["result-at-bound", "diagnostic-max"] {
        let mut fixture = RunnerFixture::new(mode).await;
        let verified = fixture.run().await.unwrap();
        let mut bytes = Vec::new();
        verified.copy_to(&mut bytes).unwrap();
        assert_eq!(bytes, CANDIDATE);
        if mode == "diagnostic-max" {
            assert_eq!(verified.result().diagnostics.len(), 64);
        }
        fixture.assert_clean();
    }
}

#[tokio::test]
async fn cleanup_failure_suppresses_success_and_preserves_an_earlier_failure() {
    for had_prior in [false, true] {
        let mut fixture = RunnerFixture::new("success").await;
        let verified = fixture.run().await.unwrap();
        fixture.assert_clean();
        let root = tempfile::tempdir_in(&fixture.options.scratch_parent).unwrap();
        let path = root.path().to_owned();
        // Replacing an empty owned directory with a regular file makes explicit
        // directory cleanup fail even when tests run as the root user.
        fs::remove_dir(&path).unwrap();
        fs::write(&path, b"not a directory").unwrap();
        let outcome = if had_prior {
            Err(GeneratorRunnerError::MalformedResult)
        } else {
            Ok(verified)
        };
        let error = finish_cleanup(root, outcome).await.err().unwrap();
        let GeneratorRunnerError::Cleanup { prior, .. } = error else {
            panic!("wrong cleanup outcome: {error:?}");
        };
        assert_eq!(prior.is_some(), had_prior);
        if let Some(prior) = prior {
            assert!(matches!(*prior, GeneratorRunnerError::MalformedResult));
        }
        fs::remove_file(path).unwrap();
        fixture.assert_clean();
    }
}

#[tokio::test]
async fn unconfirmed_process_cleanup_preserves_the_invocation_root() {
    let directory = scratch();
    let root = tempfile::tempdir_in(directory.path()).unwrap();
    let path = root.path().to_owned();
    fs::write(path.join("still-owned-by-child"), b"synthetic").unwrap();
    let outcome = finish_cleanup(
        root,
        Err(GeneratorRunnerError::ProcessCleanup {
            invocation_root: path.clone(),
        }),
    )
    .await;
    assert!(
        matches!(outcome, Err(GeneratorRunnerError::ProcessCleanup { invocation_root }) if invocation_root == path)
    );
    assert!(path.join("still-owned-by-child").exists());
    fs::remove_dir_all(path).unwrap();
}
