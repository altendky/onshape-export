//! Synthetic acquisition tests: no credentials, private source, or live APIs.

use std::{
    collections::VecDeque,
    io::Read,
    net::TcpListener,
    sync::atomic::{AtomicBool, Ordering},
    thread,
};

use super::*;
use serde_json::json;

const ENCODING: &str = "original+encoding%value";

#[derive(Clone, Debug)]
struct CapturedRequest {
    method: String,
    target: String,
    body: Vec<u8>,
}

struct Server {
    api: OnshapeApi,
    requests: Arc<Mutex<Vec<CapturedRequest>>>,
    stopped: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Server {
    fn new(replies: Vec<Vec<u8>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let api = OnshapeApi::for_test(&format!("http://{}", listener.local_addr().unwrap()));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stopped = Arc::new(AtomicBool::new(false));
        let captured = Arc::clone(&requests);
        let stop = Arc::clone(&stopped);
        let thread = thread::spawn(move || {
            let mut replies: VecDeque<_> = replies.into();
            while !stop.load(Ordering::Acquire) {
                let (mut socket, _) = match listener.accept() {
                    Ok(value) => value,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                        continue;
                    }
                    Err(error) => panic!("synthetic listener: {error}"),
                };
                socket
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0; 4096];
                let header_end = loop {
                    let count = socket.read(&mut buffer).unwrap();
                    assert_ne!(count, 0);
                    bytes.extend_from_slice(&buffer[..count]);
                    if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        break index + 4;
                    }
                    assert!(bytes.len() <= 65_536);
                };
                let headers = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
                let length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                while bytes.len() < header_end + length {
                    let count = socket.read(&mut buffer).unwrap();
                    assert_ne!(count, 0);
                    bytes.extend_from_slice(&buffer[..count]);
                }
                let fields: Vec<_> = headers.lines().next().unwrap().split_whitespace().collect();
                captured.lock().unwrap().push(CapturedRequest {
                    method: fields[0].to_owned(),
                    target: fields[1].to_owned(),
                    body: bytes[header_end..header_end + length].to_vec(),
                });
                let response = replies.pop_front().unwrap_or_else(|| {
                    b"HTTP/1.1 500 Unexpected\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .to_vec()
                });
                socket.write_all(&response).unwrap();
            }
        });
        Self {
            api,
            requests,
            stopped,
            thread: Some(thread),
        }
    }

    fn finish(mut self) -> Vec<CapturedRequest> {
        self.stop();
        self.requests.lock().unwrap().clone()
    }

    fn stop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

