use super::*;

use sqlx::{Row, SqlitePool};

async fn database() -> (tempfile::TempDir, crate::db::Database, SqlitePool) {
    let parent =
        std::path::PathBuf::from(std::env::var_os("TMPDIR").unwrap_or_else(|| "/tmp".into()))
            .join("agents");
    std::fs::create_dir_all(&parent).unwrap();
    let directory = tempfile::tempdir_in(parent).unwrap();
    let url = format!(
        "sqlite://{}?mode=rwc",
        directory.path().join("project.db").display()
    );
    let db = crate::db::Database::connect(&url).await.unwrap();
    let pool = SqlitePool::connect(&url).await.unwrap();
    (directory, db, pool)
}

fn request() -> ProjectJob {
    let mut request = ProjectJob {
        model_slug: "synthetic-model".to_owned(),
        source_hash: "a".repeat(64),
        config_hash: "b".repeat(64),
        values: HashMap::from([("width".to_owned(), "12".to_owned())]),
        selectors: vec![
            SelectionSelector::Part {
                part_id: "synthetic-a".to_owned(),
            },
            SelectionSelector::Part {
                part_id: "synthetic-b".to_owned(),
            },
        ],
        generator_static_identity: "c".repeat(64),
        policy_identity: "d".repeat(64),
        catalog_identity: "e".repeat(64),
        request_identity: String::new(),
    };
    request.request_identity = request.identity().unwrap();
    request
}

#[test]
fn project_identity_binds_selection_order_deployment_policy_and_configuration() {
    let request = request();
    for change in 0..5 {
        let mut changed = request.clone();
        match change {
            0 => changed.selectors.reverse(),
            1 => changed.generator_static_identity = "e".repeat(64),
            2 => changed.policy_identity = "e".repeat(64),
            3 => changed.config_hash = "e".repeat(64),
            _ => {
                changed.values.insert("width".to_owned(), "13".to_owned());
            }
        }
        assert_ne!(request.request_identity, changed.identity().unwrap());
    }
    let json = request.canonical_json().unwrap();
    assert_eq!(ProjectJob::from_record(&json).unwrap(), request);
    let mut edited: serde_json::Value = serde_json::from_str(&json).unwrap();
    edited["values"]["width"] = "99".into();
    assert!(
        ProjectJob::from_record(
            &String::from_utf8(cache_key::canonical_json_bytes(&edited).unwrap()).unwrap()
        )
        .is_err()
    );
}

#[test]
fn operational_failures_remain_distinct_and_hide_upstream_text() {
    for kind in [
        FailureKind::AuthenticationFailure,
        FailureKind::TransportFailure,
        FailureKind::OperationalApiContractFailure,
        FailureKind::OperationalTimeoutFailure,
        FailureKind::OperationalTranslationFailure,
        FailureKind::OperationalHttpFailure,
    ] {
        let error = api_failure(ApiFailure::new(kind, "private_document_sensitive_message"));
        let safe = error.downcast_ref::<ProjectFailure>().unwrap();
        assert_eq!(safe.category, "operational_failure");
        assert!(!error.to_string().contains("private"));
        assert_ne!(safe.category, "unavailable_source_state");
    }
    let unavailable = api_failure(ApiFailure::new(
        FailureKind::UnavailableSourceState,
        "private_part",
    ));
    assert_eq!(
        unavailable
            .downcast_ref::<ProjectFailure>()
            .unwrap()
            .category,
        "unavailable_source_state"
    );
    assert_eq!(
        safe_code(Some("private_document")),
        "project_operation_failed"
    );
    assert_eq!(safe_category("private_document"), "operational_failure");
}

#[tokio::test]
async fn stored_recipe_reopens_against_current_deployment_and_rejects_any_recipe_edit() {
    let fixture = crate::generator_runner::tests::RunnerFixture::new("success").await;
    let prepared = &fixture.prepared;
    let reopened = crate::generator_processing::reopen_generator_processing(
        &fixture.generator,
        prepared.processing_hash(),
        prepared.recipe_json(),
    )
    .unwrap();
    assert_eq!(reopened, *prepared);
    assert!(
        crate::generator_processing::reopen_generator_processing(
            &fixture.generator,
            &"f".repeat(64),
            prepared.recipe_json()
        )
        .is_err()
    );
    let mut json: serde_json::Value = serde_json::from_str(prepared.recipe_json()).unwrap();
    json["settings"]["placements"][0]["matrix"][3] = 123.into();
    let edited = String::from_utf8(cache_key::canonical_json_bytes(&json).unwrap()).unwrap();
    assert!(
        crate::generator_processing::reopen_generator_processing(
            &fixture.generator,
            prepared.processing_hash(),
            &edited
        )
        .is_err()
    );
    let another = crate::generator_runner::tests::RunnerFixture::new("malformed").await;
    assert!(
        crate::generator_processing::reopen_generator_processing(
            &another.generator,
            prepared.processing_hash(),
            prepared.recipe_json()
        )
        .is_err()
    );
}

