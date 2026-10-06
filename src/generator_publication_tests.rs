//! Source-neutral publication conformance through the real trusted runner and
//! an in-memory loopback S3 service. No target archives or live services.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    response::Response,
};

use super::*;
use crate::{
    cache_key,
    config::StorageConfig,
    db::{GeneratorArtifactFileInsert, GeneratorArtifactLookup, GeneratorArtifactStage},
    generator_processing::generator_artifact_set_hash,
    generator_runner::tests::RunnerFixture,
};

const CANDIDATE: &[u8] = b"synthetic generated project";

#[derive(Clone, Copy, Default, Debug)]
enum Fault {
    #[default]
    None,
    Put,
    Head,
    Get,
    Length,
    Hash,
    ContentType,
}

#[derive(Clone)]
struct StoredObject {
    bytes: Vec<u8>,
    content_type: String,
}

#[derive(Clone)]
struct CapturedRequest {
    method: String,
    path: String,
    headers: HeaderMap,
    bytes: Vec<u8>,
    artifact_status: Option<String>,
}

#[derive(Default)]
struct S3State {
    objects: HashMap<String, StoredObject>,
    requests: Vec<CapturedRequest>,
    fault: Fault,
    observed_artifact: Option<(Database, String)>,
    pause_put: Option<Arc<tokio::sync::Notify>>,
}

struct PublicationFixture {
    _directory: tempfile::TempDir,
    db: Database,
    storage: StorageClient,
    server: tokio::task::JoinHandle<()>,
    state: Arc<Mutex<S3State>>,
    runner: RunnerFixture,
    output: Arc<VerifiedGeneratorOutput>,
    target: GeneratorPublicationTarget,
}