fn response(media: &str, body: &[u8]) -> Vec<u8> {
    let mut bytes = format!("HTTP/1.1 200 OK\r\nContent-Type: {media}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

fn reply(value: Value) -> Vec<u8> {
    response("application/json", &serde_json::to_vec(&value).unwrap())
}
fn version(microversion: &str) -> Vec<u8> {
    reply(json!({"documentId":"d", "id":"v", "microversion":microversion}))
}
fn done(id: &str, external: &str) -> Vec<u8> {
    reply(json!({"id":id,"requestState":"DONE","resultExternalDataIds":[external]}))
}
fn bytes() -> Vec<u8> {
    response("application/octet-stream", b"opaque geometry bytes")
}

fn metadata(element: &str, part: &str, configuration: &str, name: &str) -> Value {
    json!({"elementId":element,"microversionId":"m","partId":part,"configurationId":configuration,
        "bodyType":"solid","isFlattenedBody":false,"isMesh":false,"meshState":"NO_MESH",
        "isHidden":false,"name":name,"description":""})
}

fn part_reply() -> Vec<u8> {
    reply(json!([metadata(
        "e",
        "p",
        "response-config",
        "Synthetic part"
    )]))
}
fn part_request_selector() -> Vec<SelectionSelector> {
    vec![SelectionSelector::Part {
        part_id: "p".to_owned(),
    }]
}

async fn request(
    db: &Database,
    api: &OnshapeApi,
    kind: SelectionElementKind,
    selectors: Vec<SelectionSelector>,
) -> SelectionRequest {
    let source = ResolvedOnshapeSourceIdentity {
        document_id: "d".to_owned(),
        version_id: "v".to_owned(),
        microversion_id: "m".to_owned(),
        element_id: "e".to_owned(),
        element_kind: match kind {
            SelectionElementKind::PartStudio => ElementKind::PartStudio,
            SelectionElementKind::Assembly => ElementKind::Assembly,
        },
        link_document_id: None,
    };
    let handoff = configuration_encoding::seed(
        db,
        api,
        &source,
        &BTreeMap::new(),
        ENCODING,
        "configuration=original%2Bencoding%25value",
    )
    .await;
    SelectionRequest {
        document_id: "d".to_owned(),
        version_id: "v".to_owned(),
        element_id: "e".to_owned(),
        element_kind: kind,
        configuration_encoding: handoff,
        selectors,
    }
}

async fn planned(db: &Database, api: &OnshapeApi) -> TrustedAcquisitionPlan {
    let request = request(
        db,
        api,
        SelectionElementKind::PartStudio,
        part_request_selector(),
    )
    .await;
    record_planning_invocation(db, api, request).await.unwrap()
}

fn scratch() -> tempfile::TempDir {
    let root = std::env::temp_dir().join("agents");
    std::fs::create_dir_all(&root).unwrap();
    tempfile::tempdir_in(root).unwrap()
}

#[tokio::test]
async fn original_invocation_roundtrips_and_acquires_exact_request_and_causal_download() {
    let server = Server::new(vec![
        version("m"),
        part_reply(),
        version("m"),
        done("t", "external"),
        bytes(),
    ]);
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let trusted = planned(&db, &server.api).await;
    assert_eq!(trusted.record.configuration_encoding.encoded_id, ENCODING);
    assert_eq!(
        trusted.plan().root.configuration_identity,
        "response-config"
    );
    let lookup = trusted.lookup_request().unwrap();
    let reopened = lookup_acquisition_plan(&db, &server.api, &lookup)
        .await
        .unwrap();
    let directory = scratch();
    let acquired = acquire_geometry(&db, &server.api, &reopened, directory.path())
        .await
        .unwrap();
    assert_eq!(acquired.plan, *trusted.plan());
    assert_eq!(
        acquired.provenance_identity,
        trusted.record.provenance_identity
    );
    assert_eq!(acquired.bindings.len(), 1);
    let binding = &acquired.bindings[0];
    assert_eq!(binding.position, 0);
    assert_eq!(
        binding.plan_local_object_identity,
        acquired.plan.objects[0].plan_local_object_identity
    );
    let mut retained = Vec::new();
    binding.retained_payload.copy_to(&mut retained).unwrap();
    assert_eq!(retained, b"opaque geometry bytes");
    assert_eq!(
        binding.retained_payload.sha256,
        cache_key::hex_sha256(&retained)
    );
    assert_eq!(binding.retained_payload.byte_length, retained.len() as u64);
    assert!(
        std::fs::read_dir(directory.path())
            .unwrap()
            .next()
            .is_none(),
        "retention leaves no visible paths"
    );
    let calls = server.finish();
    assert_eq!(calls.len(), 5);
    assert_eq!(calls[3].method, "POST");
    assert_eq!(
        calls[3].target,
        "/api/v16/partstudios/d/d/v/v/e/e/translations"
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&calls[3].body).unwrap(),
        json!({
            "formatName":"3MF", "storeInDocument":false, "notifyUser":false,
            "triggerAutoDownload":false,"configuration":ENCODING,"partIds":"p",
            "grouping":true,"resolution":"fine"
        })
    );
    assert_eq!(
        calls[4].target,
        "/api/v16/documents/d/d/externaldata/external"
    );
    assert_eq!(calls[4].method, "GET");
    let evidence = serde_json::to_value(&binding.acquisition_evidence).unwrap();
    assert_eq!(evidence["createTranslationId"], "t");
    assert_eq!(evidence["terminalTranslationId"], "t");
    assert_eq!(evidence["externalDataId"], "external");
}

#[tokio::test]
async fn snapshot_mismatch_has_zero_translation_creates() {
    let server = Server::new(vec![version("m"), part_reply(), version("different")]);
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let trusted = planned(&db, &server.api).await;
    let directory = scratch();
    let failure = acquire_geometry(&db, &server.api, &trusted, directory.path())
        .await
        .err()
        .unwrap();
    assert_eq!(failure.kind, FailureKind::UnavailableSourceState);
    let calls = server.finish();
    assert_eq!(calls.len(), 3);
    assert!(calls.iter().all(|call| call.method == "GET"));
}

#[test]
fn terminal_results_require_one_external_id_and_no_result_elements() {
    for elements in [None, Some(Value::Null), Some(json!([]))] {
        let mut value = json!({"resultExternalDataIds":["external"]});
        if let Some(elements) = elements {
            value["resultElementIds"] = elements;
        }
        assert_eq!(terminal_external_data(&value).unwrap(), "external");
    }
    for value in [
        json!({}),
        json!({"resultExternalDataIds":null}),
        json!({"resultExternalDataIds":[]}),
        json!({"resultExternalDataIds":["a","b"]}),
        json!({"resultExternalDataIds":[null]}),
        json!({"resultExternalDataIds":[""]}),
        json!({"resultExternalDataIds":["a"],"resultElementIds":["e"]}),
        json!({"resultExternalDataIds":["a"],"resultElementIds":{}}),
    ] {
        assert!(terminal_external_data(&value).is_err(), "accepted {value}");
    }
}

#[tokio::test]
async fn translation_identity_state_and_source_echoes_are_validated() {
    let server = Server::new(vec![version("m"), part_reply()]);
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let trusted = planned(&db, &server.api).await;
    let leaf = &trusted.plan().objects[0].configured_leaf;
    for state in ["ACTIVE", "DONE", "FAILED"] {
        assert!(
            translation(
                &json!({"id":"t","requestState":state}),
                trusted.plan(),
                leaf,
                Some("t")
            )
            .is_ok()
        );
    }
    for value in [
        json!({"id":"other","requestState":"DONE"}),
        json!({"id":"t","requestState":"WAITING"}),
        json!({"id":"t","requestState":null}),
        json!({"id":"","requestState":"DONE"}),
    ] {
        assert!(translation(&value, trusted.plan(), leaf, Some("t")).is_err());
    }
    for (field, expected) in [
        ("documentId", "d"),
        ("versionId", "v"),
        ("requestElementId", "e"),
        ("resultDocumentId", "d"),
    ] {
        let mut value = json!({"id":"t","requestState":"DONE"});
        value[field] = json!(expected);
        assert!(translation(&value, trusted.plan(), leaf, Some("t")).is_ok());
        for wrong in [json!("wrong"), Value::Null, json!(42)] {
            value[field] = wrong;
            assert!(translation(&value, trusted.plan(), leaf, Some("t")).is_err());
        }
    }
    assert_eq!(server.finish().len(), 2);
}

#[tokio::test]
async fn active_translation_polls_same_id_then_downloads_terminal_result() {
    let server = Server::new(vec![
        version("m"),
        part_reply(),
        version("m"),
        reply(json!({"id":"t","requestState":"ACTIVE","resultExternalDataIds":["premature"]})),
        done("t", "terminal"),
        bytes(),
    ]);
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let trusted = planned(&db, &server.api).await;
    let directory = scratch();
    acquire_with_timing(&db, &server.api, &trusted, directory.path(), fast_timing())
        .await
        .unwrap();
    let calls = server.finish();
    assert_eq!(calls.len(), 6);
    assert_eq!(calls[4].target, "/api/v16/translations/t");
    assert_eq!(
        calls[5].target,
        "/api/v16/documents/d/d/externaldata/terminal"
    );
}

#[tokio::test]
async fn failed_or_unknown_translation_never_downloads() {
    for state in ["FAILED", "UNKNOWN"] {
        let server = Server::new(vec![
            version("m"),
            part_reply(),
            version("m"),
            reply(json!({"id":"t","requestState":state,"resultExternalDataIds":["e"]})),
        ]);
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let trusted = planned(&db, &server.api).await;
        let directory = scratch();
        assert!(
            acquire_geometry(&db, &server.api, &trusted, directory.path())
                .await
                .is_err()
        );
        assert_eq!(server.finish().len(), 4);
    }
}

#[tokio::test]
async fn retention_failure_returns_no_partial_success() {
    let server = Server::new(vec![
        version("m"),
        part_reply(),
        version("m"),
        done("t", "e"),
        bytes(),
    ]);
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let trusted = planned(&db, &server.api).await;
    let directory = scratch();
    assert!(
        acquire_geometry(
            &db,
            &server.api,
            &trusted,
            &directory.path().join("missing")
        )
        .await
        .is_err()
    );
    assert_eq!(server.finish().len(), 5);
}

fn fast_timing() -> AcquisitionTiming {
    AcquisitionTiming {
        poll_intervals: [Duration::from_millis(1); 5],
        ..AcquisitionTiming::default()
    }
}

async fn disk_database(directory: &tempfile::TempDir) -> (String, Database, sqlx::SqlitePool) {
    let url = format!(
        "sqlite://{}?mode=rwc",
        directory.path().join("state.sqlite").display()
    );
    let db = Database::connect(&url).await.unwrap();
    let pool = sqlx::SqlitePool::connect(&url).await.unwrap();
    (url, db, pool)
}

#[tokio::test]
async fn disk_lookup_survives_restart_without_metadata_or_encoding_requests() {
    let server = Server::new(vec![
        version("m"),
        part_reply(),
        version("m"),
        done("t", "external"),
        bytes(),
    ]);
    let directory = scratch();
    let (url, db, pool) = disk_database(&directory).await;
    let trusted = planned(&db, &server.api).await;
    let lookup = trusted.lookup_request().unwrap();
    let original = bounded_canonical(&trusted.record).unwrap();
    drop(trusted);
    drop(db);
    pool.close().await;
    let db = Database::connect(&url).await.unwrap();
    let restored = lookup_acquisition_plan(&db, &server.api, &lookup)
        .await
        .unwrap();
    assert_eq!(bounded_canonical(&restored.record).unwrap(), original);
    acquire_geometry(&db, &server.api, &restored, directory.path())
        .await
        .unwrap();
    assert_eq!(server.finish().len(), 5);
}

#[tokio::test]
async fn lookup_rejects_forged_or_swapped_valid_handoff_even_with_recomputed_hash() {
    let server = Server::new(vec![version("m"), part_reply()]);
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let trusted = planned(&db, &server.api).await;
    let source = source(trusted.plan());
    let other = configuration_encoding::seed(
        &db,
        &server.api,
        &source,
        &BTreeMap::from([(
            "synthetic-option".to_owned(),
            CanonicalParameterValue::Boolean { value: true },
        )]),
        "independently-valid-encoding",
        "configuration=independently-valid-encoding",
    )
    .await;
    configuration_encoding::validate_handoff(&db, &server.api, &source, &other)
        .await
        .unwrap();
    let before = db
        .acquisition_provenance(&trusted.plan().plan_identity)
        .await
        .unwrap()
        .unwrap();
    for handoff in [
        other,
        EncodingHandoff {
            encoded_id: "forged".to_owned(),
            ..trusted.record.configuration_encoding.clone()
        },
    ] {
        let mut record = trusted.record.clone();
        record.configuration_encoding = handoff;
        record.bindings[0].configuration_request_value =
            record.configuration_encoding.encoded_id.clone();
        record.provenance_identity = record.identity().unwrap();
        let lookup = ProvenanceLookup {
            provenance_schema_version: 1,
            plan_identity: trusted.plan().plan_identity.clone(),
            provenance_identity: record.provenance_identity.clone(),
            plan: Some(trusted.plan().clone()),
            record: Some(record),
        };
        assert!(
            lookup_acquisition_plan(&db, &server.api, &bounded_canonical(&lookup).unwrap())
                .await
                .is_err()
        );
    }
    assert_eq!(
        db.acquisition_provenance(&trusted.plan().plan_identity)
            .await
            .unwrap()
            .unwrap(),
        before
    );
    assert_eq!(server.finish().len(), 2);
}

#[tokio::test]
async fn malformed_missing_and_corrupt_associations_fail_before_network_or_create() {
    let server = Server::new(vec![version("m"), part_reply()]);
    let directory = scratch();
    let (_, db, pool) = disk_database(&directory).await;
    let trusted = planned(&db, &server.api).await;
    let lookup = trusted.lookup_request().unwrap();
    let canonical = String::from_utf8(bounded_canonical(&trusted.record).unwrap()).unwrap();
    for mutation in [
        "{invalid".to_owned(),
        format!("{canonical} "),
        canonical.replacen(
            "\"provenanceSchemaVersion\":1",
            "\"provenanceSchemaVersion\":2",
            1,
        ),
        canonical.replacen(
            "\"requestContract\":",
            "\"unknown\":true,\"requestContract\":",
            1,
        ),
    ] {
        sqlx::query("UPDATE acquisition_plan_provenance SET record_json = ?")
            .bind(&mutation)
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            lookup_acquisition_plan(&db, &server.api, &lookup)
                .await
                .is_err()
        );
        assert!(
            acquire_geometry(&db, &server.api, &trusted, directory.path())
                .await
                .is_err()
        );
    }
    sqlx::query("DELETE FROM acquisition_plan_provenance")
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        lookup_acquisition_plan(&db, &server.api, &lookup)
            .await
            .is_err()
    );
    assert!(
        acquire_geometry(&db, &server.api, &trusted, directory.path())
            .await
            .is_err()
    );
    for malformed in [
        br#"{"provenanceSchemaVersion":1,"planIdentity":"bad","provenanceIdentity":"bad"}"#
            .as_slice(),
        br#"{"provenanceSchemaVersion":1,"provenanceSchemaVersion":1}"#.as_slice(),
    ] {
        assert!(
            lookup_acquisition_plan(&db, &server.api, malformed)
                .await
                .is_err()
        );
    }
    assert_eq!(server.finish().len(), 2);
}