#[tokio::test]
async fn request_association_is_immutable_and_reopen_checks_every_persisted_occurrence() {
    let (_directory, db, pool) = database().await;
    let request = request();
    let json = request.canonical_json().unwrap();
    db.insert_bambu_project_request(&request.request_identity, &json)
        .await
        .unwrap();
    assert!(
        db.insert_bambu_project_request(&request.request_identity, "{}")
            .await
            .is_err()
    );
    let fixture = crate::generator_runner::tests::RunnerFixture::new("success").await;
    let snapshot = "f".repeat(64);
    db.bind_bambu_project_output(
        &request.request_identity,
        &json,
        &fixture.prepared,
        &snapshot,
    )
    .await
    .unwrap();
    db.bind_bambu_project_output(
        &request.request_identity,
        &json,
        &fixture.prepared,
        &snapshot,
    )
    .await
    .unwrap();
    let record = db
        .bambu_project_request(&request.request_identity)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        record.processing_hash.as_deref(),
        Some(fixture.prepared.processing_hash())
    );
    assert_eq!(
        record.output_snapshot_identity.as_deref(),
        Some(snapshot.as_str())
    );
    assert!(
        db.bind_bambu_project_output(
            &request.request_identity,
            &json,
            &fixture.prepared,
            &"e".repeat(64)
        )
        .await
        .is_err()
    );
    assert_eq!(
        db.reopen_generator_processing(&fixture.generator, fixture.prepared.processing_hash())
            .await
            .unwrap(),
        fixture.prepared
    );
    let another = crate::generator_runner::tests::RunnerFixture::new("malformed").await;
    assert!(
        db.bind_bambu_project_output(
            &request.request_identity,
            &json,
            &another.prepared,
            &snapshot
        )
        .await
        .is_err()
    );
    sqlx::query("UPDATE generator_processing_occurrences SET placement_json = '{}' WHERE processing_hash = ? AND occurrence_order = 1")
        .bind(fixture.prepared.processing_hash()).execute(&pool).await.unwrap();
    assert!(
        db.reopen_generator_processing(&fixture.generator, fixture.prepared.processing_hash())
            .await
            .is_err()
    );
    let row = sqlx::query(
        "SELECT processing_hash FROM bambu_project_requests WHERE request_identity = ?",
    )
    .bind(&request.request_identity)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        row.get::<String, _>("processing_hash"),
        fixture.prepared.processing_hash()
    );
}

#[tokio::test]
async fn project_lease_extension_requires_current_running_attempt() {
    let (_directory, db, _pool) = database().await;
    db.enqueue_job("synthetic-project", JOB_KIND, "{}")
        .await
        .unwrap();
    let first = db.claim_next_job(0).await.unwrap().unwrap();
    let second = db.claim_next_job(60).await.unwrap().unwrap();
    assert!(
        !db.extend_job_lease(&first.work_key, first.attempt, 2100)
            .await
            .unwrap()
    );
    assert!(
        db.extend_job_lease(&second.work_key, second.attempt, 2100)
            .await
            .unwrap()
    );
    assert!(
        db.finish_job(&second.work_key, second.attempt, "ready", None)
            .await
            .unwrap()
    );
    assert!(
        !db.extend_job_lease(&second.work_key, second.attempt, 2100)
            .await
            .unwrap()
    );
}

type RuntimeObjects = Arc<std::sync::Mutex<HashMap<String, (Vec<u8>, String)>>>;
type RuntimeCalls = Arc<std::sync::Mutex<Vec<String>>>;

struct RuntimeFixture {
    _directory: tempfile::TempDir,
    _runner: crate::generator_runner::tests::RunnerFixture,
    state: AppState,
    model: catalog::Model,
    request: ProjectJob,
    prepared: PreparedGeneratorProcessing,
    object_key: String,
    snapshot_identity: String,
    objects: RuntimeObjects,
    calls: RuntimeCalls,
    server: tokio::task::JoinHandle<()>,
}