impl Drop for PublicationFixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl PublicationFixture {
    async fn new() -> Self {
        let parent =
            std::path::PathBuf::from(std::env::var_os("TMPDIR").unwrap_or_else(|| "/tmp".into()))
                .join("agents");
        std::fs::create_dir_all(&parent).unwrap();
        let directory = tempfile::tempdir_in(parent).unwrap();
        let db = Database::connect(&format!(
            "sqlite://{}?mode=rwc",
            directory.path().join("publication.db").display()
        ))
        .await
        .unwrap();
        let state = Arc::new(Mutex::new(S3State::default()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new().fallback(s3_request).with_state(state.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let storage = StorageClient::new(StorageConfig {
            bucket: "synthetic-bucket".to_owned(),
            endpoint_url: Some(endpoint),
            region: "auto".to_owned(),
            access_key_id: Some("synthetic-access".to_owned()),
            secret_access_key: Some("synthetic-secret".to_owned()),
            public_base_url: None,
            force_path_style: true,
        })
        .await
        .unwrap();
        let mut runner = RunnerFixture::new("success").await;
        let output = Arc::new(runner.run().await.unwrap());
        let target = GeneratorPublicationTarget {
            model_slug: "synthetic-model".to_owned(),
            output_kind: format!(
                "slicer_project:{}",
                runner.prepared.recipe().compatibility.dialect_identity
            ),
            format: "project_3mf".to_owned(),
            logical_path: "synthetic-project.bin".to_owned(),
            producing_job_key: None,
            parameter_schema_version: 2,
            config_values_json: "{}".to_owned(),
        };
        state.lock().unwrap().observed_artifact = Some((
            db.clone(),
            generator_artifact_set_hash(&runner.prepared, &target.output_kind, &target.format)
                .unwrap(),
        ));
        Self {
            _directory: directory,
            db,
            storage,
            server,
            state,
            runner,
            output,
            target,
        }
    }

    async fn publish(&self) -> anyhow::Result<String> {
        publish_generator_output(
            &self.db,
            &self.storage,
            &self.runner.generator,
            &self.runner.prepared,
            self.output.clone(),
            &self.target,
        )
        .await
    }

    async fn reconcile(&self) -> anyhow::Result<Option<String>> {
        reconcile_generator_artifact(
            &self.db,
            &self.storage,
            &self.runner.generator,
            &self.runner.prepared,
            &self.target,
        )
        .await
    }

    fn artifact_hash(&self) -> String {
        generator_artifact_set_hash(
            &self.runner.prepared,
            &self.target.output_kind,
            &self.target.format,
        )
        .unwrap()
    }

    fn fault(&self, fault: Fault) {
        self.state.lock().unwrap().fault = fault;
    }

    fn methods(&self) -> Vec<String> {
        self.state
            .lock()
            .unwrap()
            .requests
            .iter()
            .map(|request| request.method.clone())
            .collect()
    }

    fn reset_requests(&self) {
        self.state.lock().unwrap().requests.clear();
    }

    async fn assert_nonready(&self) {
        if let Some(record) = self.db.artifact_set(&self.artifact_hash()).await.unwrap() {
            assert_ne!(record.status, "ready");
        }
        assert!(
            self.db
                .latest_ready_artifact_for_generator_recipe(GeneratorArtifactLookup {
                    prepared: &self.runner.prepared,
                    output_kind: &self.target.output_kind,
                    format: &self.target.format,
                    primary_role: "generated_project",
                    primary_logical_path: &self.target.logical_path,
                    primary_content_type: "application/octet-stream",
                })
                .await
                .unwrap()
                .is_none()
        );
    }

    async fn stage(&self) -> String {
        let key = generator_publication_object_key(&self.runner.prepared, &self.target).unwrap();
        let result = self.output.result().output.as_ref().unwrap();
        let metadata_json = serde_json::to_string(self.output.result()).unwrap();
        self.db
            .stage_generator_artifact(
                &self.runner.prepared,
                GeneratorArtifactStage {
                    model_slug: &self.target.model_slug,
                    output_kind: &self.target.output_kind,
                    format: &self.target.format,
                    object_key: &key,
                    content_type: &result.media_type,
                    byte_len: result.byte_length.try_into().unwrap(),
                    sha256: &result.sha256,
                    producing_job_key: self.target.producing_job_key.as_deref(),
                    parameter_schema_version: self.target.parameter_schema_version,
                    config_values_json: &self.target.config_values_json,
                },
                &[GeneratorArtifactFileInsert {
                    role: "generated_project",
                    logical_path: &self.target.logical_path,
                    original_path: None,
                    object_key: &key,
                    content_type: &result.media_type,
                    byte_len: result.byte_length.try_into().unwrap(),
                    sha256: &result.sha256,
                    metadata_json: &metadata_json,
                }],
            )
            .await
            .unwrap();
        key
    }

    fn stored(&self, key: &str) -> Vec<u8> {
        self.state.lock().unwrap().objects[&format!("/synthetic-bucket/{key}")]
            .bytes
            .clone()
    }
}

async fn s3_request(State(state): State<Arc<Mutex<S3State>>>, request: Request) -> Response {
    let method = request.method().as_str().to_owned();
    let path = request.uri().path().to_owned();
    let headers = request.headers().clone();
    let mut bytes = to_bytes(request.into_body(), 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    if headers
        .get("content-encoding")
        .is_some_and(|value| value.to_str().unwrap().contains("aws-chunked"))
    {
        bytes = decode_aws_chunks(&bytes);
    }
    let observed = state.lock().unwrap().observed_artifact.clone();
    let artifact_status = if let Some((db, hash)) = observed {
        db.artifact_set(&hash)
            .await
            .unwrap()
            .map(|record| record.status)
    } else {
        None
    };
    if method == "PUT" {
        let pause = {
            let mut state = state.lock().unwrap();
            state.requests.push(CapturedRequest {
                method: method.clone(),
                path: path.clone(),
                headers: headers.clone(),
                bytes: bytes.clone(),
                artifact_status,
            });
            if matches!(state.fault, Fault::Put) {
                return Response::builder()
                    .status(StatusCode::FORBIDDEN)
                    .body(Body::empty())
                    .unwrap();
            }
            state.objects.insert(
                path,
                StoredObject {
                    bytes,
                    content_type: headers["content-type"].to_str().unwrap().to_owned(),
                },
            );
            state.pause_put.clone()
        };
        if let Some(pause) = pause {
            pause.notified().await;
        }
        return Response::builder()
            .status(StatusCode::OK)
            .body(Body::empty())
            .unwrap();
    }
    let mut state = state.lock().unwrap();
    state.requests.push(CapturedRequest {
        method: method.clone(),
        path: path.clone(),
        headers: headers.clone(),
        bytes: bytes.clone(),
        artifact_status,
    });
    let fault = state.fault;
    if matches!(
        (method.as_str(), fault),
        ("HEAD", Fault::Head) | ("GET", Fault::Get)
    ) {
        return Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(Body::empty())
            .unwrap();
    }
    let Some(object) = state.objects.get(&path) else {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap();
    };
    let mut body = object.bytes.clone();
    if matches!(fault, Fault::Hash) && method == "GET" {
        body[0] ^= 1;
    }
    let length = body.len() + usize::from(matches!(fault, Fault::Length));
    let content_type = if matches!(fault, Fault::ContentType) {
        "application/x-wrong"
    } else {
        &object.content_type
    };
    Response::builder()
        .status(StatusCode::OK)
        .header("content-length", length.to_string())
        .header("content-type", content_type)
        .body(if method == "HEAD" {
            Body::empty()
        } else {
            Body::from(body)
        })
        .unwrap()
}

fn decode_aws_chunks(bytes: &[u8]) -> Vec<u8> {
    let mut remaining = bytes;
    let mut decoded = Vec::new();
    loop {
        let end = remaining
            .windows(2)
            .position(|part| part == b"\r\n")
            .unwrap();
        let size = usize::from_str_radix(
            std::str::from_utf8(&remaining[..end])
                .unwrap()
                .split(';')
                .next()
                .unwrap(),
            16,
        )
        .unwrap();
        if size == 0 {
            break;
        }
        remaining = &remaining[end + 2..];
        decoded.extend_from_slice(&remaining[..size]);
        remaining = &remaining[size + 2..];
    }
    decoded
}

#[tokio::test]
async fn successful_generator_publication_uploads_exact_bytes_then_verifies_and_marks_ready() {
    let fixture = PublicationFixture::new().await;
    fixture.runner.assert_clean();
    let key = fixture.publish().await.unwrap();
    assert_eq!(fixture.stored(&key), CANDIDATE);
    assert_eq!(fixture.methods(), ["PUT", "HEAD", "GET"]);
    {
        let state = fixture.state.lock().unwrap();
        let upload = &state.requests[0];
        assert!(
            state
                .requests
                .iter()
                .all(|request| request.artifact_status.as_deref() == Some("staged"))
        );
        assert_eq!(upload.bytes, CANDIDATE);
        assert_eq!(upload.path, format!("/synthetic-bucket/{key}"));
        assert_eq!(upload.headers["content-type"], "application/octet-stream");
        assert_eq!(
            upload.headers["cache-control"],
            "public, max-age=31536000, immutable"
        );
        assert!(
            upload.headers["content-disposition"]
                .to_str()
                .unwrap()
                .contains(&fixture.target.logical_path)
        );
    }
    let record = fixture
        .db
        .artifact_set(&fixture.artifact_hash())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.status, "ready");
    assert_eq!(
        record.generator_processing_hash.as_deref(),
        Some(fixture.runner.prepared.processing_hash())
    );
    assert_eq!(record.primary_object_key.as_deref(), Some(key.as_str()));
}

#[tokio::test]
async fn processing_mismatch_does_not_stage_or_touch_storage() {
    let fixture = PublicationFixture::new().await;
    let other = RunnerFixture::new("diagnostic-max").await;
    assert_ne!(
        other.prepared.processing_hash(),
        fixture.runner.prepared.processing_hash()
    );
    assert!(
        publish_generator_output(
            &fixture.db,
            &fixture.storage,
            &other.generator,
            &other.prepared,
            fixture.output.clone(),
            &fixture.target
        )
        .await
        .is_err()
    );
    assert!(fixture.methods().is_empty());
    assert!(
        fixture
            .db
            .artifact_set(&fixture.artifact_hash())
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn publication_target_cannot_expose_the_invocation_path_or_unsafe_download_names() {
    for path in [
        "../escape.bin",
        "/absolute.bin",
        "nested/../escape.bin",
        "bad\r\nheader.bin",
        "bad\"header.bin",
    ] {
        let mut fixture = PublicationFixture::new().await;
        fixture.target.logical_path = path.to_owned();
        assert!(fixture.publish().await.is_err(), "{path:?}");
        assert!(fixture.methods().is_empty());
        assert!(
            fixture
                .db
                .artifact_set(&fixture.artifact_hash())
                .await
                .unwrap()
                .is_none()
        );
    }
}

#[tokio::test]
async fn retained_bytes_are_remeasured_before_any_staging_or_upload() {
    for bytes in [
        b"wrong length".as_slice(),
        b"Synthetic generated project".as_slice(),
    ] {
        let mut fixture = PublicationFixture::new().await;
        Arc::get_mut(&mut fixture.output)
            .unwrap()
            .replace_test_bytes(bytes, false);
        assert!(fixture.publish().await.is_err());
        assert!(fixture.methods().is_empty());
        assert!(
            fixture
                .db
                .artifact_set(&fixture.artifact_hash())
                .await
                .unwrap()
                .is_none()
        );
    }
}

#[tokio::test]
async fn different_candidate_bytes_cannot_replace_an_existing_recipe_artifact() {
    let mut fixture = PublicationFixture::new().await;
    let key = fixture.publish().await.unwrap();
    let original = fixture
        .db
        .artifact_set(&fixture.artifact_hash())
        .await
        .unwrap()
        .unwrap();
    Arc::get_mut(&mut fixture.output)
        .unwrap()
        .replace_test_bytes(b"different synthetic project", true);
    fixture.reset_requests();
    assert!(fixture.publish().await.is_err());
    assert!(fixture.methods().is_empty());
    assert_eq!(fixture.stored(&key), CANDIDATE);
    let after = fixture
        .db
        .artifact_set(&fixture.artifact_hash())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.status, "ready");
    assert_eq!(after.created_at, original.created_at);
    assert_eq!(
        cache_key::hex_sha256(&fixture.stored(&key)),
        cache_key::hex_sha256(CANDIDATE)
    );
}

#[tokio::test]
async fn every_storage_failure_stays_nonready_and_retry_uses_exact_immutable_evidence() {
    for fault in [
        Fault::Put,
        Fault::Head,
        Fault::Get,
        Fault::Length,
        Fault::Hash,
        Fault::ContentType,
    ] {
        let fixture = PublicationFixture::new().await;
        fixture.fault(fault);
        assert!(fixture.publish().await.is_err(), "{fault:?}");
        fixture.assert_nonready().await;
        assert_ne!(
            fixture
                .db
                .artifact_set(&fixture.artifact_hash())
                .await
                .unwrap()
                .unwrap()
                .status,
            "ready"
        );
        fixture.fault(Fault::None);
        fixture.reset_requests();
        let key = fixture.publish().await.unwrap();
        assert_eq!(fixture.stored(&key), CANDIDATE);
        assert_eq!(
            fixture
                .db
                .artifact_set(&fixture.artifact_hash())
                .await
                .unwrap()
                .unwrap()
                .status,
            "ready"
        );
    }
}

#[tokio::test]
async fn ready_retry_and_reconciliation_read_back_without_rewriting_ready_bytes() {
    let fixture = PublicationFixture::new().await;
    let key = fixture.publish().await.unwrap();
    fixture.reset_requests();
    assert_eq!(fixture.publish().await.unwrap(), key);
    assert_eq!(fixture.reconcile().await.unwrap(), Some(key.clone()));
    assert!(!fixture.methods().iter().any(|method| method == "PUT"));
    assert_eq!(fixture.stored(&key), CANDIDATE);
}

#[tokio::test]
async fn failed_ready_reverification_withdraws_advertisement_until_exact_repair() {
    for reconcile in [false, true] {
        for fault in [Fault::Head, Fault::Hash] {
            let fixture = PublicationFixture::new().await;
            let key = fixture.publish().await.unwrap();
            fixture.fault(fault);
            fixture.reset_requests();
            if reconcile {
                assert!(fixture.reconcile().await.is_err());
            } else {
                assert!(fixture.publish().await.is_err());
            }
            assert!(!fixture.methods().iter().any(|method| method == "PUT"));
            fixture.assert_nonready().await;
            assert_eq!(
                fixture
                    .db
                    .artifact_set(&fixture.artifact_hash())
                    .await
                    .unwrap()
                    .unwrap()
                    .status,
                "upload_failed"
            );
            assert_eq!(fixture.stored(&key), CANDIDATE);
            fixture.fault(Fault::None);
            fixture.reset_requests();
            assert_eq!(fixture.publish().await.unwrap(), key);
            assert_eq!(fixture.methods(), ["PUT", "HEAD", "GET"]);
            assert_eq!(
                fixture
                    .db
                    .artifact_set(&fixture.artifact_hash())
                    .await
                    .unwrap()
                    .unwrap()
                    .status,
                "ready"
            );
        }
    }
}

#[tokio::test]
async fn concurrent_exact_publications_converge_on_one_ready_immutable_artifact() {
    let fixture = PublicationFixture::new().await;
    let (first, second) = tokio::join!(fixture.publish(), fixture.publish());
    assert_eq!(first.unwrap(), second.unwrap());
    let key = generator_publication_object_key(&fixture.runner.prepared, &fixture.target).unwrap();
    assert_eq!(fixture.stored(&key), CANDIDATE);
    assert_eq!(
        fixture
            .db
            .artifact_set(&fixture.artifact_hash())
            .await
            .unwrap()
            .unwrap()
            .status,
        "ready"
    );
    fixture.runner.assert_clean();
}

#[tokio::test]
async fn reconciliation_requires_completed_verified_storage_and_never_reruns_inputs() {
    let fixture = PublicationFixture::new().await;
    assert_eq!(fixture.reconcile().await.unwrap(), None);
    assert!(fixture.methods().is_empty());
    let key = fixture.stage().await;
    assert!(fixture.reconcile().await.is_err());
    fixture.assert_nonready().await;
    fixture.state.lock().unwrap().objects.insert(
        format!("/synthetic-bucket/{key}"),
        StoredObject {
            bytes: CANDIDATE.to_vec(),
            content_type: "application/octet-stream".to_owned(),
        },
    );
    fixture.reset_requests();
    assert_eq!(fixture.reconcile().await.unwrap(), Some(key));
    assert_eq!(fixture.methods(), ["HEAD", "GET"]);
    fixture.runner.assert_clean();
    assert_eq!(
        fixture
            .db
            .artifact_set(&fixture.artifact_hash())
            .await
            .unwrap()
            .unwrap()
            .status,
        "ready"
    );
}

#[tokio::test]
async fn cancelled_upload_stays_staged_until_reconciliation_verifies_completed_storage() {
    let fixture = PublicationFixture::new().await;
    let release = Arc::new(tokio::sync::Notify::new());
    fixture.state.lock().unwrap().pause_put = Some(release.clone());
    {
        let publication = fixture.publish();
        tokio::pin!(publication);
        let uploaded = async {
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                loop {
                    if fixture.methods().iter().any(|method| method == "PUT") {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap();
        };
        tokio::select! {
            result = &mut publication => panic!("publication completed before held upload response: {result:?}"),
            () = uploaded => {},
        }
    }
    fixture.assert_nonready().await;
    assert_eq!(
        fixture
            .db
            .artifact_set(&fixture.artifact_hash())
            .await
            .unwrap()
            .unwrap()
            .status,
        "staged"
    );
    release.notify_one();
    fixture.state.lock().unwrap().pause_put = None;
    fixture.reset_requests();
    let key = fixture.reconcile().await.unwrap().unwrap();
    assert_eq!(fixture.stored(&key), CANDIDATE);
    assert_eq!(fixture.methods(), ["HEAD", "GET"]);
    assert_eq!(
        fixture
            .db
            .artifact_set(&fixture.artifact_hash())
            .await
            .unwrap()
            .unwrap()
            .status,
        "ready"
    );
}

#[tokio::test]
async fn superseded_generator_artifacts_cannot_be_republished_or_reconciled() {
    let fixture = PublicationFixture::new().await;
    let key = fixture.publish().await.unwrap();
    fixture
        .db
        .supersede_artifact_set(&fixture.artifact_hash(), None, Some("synthetic withdrawal"))
        .await
        .unwrap();
    fixture.reset_requests();
    assert!(fixture.publish().await.is_err());
    assert!(fixture.reconcile().await.is_err());
    assert!(fixture.methods().is_empty());
    assert_eq!(fixture.stored(&key), CANDIDATE);
    assert_eq!(
        fixture
            .db
            .artifact_set(&fixture.artifact_hash())
            .await
            .unwrap()
            .unwrap()
            .status,
        "superseded"
    );
}

#[tokio::test]
async fn changed_recipe_supersedes_only_after_the_new_upload_is_verified() {
    let fixture = PublicationFixture::new().await;
    let old_key = fixture.publish().await.unwrap();
    let mut replacement = RunnerFixture::new("diagnostic-max").await;
    let replacement_output = Arc::new(replacement.run().await.unwrap());
    fixture.fault(Fault::Hash);
    assert!(
        publish_generator_output(
            &fixture.db,
            &fixture.storage,
            &replacement.generator,
            &replacement.prepared,
            replacement_output.clone(),
            &fixture.target
        )
        .await
        .is_err()
    );
    assert_eq!(
        fixture
            .db
            .artifact_set(&fixture.artifact_hash())
            .await
            .unwrap()
            .unwrap()
            .status,
        "ready"
    );
    fixture.fault(Fault::None);
    let new_key = publish_generator_output(
        &fixture.db,
        &fixture.storage,
        &replacement.generator,
        &replacement.prepared,
        replacement_output.clone(),
        &fixture.target,
    )
    .await
    .unwrap();
    assert_ne!(old_key, new_key);
    assert_eq!(fixture.stored(&old_key), CANDIDATE);
    let old = fixture
        .db
        .artifact_set(&fixture.artifact_hash())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(old.status, "superseded");
    assert_eq!(
        old.superseded_by,
        Some(
            generator_artifact_set_hash(
                &replacement.prepared,
                &fixture.target.output_kind,
                &fixture.target.format
            )
            .unwrap()
        )
    );
}