#[tokio::test]
async fn complete_binding_changes_are_rejected_after_identity_recomputation() {
    let server = Server::new(vec![version("m"), part_reply()]);
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let trusted = planned(&db, &server.api).await;
    for mutation in 0..7 {
        let mut record = trusted.record.clone();
        match mutation {
            0 => record.bindings[0].position = 1,
            1 => record.bindings[0].plan_local_object_identity = "0".repeat(64),
            2 => record.bindings[0].configured_leaf.part_id = "other".to_owned(),
            3 => record.bindings[0].configuration_request_value = "other".to_owned(),
            4 => record.plan.objects[0].display_name = "modified".to_owned(),
            5 => record.plan.objects[0].expected_neutral_placement_matrix[3] = 7.,
            _ => record.bindings.clear(),
        }
        record.provenance_identity = record.identity().unwrap();
        assert!(
            validate_record(&db, &server.api, &record).await.is_err(),
            "mutation {mutation}"
        );
    }
    assert_eq!(server.finish().len(), 2);
}

#[tokio::test]
async fn planner_or_recording_failure_publishes_no_association() {
    for recording in [false, true] {
        let server = Server::new(vec![
            version("m"),
            if recording {
                part_reply()
            } else {
                reply(json!([]))
            },
        ]);
        let directory = scratch();
        let (_, db, pool) = disk_database(&directory).await;
        if recording {
            sqlx::query("CREATE TRIGGER fail_record BEFORE INSERT ON acquisition_plan_provenance BEGIN SELECT RAISE(ABORT, 'synthetic recording failure'); END").execute(&pool).await.unwrap();
        }
        let request = request(
            &db,
            &server.api,
            SelectionElementKind::PartStudio,
            part_request_selector(),
        )
        .await;
        assert!(
            record_planning_invocation(&db, &server.api, request)
                .await
                .is_err()
        );
        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM acquisition_plan_provenance")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(rows, 0);
        assert_eq!(server.finish().len(), 2);
    }
}