impl Drop for RuntimeFixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl RuntimeFixture {
    async fn new() -> Self {
        use crate::{
            catalog::*,
            config::{OnshapeConfig, StorageConfig},
            db::{GeneratorArtifactFileInsert, GeneratorArtifactStage},
        };
        let (directory, db, _pool) = database().await;
        let runner = crate::generator_runner::tests::RunnerFixture::new("success").await;
        let generator = runner.generator.clone();
        let inputs = crate::onshape_geometry::generator_project_test_inputs(
            &bambu_deployment::input_policy(&generator),
        )
        .await;
        let root = &inputs.provenance().plan.root;
        let selectors = ["i-a1", "i-b", "i-a2"]
            .into_iter()
            .map(|id| SelectionSelector::Occurrence {
                occurrence_path: vec![id.to_owned()],
            })
            .collect::<Vec<_>>();
        let model = Model {
            catalog_schema_version: CATALOG_SCHEMA_VERSION,
            entry_version: CATALOG_ENTRY_VERSION,
            slug: "synthetic-model".to_owned(),
            name: "Synthetic model".to_owned(),
            description: "Synthetic source-neutral project test.".to_owned(),
            published: true,
            tags: vec![],
            thumbnail: None,
            onshape: OnshapeSource {
                document_id: root.document_id.clone(),
                version_id: root.version_id.clone(),
                element_id: root.element_id.clone(),
                element_kind: ElementKind::Assembly,
                link_document_id: None,
            },
            exports: ExportConfig {
                downloads: vec![DownloadFormat::Step],
                preview: PreviewFormat::Glb,
                preview_options: PreviewOptions {
                    resolution: PreviewResolution::Fine,
                },
                download_options: DownloadOptions {
                    step_version_string: StepVersionString::Ap242,
                    stl: StlDownloadOptions {
                        resolution: MeshResolution::Fine,
                        stl_mode: StlMode::Binary,
                    },
                    three_mf: ThreeMfDownloadOptions {
                        resolution: MeshResolution::Fine,
                    },
                },
                bambu_project: Some(BambuProjectConfig {
                    selectors: selectors.clone(),
                }),
            },
            parameter_policy: ParameterPolicy {
                source: ParameterSource::Onshape,
                allow_unknown: false,
                auto_refresh: true,
            },
            parameter_presets: vec![],
            parameter_overrides: HashMap::new(),
        };
        db.replace_catalog(&Catalog::from_models(vec![model.clone()]).unwrap())
            .await
            .unwrap();
        let source_hash = inputs.manifest().source_identity.clone();
        db.upsert_source_resolution(crate::db::SourceResolutionUpsert {
            source_hash: &source_hash,
            model_slug: &model.slug,
            document_id: &root.document_id,
            version_id: &root.version_id,
            microversion_id: &root.document_microversion,
            element_id: &root.element_id,
            element_kind: "assembly",
            link_document_id: None,
            diagnostics_json: "{}",
        })
        .await
        .unwrap();
        let prepared = prepare_generator_processing(
            &generator,
            &bambu_deployment::compatibility(&generator),
            inputs.manifest(),
            inputs.settings(),
            inputs.expected_placements(),
            &bambu_deployment::protocol_inputs(&generator),
        )
        .unwrap();
        let mut request = ProjectJob {
            model_slug: model.slug.clone(),
            source_hash,
            config_hash: crate::cache_model::config_hash(
                &inputs.manifest().source_identity,
                crate::parameters::SCHEMA_VERSION,
                &Default::default(),
            )
            .unwrap(),
            values: HashMap::new(),
            selectors,
            generator_static_identity: generator.static_identity().to_owned(),
            policy_identity: profile_identity(&generator).unwrap(),
            catalog_identity: catalog_identity(&model).unwrap(),
            request_identity: String::new(),
        };
        request.request_identity = request.identity().unwrap();
        db.insert_bambu_project_request(
            &request.request_identity,
            &request.canonical_json().unwrap(),
        )
        .await
        .unwrap();
        let output = Arc::new(
            run_generator(
                &generator,
                inputs,
                &prepared,
                &GeneratorRunnerOptions::default(),
            )
            .await
            .unwrap(),
        );
        let snapshot = crate::generator_output_checkpoint::persist(
            &db.generator_output_root().unwrap(),
            output.clone(),
            &prepared,
        )
        .await
        .unwrap();
        db.bind_bambu_project_output(
            &request.request_identity,
            &request.canonical_json().unwrap(),
            &prepared,
            &snapshot,
        )
        .await
        .unwrap();
        let publication = target(
            &request,
            &generator,
            Some(&work_key(&request.request_identity)),
        )
        .unwrap();
        let object_key =
            crate::generator_publication::generator_publication_object_key(&prepared, &publication)
                .unwrap();
        let declaration = output.result().output.as_ref().unwrap();
        let result_json = serde_json::to_string(output.result()).unwrap();
        db.stage_generator_artifact(
            &prepared,
            GeneratorArtifactStage {
                model_slug: &model.slug,
                output_kind: &publication.output_kind,
                format: &publication.format,
                object_key: &object_key,
                content_type: &declaration.media_type,
                byte_len: declaration.byte_length as i64,
                sha256: &declaration.sha256,
                producing_job_key: publication.producing_job_key.as_deref(),
                parameter_schema_version: publication.parameter_schema_version,
                config_values_json: &publication.config_values_json,
            },
            &[GeneratorArtifactFileInsert {
                role: "generated_project",
                logical_path: &publication.logical_path,
                original_path: None,
                object_key: &object_key,
                content_type: &declaration.media_type,
                byte_len: declaration.byte_length as i64,
                sha256: &declaration.sha256,
                metadata_json: &result_json,
            }],
        )
        .await
        .unwrap();
        let schema = crate::parameters::ParameterSchema {
            schema_version: crate::parameters::SCHEMA_VERSION,
            source: model.onshape.clone(),
            parameters: vec![],
        };
        db.upsert_parameter_metadata(
            &request.source_hash,
            "synthetic-parameters-raw",
            "synthetic-parameters",
            "synthetic-schema",
            crate::parameters::SCHEMA_VERSION as i64,
        )
        .await
        .unwrap();
        let mut bytes = Vec::new();
        output.copy_to(&mut bytes).unwrap();
        let objects = Arc::new(std::sync::Mutex::new(HashMap::from([
            (
                format!("/synthetic-bucket/{object_key}"),
                (bytes, declaration.media_type.clone()),
            ),
            (
                "/synthetic-bucket/synthetic-parameters".to_owned(),
                (
                    serde_json::to_vec(&schema).unwrap(),
                    "application/json".to_owned(),
                ),
            ),
        ])));
        let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let router = axum::Router::new()
            .fallback(runtime_storage)
            .with_state((objects.clone(), calls.clone()));
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let storage = crate::storage::StorageClient::new(StorageConfig {
            bucket: "synthetic-bucket".to_owned(),
            endpoint_url: Some(endpoint),
            region: "auto".to_owned(),
            access_key_id: Some("synthetic-access".to_owned()),
            secret_access_key: Some("synthetic-secret".to_owned()),
            public_base_url: Some("https://cdn.example.test".to_owned()),
            force_path_style: true,
        })
        .await
        .unwrap();
        let mut onshape = crate::onshape::OnshapeClient::new(OnshapeConfig {
            base_url: "https://cad.onshape.com".to_owned(),
            access_key: None,
            secret_key: None,
        })
        .unwrap();
        // Any accidental strict acquisition can reach only a closed loopback
        // port. Successful recovery therefore proves there was no reacquisition.
        onshape.set_test_selection_origin("http://127.0.0.1:9".to_owned());
        let state = AppState {
            db,
            storage,
            onshape,
            deployed_generator: Some(
                crate::deployed_generator::DeployedGeneratorAvailability::Available(Box::new(
                    generator,
                )),
            ),
        };
        Self {
            _directory: directory,
            _runner: runner,
            state,
            model,
            request,
            prepared,
            object_key,
            snapshot_identity: snapshot,
            objects,
            calls,
            server,
        }
    }