#[tokio::test]
async fn identical_and_conflicting_association_races_preserve_first_record() {
    let server = Server::new(vec![version("m"), part_reply()]);
    let directory = scratch();
    let (_, db, pool) = disk_database(&directory).await;
    let trusted = planned(&db, &server.api).await;
    let plan_id = &trusted.plan().plan_identity;
    let provenance_id = &trusted.record.provenance_identity;
    let canonical = String::from_utf8(bounded_canonical(&trusted.record).unwrap()).unwrap();
    sqlx::query("DELETE FROM acquisition_plan_provenance")
        .execute(&pool)
        .await
        .unwrap();
    let (first, second) = tokio::join!(
        db.record_acquisition_provenance(plan_id, provenance_id, &canonical),
        db.record_acquisition_provenance(plan_id, provenance_id, &canonical)
    );
    first.unwrap();
    second.unwrap();
    let before = db.acquisition_provenance(plan_id).await.unwrap().unwrap();
    let different = "0".repeat(64);
    let (same, conflict) = tokio::join!(
        db.record_acquisition_provenance(plan_id, provenance_id, &canonical),
        db.record_acquisition_provenance(plan_id, &different, "{}")
    );
    same.unwrap();
    assert!(conflict.is_err());
    assert_eq!(
        db.acquisition_provenance(plan_id).await.unwrap().unwrap(),
        before
    );
    lookup_acquisition_plan(&db, &server.api, &trusted.lookup_request().unwrap())
        .await
        .unwrap();
    assert_eq!(server.finish().len(), 2);
}

fn assembly_replies() -> Vec<Vec<u8>> {
    fn instance(id: &str, element: &str, part: &str, configuration: &str) -> Value {
        json!({"id":id,"type":"Part","suppressed":false,"documentId":"d","documentMicroversion":"m",
            "elementId":element,"fullConfiguration":configuration,"partId":part,"bodyType":"solid"})
    }
    fn placed(id: &str, x: f64) -> Value {
        let mut matrix = onshape_selection::IDENTITY_PLACEMENT;
        matrix[3] = x;
        json!({"path":[id],"hidden":false,"transform":matrix})
    }
    vec![
        version("m"),
        reply(json!({
            "rootAssembly":{"documentId":"d","documentMicroversion":"m","elementId":"e","fullConfiguration":"root-assembly-response",
                "instances":[instance("i-a2","leaf-a","p-a","leaf+A%25"),instance("i-b","leaf-b","p-b","leaf=B;value"),instance("i-a1","leaf-a","p-a","leaf+A%25")],
                "occurrences":[placed("i-a2",3.),placed("i-b",2.),placed("i-a1",1.)],"parametricInstances":[],"patterns":[]},
            "parts":[
                {"documentId":"d","documentMicroversion":"m","elementId":"leaf-a","fullConfiguration":"leaf+A%25","partId":"p-a","bodyType":"solid"},
                {"documentId":"d","documentMicroversion":"m","elementId":"leaf-b","fullConfiguration":"leaf=B;value","partId":"p-b","bodyType":"solid"}
            ],"subAssemblies":[],"partStudioFeatures":[]
        })),
        reply(json!([metadata(
            "leaf-a",
            "p-a",
            "carrier-config-a",
            "Shared [onshape-export:v1;role=printable;key=a;targets=]"
        )])),
        reply(json!([metadata(
            "leaf-b",
            "p-b",
            "carrier-config-b",
            "Other [onshape-export:v1;role=printable;key=b;targets=]"
        )])),
    ]
}

async fn planned_assembly(db: &Database, api: &OnshapeApi) -> TrustedAcquisitionPlan {
    let selectors = ["i-a1", "i-b", "i-a2"]
        .map(|id| SelectionSelector::Occurrence {
            occurrence_path: vec![id.to_owned()],
        })
        .to_vec();
    let request = request(db, api, SelectionElementKind::Assembly, selectors).await;
    record_planning_invocation(db, api, request).await.unwrap()
}

#[tokio::test]
async fn occurrence_order_annotations_placements_and_leaf_identity_control_reuse() {
    let mut replies = assembly_replies();
    replies.extend([
        version("m"),
        done("t-a", "x-a"),
        bytes(),
        done("t-b", "x-b"),
        bytes(),
    ]);
    let server = Server::new(replies);
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let trusted = planned_assembly(&db, &server.api).await;
    let directory = scratch();
    let mut acquired = acquire_geometry(&db, &server.api, &trusted, directory.path())
        .await
        .unwrap();
    assert_eq!(acquired.plan, *trusted.plan());
    assert_eq!(acquired.bindings.len(), 3);
    for (position, (binding, object)) in acquired
        .bindings
        .iter()
        .zip(&trusted.plan().objects)
        .enumerate()
    {
        assert_eq!(binding.position, position);
        assert_eq!(
            binding.plan_local_object_identity,
            object.plan_local_object_identity
        );
        assert_eq!(binding.configured_leaf, object.configured_leaf);
        assert_eq!(
            object.expected_neutral_placement_matrix[3],
            (position + 1) as f64
        );
    }
    assert_eq!(
        acquired.plan.objects[0].annotation.key.as_deref(),
        Some("a")
    );
    assert_eq!(
        acquired.plan.objects[1].annotation.key.as_deref(),
        Some("b")
    );
    assert!(Arc::ptr_eq(
        &acquired.bindings[0].retained_payload.storage_reference,
        &acquired.bindings[2].retained_payload.storage_reference
    ));
    assert!(!Arc::ptr_eq(
        &acquired.bindings[0].retained_payload.storage_reference,
        &acquired.bindings[1].retained_payload.storage_reference
    ));
    assert_eq!(
        acquired.bindings[0].retained_payload.sha256, acquired.bindings[1].retained_payload.sha256,
        "equal bytes remain distinct leaves"
    );
    assert_ne!(
        acquired.bindings[0].plan_local_object_identity,
        acquired.bindings[2].plan_local_object_identity
    );
    validate_acquired(&trusted.record, &acquired.bindings).unwrap();
    acquired.bindings.swap(0, 1);
    assert!(validate_acquired(&trusted.record, &acquired.bindings).is_err());
    acquired.bindings.swap(0, 1);
    acquired.bindings[1]
        .acquisition_evidence
        .terminal_translation_id = "swapped".to_owned();
    assert!(validate_acquired(&trusted.record, &acquired.bindings).is_err());
    let calls = server.finish();
    assert_eq!(calls.len(), 9);
    for (index, element, part, configuration) in [
        (5, "leaf-a", "p-a", "leaf+A%25"),
        (7, "leaf-b", "p-b", "leaf=B;value"),
    ] {
        assert_eq!(calls[index].method, "POST");
        assert_eq!(
            calls[index].target,
            format!("/api/v16/partstudios/d/d/v/v/e/{element}/translations")
        );
        let body: Value = serde_json::from_slice(&calls[index].body).unwrap();
        assert_eq!(body["configuration"], configuration);
        assert_eq!(body["partIds"], part);
    }
    assert_eq!(calls[6].target, "/api/v16/documents/d/d/externaldata/x-a");
    assert_eq!(calls[8].target, "/api/v16/documents/d/d/externaldata/x-b");
}