    async fn remove_publication(&self) {
        let url = format!(
            "sqlite://{}?mode=rwc",
            self._directory.path().join("project.db").display()
        );
        let pool = SqlitePool::connect(&url).await.unwrap();
        sqlx::query("DELETE FROM artifact_files")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM artifact_sets")
            .execute(&pool)
            .await
            .unwrap();
        self.objects
            .lock()
            .unwrap()
            .remove(&format!("/synthetic-bucket/{}", self.object_key));
    }

    async fn restarted_state(&self) -> AppState {
        let url = format!(
            "sqlite://{}?mode=rwc",
            self._directory.path().join("project.db").display()
        );
        let mut state = self.state.clone();
        state.db = crate::db::Database::connect(&url).await.unwrap();
        state
    }
}

type RuntimeStorage = (RuntimeObjects, RuntimeCalls);

async fn runtime_storage(
    axum::extract::State((objects, calls)): axum::extract::State<RuntimeStorage>,
    request: axum::extract::Request,
) -> axum::response::Response {
    let method = request.method().as_str().to_owned();
    let path = request.uri().path().to_owned();
    calls.lock().unwrap().push(method.clone());
    if method == "PUT" {
        let media = request.headers()["content-type"]
            .to_str()
            .unwrap()
            .to_owned();
        let chunked = request
            .headers()
            .get("content-encoding")
            .is_some_and(|value| value.to_str().unwrap().contains("aws-chunked"));
        let mut bytes = axum::body::to_bytes(request.into_body(), 1_048_576)
            .await
            .unwrap()
            .to_vec();
        if chunked {
            bytes = decode_storage_chunks(&bytes);
        }
        objects.lock().unwrap().insert(path, (bytes, media));
        return axum::response::Response::builder()
            .status(200)
            .body(axum::body::Body::empty())
            .unwrap();
    }
    let objects = objects.lock().unwrap();
    let Some((bytes, media)) = objects.get(&path) else {
        return axum::response::Response::builder()
            .status(404)
            .body(axum::body::Body::empty())
            .unwrap();
    };
    axum::response::Response::builder()
        .status(200)
        .header("content-type", media)
        .header("content-length", bytes.len().to_string())
        .body(if method == "HEAD" {
            axum::body::Body::empty()
        } else {
            axum::body::Body::from(bytes.clone())
        })
        .unwrap()
}

fn decode_storage_chunks(mut bytes: &[u8]) -> Vec<u8> {
    let mut decoded = Vec::new();
    loop {
        let end = bytes.windows(2).position(|part| part == b"\r\n").unwrap();
        let size = usize::from_str_radix(
            std::str::from_utf8(&bytes[..end])
                .unwrap()
                .split(';')
                .next()
                .unwrap(),
            16,
        )
        .unwrap();
        if size == 0 {
            return decoded;
        }
        bytes = &bytes[end + 2..];
        decoded.extend_from_slice(&bytes[..size]);
        bytes = &bytes[size + 2..];
    }
}