#[tokio::test]
async fn later_leaf_failure_discards_all_previous_retention() {
    let mut replies = assembly_replies();
    replies.extend([
        version("m"),
        done("t-a", "x-a"),
        bytes(),
        reply(json!({"id":"t-b","requestState":"FAILED"})),
    ]);
    let server = Server::new(replies);
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let trusted = planned_assembly(&db, &server.api).await;
    let directory = scratch();
    let failure = acquire_geometry(&db, &server.api, &trusted, directory.path())
        .await
        .err()
        .unwrap();
    assert_eq!(failure.kind, FailureKind::OperationalTranslationFailure);
    assert!(
        std::fs::read_dir(directory.path())
            .unwrap()
            .next()
            .is_none()
    );
    assert_eq!(server.finish().len(), 8);
}

#[tokio::test]
async fn invalid_done_result_never_downloads_any_geometry() {
    for result in [
        json!({}),
        json!({"resultExternalDataIds":[]}),
        json!({"resultExternalDataIds":["a","b"]}),
        json!({"resultExternalDataIds":[null]}),
        json!({"resultExternalDataIds":["a"],"resultElementIds":["element"]}),
        json!({"documentId":"wrong","resultExternalDataIds":["a"]}),
    ] {
        let mut value = json!({"id":"t","requestState":"DONE"});
        value
            .as_object_mut()
            .unwrap()
            .extend(result.as_object().unwrap().clone());
        let server = Server::new(vec![version("m"), part_reply(), version("m"), reply(value)]);
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let trusted = planned(&db, &server.api).await;
        let directory = scratch();
        assert!(
            acquire_geometry(&db, &server.api, &trusted, directory.path())
                .await
                .is_err()
        );
        assert_eq!(server.finish().len(), 4);
    }
}

#[tokio::test]
async fn plan_translation_leaf_and_poll_count_bounds_fail_closed() {
    for mode in 0..4 {
        let mut replies = vec![version("m"), part_reply(), version("m")];
        if mode == 2 {
            replies.push(done("t", "e"));
        } else {
            replies.extend((0..3).map(|_| reply(json!({"id":"t","requestState":"ACTIVE"}))));
        }
        let server = Server::new(replies);
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let trusted = planned(&db, &server.api).await;
        let directory = scratch();
        let mut timing = fast_timing();
        match mode {
            0 => timing.plan = Duration::ZERO,
            1 => {
                timing.translation = Duration::from_millis(250);
                timing.poll_intervals = [Duration::from_secs(1); 5];
            }
            2 => timing.leaf = Duration::ZERO,
            _ => timing.max_polls = 2,
        }
        let failure = acquire_with_timing(&db, &server.api, &trusted, directory.path(), timing)
            .await
            .err()
            .unwrap();
        assert_eq!(
            failure.kind,
            FailureKind::OperationalTimeoutFailure,
            "mode {mode}"
        );
        let calls = server.finish();
        assert_eq!(calls.len(), [2, 4, 4, 6][mode], "mode {mode}");
        assert!(
            calls
                .iter()
                .all(|call| !call.target.contains("externaldata"))
        );
    }
}

#[tokio::test]
async fn producer_replay_reuses_record_and_conflicting_same_plan_is_not_overwritten() {
    let server = Server::new(vec![
        version("m"),
        part_reply(),
        version("m"),
        part_reply(),
        version("m"),
        part_reply(),
    ]);
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let original_request = request(
        &db,
        &server.api,
        SelectionElementKind::PartStudio,
        part_request_selector(),
    )
    .await;
    let trusted = record_planning_invocation(&db, &server.api, original_request.clone())
        .await
        .unwrap();
    let same = record_planning_invocation(&db, &server.api, original_request.clone())
        .await
        .unwrap();
    assert_eq!(
        bounded_canonical(&same.record).unwrap(),
        bounded_canonical(&trusted.record).unwrap()
    );
    let alternate = configuration_encoding::seed(
        &db,
        &server.api,
        &source(trusted.plan()),
        &BTreeMap::from([(
            "option".to_owned(),
            CanonicalParameterValue::Boolean { value: true },
        )]),
        "alternate-encoding",
        "configuration=alternate-encoding",
    )
    .await;
    let alternate_request = SelectionRequest {
        configuration_encoding: alternate,
        ..original_request
    };
    assert!(
        record_planning_invocation(&db, &server.api, alternate_request)
            .await
            .is_err()
    );
    let reopened = lookup_acquisition_plan(&db, &server.api, &trusted.lookup_request().unwrap())
        .await
        .unwrap();
    assert_eq!(
        bounded_canonical(&reopened.record).unwrap(),
        bounded_canonical(&trusted.record).unwrap()
    );
    let calls = server.finish();
    assert_eq!(calls.len(), 6);
    assert!(calls.iter().all(|call| call.method == "GET"));
}