#[tokio::test]
async fn completed_upload_reconciles_without_acquisition_and_exact_cached_request_reuses_ready_project()
 {
    let fixture = RuntimeFixture::new().await;
    assert_ne!(
        fixture.request.config_hash,
        fixture
            .prepared
            .recipe()
            .input_manifest
            .configuration_identity,
        "typed request configuration and Onshape configured-response identities have distinct domains"
    );
    assert_ne!(
        status(
            &fixture.state,
            &fixture.model,
            &fixture.request.request_identity
        )
        .await
        .unwrap()
        .status,
        "ready"
    );
    execute_job(
        &fixture.state,
        &fixture.request,
        &work_key(&fixture.request.request_identity),
    )
    .await
    .unwrap();
    assert_eq!(*fixture.calls.lock().unwrap(), vec!["GET", "HEAD", "GET"]);
    let ready = status(
        &fixture.state,
        &fixture.model,
        &fixture.request.request_identity,
    )
    .await
    .unwrap();
    assert_eq!(ready.status, "ready");
    assert_eq!(
        ready.object_key.as_deref(),
        Some(fixture.object_key.as_str())
    );
    assert_eq!(ready.media_type.as_deref(), Some("model/3mf"));
    let validated = ValidatedConfiguration {
        values: HashMap::new(),
        typed_values: Default::default(),
    };
    let cached = enqueue(
        &fixture.state,
        &fixture.model,
        &validated,
        &fixture.request.selectors,
    )
    .await
    .unwrap();
    assert_eq!(cached.request_identity, fixture.request.request_identity);
    assert_eq!(fixture.calls.lock().unwrap().len(), 3);
    assert!(
        fixture
            .state
            .db
            .job(&work_key(&cached.request_identity))
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn failed_storage_reverification_repairs_the_exact_retained_output() {
    let fixture = RuntimeFixture::new().await;
    execute_job(
        &fixture.state,
        &fixture.request,
        &work_key(&fixture.request.request_identity),
    )
    .await
    .unwrap();
    fixture
        .objects
        .lock()
        .unwrap()
        .get_mut(&format!("/synthetic-bucket/{}", fixture.object_key))
        .unwrap()
        .0[0] ^= 1;
    execute_job(
        &fixture.state,
        &fixture.request,
        &work_key(&fixture.request.request_identity),
    )
    .await
    .unwrap();
    let repaired = status(
        &fixture.state,
        &fixture.model,
        &fixture.request.request_identity,
    )
    .await
    .unwrap();
    assert_eq!(repaired.status, "ready");
    assert!(repaired.failure_code.is_none());
    assert_eq!(
        fixture.objects.lock().unwrap()[&format!("/synthetic-bucket/{}", fixture.object_key)]
            .0
            .as_slice(),
        b"synthetic generated project"
    );
    assert_eq!(
        fixture
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|method| *method == "PUT")
            .count(),
        1
    );
    assert_eq!(
        fixture
            .state
            .db
            .bambu_project_request(&fixture.request.request_identity)
            .await
            .unwrap()
            .unwrap()
            .processing_hash
            .as_deref(),
        Some(fixture.prepared.processing_hash())
    );
}

#[tokio::test]
async fn changed_catalog_selection_or_missing_deployment_hides_previously_ready_project() {
    let mut fixture = RuntimeFixture::new().await;
    execute_job(
        &fixture.state,
        &fixture.request,
        &work_key(&fixture.request.request_identity),
    )
    .await
    .unwrap();
    let mut changed = fixture.model.clone();
    changed
        .exports
        .bambu_project
        .as_mut()
        .unwrap()
        .selectors
        .reverse();
    let stale = status(&fixture.state, &changed, &fixture.request.request_identity)
        .await
        .unwrap();
    assert_eq!(stale.status, "superseded");
    assert!(stale.object_key.is_none());
    fixture.state.deployed_generator = None;
    let unavailable = status(
        &fixture.state,
        &fixture.model,
        &fixture.request.request_identity,
    )
    .await
    .unwrap();
    assert_eq!(unavailable.status, "unavailable");
    assert!(unavailable.object_key.is_none());
    assert_eq!(
        unavailable.failure_category.as_deref(),
        Some("deployment_unavailable")
    );
    let mut linked = fixture.model.clone();
    linked.onshape.link_document_id = Some("synthetic-linked-document".to_owned());
    let error = validate_selectors(&linked, &fixture.request.selectors).unwrap_err();
    let safe = error.downcast_ref::<ProjectFailure>().unwrap();
    assert_eq!(safe.category, "unavailable_source_state");
    assert_eq!(safe.code, "linked_source_unsupported");
}

#[tokio::test]
async fn unconfigured_project_route_returns_unavailable_without_queueing_or_storage_calls() {
    let mut fixture = RuntimeFixture::new().await;
    fixture.state.deployed_generator = None;
    let response = crate::generate_bambu_project(
        axum::extract::State(fixture.state.clone()),
        axum::extract::Path(fixture.model.slug.clone()),
        axum::Form(HashMap::new()),
    )
    .await
    .unwrap();
    assert!(
        response
            .0
            .contains("Bambu project generation is unavailable")
    );
    assert!(fixture.state.db.jobs(20).await.unwrap().is_empty());
    assert!(fixture.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn legacy_associated_recipe_without_snapshot_requires_recovery_without_new_acquisition() {
    let fixture = RuntimeFixture::new().await;
    let url = format!(
        "sqlite://{}?mode=rwc",
        fixture._directory.path().join("project.db").display()
    );
    let pool = SqlitePool::connect(&url).await.unwrap();
    sqlx::query("DELETE FROM artifact_files")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM artifact_sets")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE bambu_project_requests SET output_snapshot_identity = NULL")
        .execute(&pool)
        .await
        .unwrap();
    let error = execute_job(
        &fixture.state,
        &fixture.request,
        &work_key(&fixture.request.request_identity),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "project generation failed: operational_failure (project_input_recovery_required)"
    );
    assert_eq!(*fixture.calls.lock().unwrap(), vec!["GET"]);
    let status = status(
        &fixture.state,
        &fixture.model,
        &fixture.request.request_identity,
    )
    .await
    .unwrap();
    assert_eq!(status.status, "failed");
    assert_eq!(
        status.failure_code.as_deref(),
        Some("project_input_recovery_required")
    );
    assert!(status.object_key.is_none());
    assert_eq!(
        fixture
            .state
            .db
            .bambu_project_request(&fixture.request.request_identity)
            .await
            .unwrap()
            .unwrap()
            .processing_hash
            .as_deref(),
        Some(fixture.prepared.processing_hash())
    );
}

#[tokio::test]
async fn accepted_snapshot_resumes_after_restart_before_upload_without_reacquisition() {
    let fixture = RuntimeFixture::new().await;
    fixture.remove_publication().await;
    let restarted = fixture.restarted_state().await;
    assert_eq!(
        restarted.db.generator_output_root().unwrap(),
        fixture.state.db.generator_output_root().unwrap()
    );
    execute_job(
        &restarted,
        &fixture.request,
        &work_key(&fixture.request.request_identity),
    )
    .await
    .unwrap();
    assert_eq!(
        *fixture.calls.lock().unwrap(),
        vec!["GET", "PUT", "HEAD", "GET"]
    );
    let ready = status(
        &restarted,
        &fixture.model,
        &fixture.request.request_identity,
    )
    .await
    .unwrap();
    assert_eq!(ready.status, "ready");
    assert_eq!(
        ready.object_key.as_deref(),
        Some(fixture.object_key.as_str())
    );
    let record = restarted
        .db
        .bambu_project_request(&fixture.request.request_identity)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        record.output_snapshot_identity.as_deref(),
        Some(fixture.snapshot_identity.as_str())
    );
    assert_eq!(
        record.processing_hash.as_deref(),
        Some(fixture.prepared.processing_hash())
    );
}

#[tokio::test]
async fn corrupt_output_snapshot_is_rejected_without_upload_or_new_acquisition() {
    let fixture = RuntimeFixture::new().await;
    fixture.remove_publication().await;
    let retained = fixture
        .state
        .db
        .generator_output_root()
        .unwrap()
        .join(&fixture.snapshot_identity)
        .join("output.bin");
    let mut bytes = std::fs::read(&retained).unwrap();
    bytes[0] ^= 1;
    std::fs::write(&retained, bytes).unwrap();
    let restarted = fixture.restarted_state().await;
    let error = execute_job(
        &restarted,
        &fixture.request,
        &work_key(&fixture.request.request_identity),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "project generation failed: operational_failure (output_snapshot_integrity)"
    );
    assert_eq!(*fixture.calls.lock().unwrap(), vec!["GET"]);
    let failed = status(
        &restarted,
        &fixture.model,
        &fixture.request.request_identity,
    )
    .await
    .unwrap();
    assert_eq!(failed.status, "failed");
    assert!(failed.object_key.is_none());
    assert_eq!(
        failed.failure_code.as_deref(),
        Some("output_snapshot_integrity")
    );
}

#[tokio::test]
async fn snapshot_without_recipe_is_rejected_before_metadata_or_acquisition() {
    let fixture = RuntimeFixture::new().await;
    let url = format!(
        "sqlite://{}?mode=rwc",
        fixture._directory.path().join("project.db").display()
    );
    let pool = SqlitePool::connect(&url).await.unwrap();
    sqlx::query("UPDATE bambu_project_requests SET processing_hash = NULL")
        .execute(&pool)
        .await
        .unwrap();
    let error = execute_job(
        &fixture.state,
        &fixture.request,
        &work_key(&fixture.request.request_identity),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "project generation failed: operational_failure (stored_output_association)"
    );
    assert!(fixture.calls.lock().unwrap().is_empty());
    let failed = status(
        &fixture.state,
        &fixture.model,
        &fixture.request.request_identity,
    )
    .await
    .unwrap();
    assert_eq!(failed.status, "failed");
    assert_eq!(
        failed.failure_code.as_deref(),
        Some("stored_output_association")
    );
    assert!(failed.object_key.is_none());
}

#[tokio::test]
async fn runner_failure_leaves_request_unbound_and_an_exact_retry_can_be_queued() {
    let mut fixture = RuntimeFixture::new().await;
    let failed = crate::generator_runner::tests::RunnerFixture::new("structured").await;
    let generator = failed.generator.clone();
    let inputs = crate::onshape_geometry::generator_project_test_inputs(
        &bambu_deployment::input_policy(&generator),
    )
    .await;
    let prepared = prepare_generator_processing(
        &generator,
        &bambu_deployment::compatibility(&generator),
        inputs.manifest(),
        inputs.settings(),
        inputs.expected_placements(),
        &bambu_deployment::protocol_inputs(&generator),
    )
    .unwrap();
    let mut request = fixture.request.clone();
    request.generator_static_identity = generator.static_identity().to_owned();
    request.policy_identity = profile_identity(&generator).unwrap();
    request.request_identity = request.identity().unwrap();
    fixture.state.deployed_generator = Some(
        crate::deployed_generator::DeployedGeneratorAvailability::Available(Box::new(
            generator.clone(),
        )),
    );
    fixture
        .state
        .db
        .insert_bambu_project_request(
            &request.request_identity,
            &request.canonical_json().unwrap(),
        )
        .await
        .unwrap();
    let result = run_generator(
        &generator,
        inputs,
        &prepared,
        &GeneratorRunnerOptions::default(),
    )
    .await;
    let error = persist_accepted_output(
        &fixture.state,
        &request,
        &prepared,
        &fixture.state.db.generator_output_root().unwrap(),
        result,
    )
    .await
    .err()
    .unwrap();
    assert_eq!(
        error.to_string(),
        "project generation failed: operational_failure (generator_reported_failure)"
    );
    let record = fixture
        .state
        .db
        .bambu_project_request(&request.request_identity)
        .await
        .unwrap()
        .unwrap();
    assert!(record.processing_hash.is_none() && record.output_snapshot_identity.is_none());
    assert!(
        fixture
            .state
            .db
            .reopen_generator_processing(&generator, prepared.processing_hash())
            .await
            .is_err()
    );
    let queued = enqueue(
        &fixture.state,
        &fixture.model,
        &ValidatedConfiguration {
            values: HashMap::new(),
            typed_values: Default::default(),
        },
        &request.selectors,
    )
    .await
    .unwrap();
    assert_eq!(queued.request_identity, request.request_identity);
    assert_eq!(
        fixture
            .state
            .db
            .job(&work_key(&queued.request_identity))
            .await
            .unwrap()
            .unwrap()
            .status,
        "queued"
    );
    assert!(fixture.calls.lock().unwrap().is_empty());
}

#[derive(Clone)]
struct FreshApiScript {
    replies: Arc<std::sync::Mutex<std::collections::VecDeque<FreshApiReply>>>,
    requests: Arc<std::sync::Mutex<Vec<FreshApiRequest>>>,
}

struct FreshApiReply {
    media: &'static str,
    bytes: Vec<u8>,
}

struct FreshApiRequest {
    method: String,
    path: String,
    bytes: Vec<u8>,
}

fn fresh_json(value: serde_json::Value) -> FreshApiReply {
    FreshApiReply {
        media: "application/json",
        bytes: serde_json::to_vec(&value).unwrap(),
    }
}

async fn fresh_api(
    axum::extract::State(script): axum::extract::State<FreshApiScript>,
    request: axum::extract::Request,
) -> axum::response::Response {
    let method = request.method().as_str().to_owned();
    let path = request.uri().path().to_owned();
    let bytes = axum::body::to_bytes(request.into_body(), 65_536)
        .await
        .unwrap()
        .to_vec();
    script.requests.lock().unwrap().push(FreshApiRequest {
        method,
        path,
        bytes,
    });
    let Some(reply) = script.replies.lock().unwrap().pop_front() else {
        return axum::response::Response::builder()
            .status(500)
            .body(axum::body::Body::empty())
            .unwrap();
    };
    axum::response::Response::builder()
        .status(200)
        .header("content-type", reply.media)
        .header("content-length", reply.bytes.len().to_string())
        .header("connection", "close")
        .body(axum::body::Body::from(reply.bytes))
        .unwrap()
}

#[tokio::test]
async fn fresh_worker_executes_trusted_acquisition_runner_checkpoint_and_verified_publication() {
    let mut fixture = RuntimeFixture::new().await;
    fixture.model.onshape = catalog::OnshapeSource {
        document_id: "fresh-document".to_owned(),
        version_id: "fresh-version".to_owned(),
        element_id: "fresh-element".to_owned(),
        element_kind: catalog::ElementKind::PartStudio,
        link_document_id: None,
    };
    let parts = ["fresh-part-a", "fresh-part-b", "fresh-part-c"];
    fixture.model.exports.bambu_project = Some(catalog::BambuProjectConfig {
        selectors: parts
            .iter()
            .map(|id| SelectionSelector::Part {
                part_id: (*id).to_owned(),
            })
            .collect(),
    });
    fixture
        .state
        .db
        .replace_catalog(&catalog::Catalog::from_models(vec![fixture.model.clone()]).unwrap())
        .await
        .unwrap();
    let source_hash =
        crate::cache_model::source_hash(&crate::cache_model::ResolvedOnshapeSourceIdentity {
            document_id: fixture.model.onshape.document_id.clone(),
            version_id: fixture.model.onshape.version_id.clone(),
            microversion_id: "fresh-microversion".to_owned(),
            element_id: fixture.model.onshape.element_id.clone(),
            element_kind: catalog::ElementKind::PartStudio,
            link_document_id: None,
        })
        .unwrap();
    fixture
        .state
        .db
        .upsert_source_resolution(crate::db::SourceResolutionUpsert {
            source_hash: &source_hash,
            model_slug: &fixture.model.slug,
            document_id: &fixture.model.onshape.document_id,
            version_id: &fixture.model.onshape.version_id,
            microversion_id: "fresh-microversion",
            element_id: &fixture.model.onshape.element_id,
            element_kind: "part_studio",
            link_document_id: None,
            diagnostics_json: "{}",
        })
        .await
        .unwrap();
    let schema = crate::parameters::ParameterSchema {
        schema_version: crate::parameters::SCHEMA_VERSION,
        source: fixture.model.onshape.clone(),
        parameters: vec![],
    };
    fixture.objects.lock().unwrap().insert(
        "/synthetic-bucket/fresh-parameters".to_owned(),
        (
            serde_json::to_vec(&schema).unwrap(),
            "application/json".to_owned(),
        ),
    );
    fixture
        .state
        .db
        .upsert_parameter_metadata(
            &source_hash,
            "fresh-parameters-raw",
            "fresh-parameters",
            "fresh-schema",
            crate::parameters::SCHEMA_VERSION as i64,
        )
        .await
        .unwrap();
    let version = || {
        fresh_json(
            serde_json::json!({ "documentId":"fresh-document", "id":"fresh-version", "microversion":"fresh-microversion" }),
        )
    };
    let metadata: Vec<_> = parts
        .iter()
        .map(|id| {
            serde_json::json!({
                "elementId":"fresh-element", "microversionId":"fresh-microversion", "partId":id,
                "configurationId":"fresh-response-configuration", "bodyType":"solid",
                "isFlattenedBody":false, "isMesh":false, "meshState":"NO_MESH", "isHidden":false,
                "name":"Fresh source-neutral part", "description":"",
            })
        })
        .collect();
    let mut replies = std::collections::VecDeque::from([
        version(),
        fresh_json(
            serde_json::json!({ "encodedId":"fresh-original-encoding", "queryParam":"configuration=fresh-original-encoding" }),
        ),
        version(),
        fresh_json(serde_json::json!(metadata)),
        version(),
    ]);
    for position in 0..3 {
        replies.push_back(fresh_json(serde_json::json!({ "id":format!("fresh-translation-{position}"),
            "requestState":"DONE", "resultExternalDataIds":[format!("fresh-external-{position}")] })));
        replies.push_back(FreshApiReply {
            media: "application/octet-stream",
            bytes: b"fresh opaque geometry bytes".to_vec(),
        });
    }
    let script = FreshApiScript {
        replies: Arc::new(std::sync::Mutex::new(replies)),
        requests: Arc::new(std::sync::Mutex::new(Vec::new())),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    fixture
        .state
        .onshape
        .set_test_selection_origin(format!("http://{}", listener.local_addr().unwrap()));
    let router = axum::Router::new()
        .fallback(fresh_api)
        .with_state(script.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let request = enqueue(
        &fixture.state,
        &fixture.model,
        &ValidatedConfiguration {
            values: HashMap::new(),
            typed_values: Default::default(),
        },
        &fixture
            .model
            .exports
            .bambu_project
            .as_ref()
            .unwrap()
            .selectors,
    )
    .await
    .unwrap();
    let before = fixture
        .state
        .db
        .bambu_project_request(&request.request_identity)
        .await
        .unwrap()
        .unwrap();
    assert!(before.processing_hash.is_none() && before.output_snapshot_identity.is_none());
    assert!(script.requests.lock().unwrap().is_empty());
    let result = execute_job(
        &fixture.state,
        &request,
        &work_key(&request.request_identity),
    )
    .await;
    server.abort();
    result.unwrap();
    {
        let requests = script.requests.lock().unwrap();
        assert_eq!(requests.len(), 11);
        assert!(script.replies.lock().unwrap().is_empty());
        let creates: Vec<_> = requests
            .iter()
            .filter(|request| request.method == "POST" && request.path.ends_with("/translations"))
            .collect();
        assert_eq!(creates.len(), 3);
        for (created, part) in creates.iter().zip(parts) {
            let body: serde_json::Value = serde_json::from_slice(&created.bytes).unwrap();
            assert_eq!(body["configuration"], "fresh-original-encoding");
            assert_eq!(body["partIds"], part);
        }
    }
    let after = fixture
        .state
        .db
        .bambu_project_request(&request.request_identity)
        .await
        .unwrap()
        .unwrap();
    assert!(after.processing_hash.is_some() && after.output_snapshot_identity.is_some());
    let ready = status(&fixture.state, &fixture.model, &request.request_identity)
        .await
        .unwrap();
    assert_eq!(ready.status, "ready");
    assert_eq!(
        *fixture.calls.lock().unwrap(),
        vec!["GET", "PUT", "HEAD", "GET"]
    );
    let generator = configured(&fixture.state).unwrap();
    let prepared = fixture
        .state
        .db
        .reopen_generator_processing(generator, after.processing_hash.as_deref().unwrap())
        .await
        .unwrap();
    assert_ne!(
        prepared.recipe().input_manifest.configuration_identity,
        request.config_hash
    );
    let retained = crate::generator_output_checkpoint::reopen(
        &fixture.state.db.generator_output_root().unwrap(),
        after.output_snapshot_identity.as_deref().unwrap(),
        &prepared,
    )
    .await
    .unwrap();
    let mut bytes = Vec::new();
    retained.copy_to(&mut bytes).unwrap();
    assert_eq!(bytes.as_slice(), b"synthetic generated project");
    let cached = enqueue(
        &fixture.state,
        &fixture.model,
        &ValidatedConfiguration {
            values: HashMap::new(),
            typed_values: Default::default(),
        },
        &request.selectors,
    )
    .await
    .unwrap();
    assert_eq!(cached.request_identity, request.request_identity);
    assert_eq!(script.requests.lock().unwrap().len(), 11);
    assert_eq!(
        *fixture.calls.lock().unwrap(),
        vec!["GET", "PUT", "HEAD", "GET"]
    );
}

#[tokio::test]
async fn generated_state_reset_clears_project_association_before_recipe_and_preserves_catalog_opt_in()
 {
    let fixture = RuntimeFixture::new().await;
    let deleted = fixture.state.db.clear_generated_state().await.unwrap();
    assert!(
        deleted
            .iter()
            .any(|table| table.table == "bambu_project_requests" && table.rows == 1)
    );
    assert!(
        fixture
            .state
            .db
            .bambu_project_request(&fixture.request.request_identity)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        fixture
            .state
            .db
            .reopen_generator_processing(
                &fixture._runner.generator,
                fixture.prepared.processing_hash()
            )
            .await
            .is_err()
    );
    let model = fixture
        .state
        .db
        .published_catalog_model(&fixture.model.slug)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        model.exports.bambu_project.unwrap().selectors,
        fixture.request.selectors
    );
}