#[tokio::test]
async fn supplied_complete_pair_is_lookup_only_and_closed_shape() {
    let server = Server::new(vec![version("m"), part_reply()]);
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let trusted = planned(&db, &server.api).await;
    let lookup = ProvenanceLookup {
        provenance_schema_version: 1,
        plan_identity: trusted.plan().plan_identity.clone(),
        provenance_identity: trusted.record.provenance_identity.clone(),
        plan: Some(trusted.plan().clone()),
        record: Some(trusted.record.clone()),
    };
    lookup_acquisition_plan(&db, &server.api, &bounded_canonical(&lookup).unwrap())
        .await
        .unwrap();
    let valid = serde_json::to_value(&lookup).unwrap();
    for (field, value) in [
        ("plan", Value::Null),
        ("record", Value::Null),
        ("unrecognized", json!(true)),
        ("provenanceSchemaVersion", json!(2)),
    ] {
        let mut malformed = valid.clone();
        malformed[field] = value;
        assert!(
            lookup_acquisition_plan(&db, &server.api, &serde_json::to_vec(&malformed).unwrap())
                .await
                .is_err()
        );
    }
    let unassociated = Database::connect("sqlite::memory:").await.unwrap();
    assert!(
        lookup_acquisition_plan(
            &unassociated,
            &server.api,
            &bounded_canonical(&lookup).unwrap()
        )
        .await
        .is_err()
    );
    assert!(
        unassociated
            .acquisition_provenance(&trusted.plan().plan_identity)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(server.finish().len(), 2);
}

#[tokio::test]
async fn public_producer_rejects_invalid_selection_before_coordinator_requests() {
    let server = Server::new(Vec::new());
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let root = OnshapeSource {
        document_id: "d".to_owned(),
        version_id: "v".to_owned(),
        element_id: "e".to_owned(),
        element_kind: ElementKind::PartStudio,
        link_document_id: None,
    };
    assert!(
        plan_for_acquisition(&db, &server.api, &root, &BTreeMap::new(), Vec::new())
            .await
            .is_err()
    );
    assert!(
        plan_for_acquisition(
            &db,
            &server.api,
            &root,
            &BTreeMap::new(),
            vec![SelectionSelector::Occurrence {
                occurrence_path: vec!["i".to_owned()]
            }]
        )
        .await
        .is_err()
    );
    assert!(server.finish().is_empty());
}

#[tokio::test]
async fn poll_id_or_echo_swap_rejects_terminal_download() {
    for terminal in [
        json!({"id":"other","requestState":"DONE","resultExternalDataIds":["external"]}),
        json!({"id":"t","requestState":"DONE","versionId":"wrong","resultExternalDataIds":["external"]}),
    ] {
        let server = Server::new(vec![
            version("m"),
            part_reply(),
            version("m"),
            reply(json!({"id":"t","requestState":"ACTIVE"})),
            reply(terminal),
        ]);
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let trusted = planned(&db, &server.api).await;
        let directory = scratch();
        assert!(
            acquire_with_timing(&db, &server.api, &trusted, directory.path(), fast_timing())
                .await
                .is_err()
        );
        let calls = server.finish();
        assert_eq!(calls.len(), 5);
        assert_eq!(calls[4].target, "/api/v16/translations/t");
    }
}

#[tokio::test]
async fn failed_transaction_commit_publishes_neither_record_nor_probe() {
    let server = Server::new(vec![version("m"), part_reply()]);
    let directory = scratch();
    let (_, db, pool) = disk_database(&directory).await;
    let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(foreign_keys, 1);
    sqlx::query("CREATE TABLE commit_parent (id TEXT PRIMARY KEY)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("CREATE TABLE commit_probe (id TEXT REFERENCES commit_parent(id) DEFERRABLE INITIALLY DEFERRED)").execute(&pool).await.unwrap();
    sqlx::query("CREATE TRIGGER fail_commit AFTER INSERT ON acquisition_plan_provenance BEGIN INSERT INTO commit_probe VALUES ('missing'); END").execute(&pool).await.unwrap();
    let request = request(
        &db,
        &server.api,
        SelectionElementKind::PartStudio,
        part_request_selector(),
    )
    .await;
    assert!(
        record_planning_invocation(&db, &server.api, request)
            .await
            .is_err()
    );
    let provenance_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM acquisition_plan_provenance")
            .fetch_one(&pool)
            .await
            .unwrap();
    let probe_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM commit_probe")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(provenance_rows, 0);
    assert_eq!(probe_rows, 0);
    assert_eq!(server.finish().len(), 2);
}

#[tokio::test]
async fn supplied_negative_zero_matrix_and_omitted_context_field_are_rejected() {
    let server = Server::new(vec![version("m"), part_reply()]);
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let trusted = planned(&db, &server.api).await;
    let lookup = ProvenanceLookup {
        provenance_schema_version: 1,
        plan_identity: trusted.plan().plan_identity.clone(),
        provenance_identity: trusted.record.provenance_identity.clone(),
        plan: Some(trusted.plan().clone()),
        record: Some(trusted.record.clone()),
    };
    let mut negative_zero = serde_json::to_value(&lookup).unwrap();
    negative_zero["plan"]["objects"][0]["expectedNeutralPlacementMatrix"][1] = json!(-0.0);
    assert!(
        lookup_acquisition_plan(
            &db,
            &server.api,
            &serde_json::to_vec(&negative_zero).unwrap()
        )
        .await
        .is_err()
    );
    let mut absent_context = serde_json::to_value(&lookup).unwrap();
    absent_context["record"]["encodingContext"]
        .as_object_mut()
        .unwrap()
        .remove("linkDocumentId");
    assert!(
        lookup_acquisition_plan(
            &db,
            &server.api,
            &serde_json::to_vec(&absent_context).unwrap()
        )
        .await
        .is_err()
    );
    assert_eq!(server.finish().len(), 2);
}

#[tokio::test]
async fn public_producer_keeps_cached_coordinator_handoff_through_same_invocation() {
    let server = Server::new(vec![version("m"), version("m"), part_reply()]);
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let seeded = request(
        &db,
        &server.api,
        SelectionElementKind::PartStudio,
        part_request_selector(),
    )
    .await;
    let root = OnshapeSource {
        document_id: "d".to_owned(),
        version_id: "v".to_owned(),
        element_id: "e".to_owned(),
        element_kind: ElementKind::PartStudio,
        link_document_id: None,
    };
    let trusted = plan_for_acquisition(
        &db,
        &server.api,
        &root,
        &BTreeMap::new(),
        part_request_selector(),
    )
    .await
    .unwrap();
    assert_eq!(
        trusted.record.configuration_encoding,
        seeded.configuration_encoding
    );
    assert_eq!(
        trusted.record.bindings[0].configuration_request_value,
        ENCODING
    );
    lookup_acquisition_plan(&db, &server.api, &trusted.lookup_request().unwrap())
        .await
        .unwrap();
    let calls = server.finish();
    assert_eq!(calls.len(), 3);
    for call in &calls[..2] {
        assert_eq!(call.method, "GET");
        assert_eq!(
            call.target,
            "/api/v16/documents/d/d/versions/v?parents=false"
        );
    }
    let parts_url =
        reqwest::Url::parse(&format!("https://cad.onshape.com{}", calls[2].target)).unwrap();
    assert_eq!(parts_url.path(), "/api/v16/parts/d/d/m/m/e/e");
    assert!(
        parts_url
            .query_pairs()
            .any(|(key, value)| key == "configuration" && value == ENCODING)
    );
    assert!(
        calls.iter().all(|call| call.method == "GET"),
        "cached coordinator must not encode"
    );
}

#[tokio::test]
async fn cancellation_while_recording_is_blocked_publishes_no_association() {
    let server = Server::new(vec![version("m"), part_reply()]);
    let directory = scratch();
    let (_, db, pool) = disk_database(&directory).await;
    let seeded = request(
        &db,
        &server.api,
        SelectionElementKind::PartStudio,
        part_request_selector(),
    )
    .await;
    let mut lock = pool.acquire().await.unwrap();
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *lock)
        .await
        .unwrap();
    let task_db = db.clone();
    let task_api = server.api.clone();
    let task =
        tokio::spawn(async move { record_planning_invocation(&task_db, &task_api, seeded).await });
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if server.requests.lock().unwrap().len() == 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    // Metadata has been served, and the sole outstanding write cannot acquire
    // SQLite's reserved writer lock. Cancellation drops its transaction.
    tokio::time::sleep(Duration::from_millis(25)).await;
    assert!(!task.is_finished());
    task.abort();
    assert!(task.await.err().unwrap().is_cancelled());
    sqlx::query("ROLLBACK").execute(&mut *lock).await.unwrap();
    drop(lock);
    let rows: i64 = tokio::time::timeout(
        Duration::from_secs(2),
        sqlx::query_scalar("SELECT COUNT(*) FROM acquisition_plan_provenance").fetch_one(&pool),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(rows, 0);
    assert_eq!(server.finish().len(), 2);
}
