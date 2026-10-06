//! Synthetic HTTP-boundary tests for immutable planning; no live Onshape data.

use std::{
    collections::{BTreeMap, VecDeque},
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use serde_json::json;

use super::*;

const ROOT_ENCODING: &str = "root+encoded%value";
const LEAF_A_CONFIGURATION: &str = "leaf+A%25";
const LEAF_B_CONFIGURATION: &str = "leaf=B;value";

/// Closing each response socket makes every operation observable separately.
/// Unexpected calls receive a failure response and remain in the request log.
struct ScriptedServer {
    api: OnshapeApi,
    requests: Arc<Mutex<Vec<String>>>,
    stopped: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl ScriptedServer {
    fn new(replies: Vec<Vec<u8>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let api = OnshapeApi::for_test(&format!("http://{}", listener.local_addr().unwrap()));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stopped = Arc::new(AtomicBool::new(false));
        let recorded = Arc::clone(&requests);
        let finished = Arc::clone(&stopped);
        let thread = thread::spawn(move || {
            let mut replies: VecDeque<_> = replies.into();
            while !finished.load(Ordering::Acquire) {
                let (mut socket, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                        continue;
                    }
                    Err(error) => panic!("synthetic listener failed: {error}"),
                };
                socket
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 1_024];
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let count = socket.read(&mut buffer).unwrap();
                    assert!(count > 0, "synthetic request ended before its headers");
                    request.extend_from_slice(&buffer[..count]);
                    assert!(request.len() <= 65_536);
                }
                let request = String::from_utf8(request).unwrap();
                recorded
                    .lock()
                    .unwrap()
                    .push(request.lines().next().unwrap().to_owned());
                let response = replies.pop_front().unwrap_or_else(|| {
                    b"HTTP/1.1 500 Unexpected operation\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
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

    fn finish(mut self) -> Vec<String> {
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

impl Drop for ScriptedServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn reply(value: Value) -> Vec<u8> {
    let body = serde_json::to_vec(&value).unwrap();
    let mut response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    response.extend(body);
    response
}

fn version_reply(microversion: &str) -> Vec<u8> {
    reply(json!({"documentId":"d","id":"v","microversion":microversion}))
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
        element_kind: kind.catalog_kind(),
        link_document_id: None,
    };
    let handoff = configuration_encoding::seed(
        db,
        api,
        &source,
        &BTreeMap::new(),
        ROOT_ENCODING,
        "configuration=root%2Bencoded%25value",
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

fn part(part_id: &str) -> SelectionSelector {
    SelectionSelector::Part {
        part_id: part_id.to_owned(),
    }
}

fn occurrence(instance_id: &str) -> SelectionSelector {
    SelectionSelector::Occurrence {
        occurrence_path: vec![instance_id.to_owned()],
    }
}

fn metadata(element: &str, part_id: &str, configuration: &str, name: &str) -> Value {
    json!({
        "elementId":element,"microversionId":"m","partId":part_id,
        "configurationId":configuration,"bodyType":"solid",
        "isFlattenedBody":false,"isMesh":false,"meshState":"NO_MESH",
        "isHidden":false,"name":name,"description":"",
        "futureField":{"unconsumed":[true,null,{"additive":"value"}]}
    })
}

fn configured_part(element: &str, part_id: &str, configuration: &str) -> Value {
    json!({
        "documentId":"d","documentMicroversion":"m","elementId":element,
        "fullConfiguration":configuration,"partId":part_id,"bodyType":"solid"
    })
}

fn instance(instance_id: &str, element: &str, part_id: &str, configuration: &str) -> Value {
    let mut instance = configured_part(element, part_id, configuration);
    instance["id"] = json!(instance_id);
    instance["type"] = json!("Part");
    instance["suppressed"] = json!(false);
    instance
}

fn placed_occurrence(instance_id: &str, translation: f64) -> Value {
    let mut transform = IDENTITY_PLACEMENT;
    transform[3] = translation;
    transform[7] = -0.0;
    json!({"path":[instance_id],"hidden":false,"transform":transform})
}

fn assembly() -> Value {
    json!({
        "rootAssembly": {
            "documentId":"d","documentMicroversion":"m","elementId":"e",
            "fullConfiguration":"root-response-identity",
            "instances":[
                instance("i-b","leaf-b","p-b",LEAF_B_CONFIGURATION),
                instance("i-a2","leaf-a","p-a",LEAF_A_CONFIGURATION),
                instance("i-a1","leaf-a","p-a",LEAF_A_CONFIGURATION)
            ],
            "occurrences":[
                placed_occurrence("i-a2",3.0),
                placed_occurrence("i-b",2.0),
                placed_occurrence("i-a1",1.0)
            ],
            "parametricInstances":[],"patterns":[]
        },
        "parts":[
            configured_part("leaf-b","p-b",LEAF_B_CONFIGURATION),
            configured_part("leaf-a","p-a",LEAF_A_CONFIGURATION)
        ],
        "subAssemblies":[{"unconsumed":true}],
        "partStudioFeatures":[null]
    })
}

fn carrier_a() -> Value {
    json!([
        metadata("leaf-a", "p-a", "carrier-response-a", "Shared part"),
        {"elementId":"leaf-a","microversionId":"m","partId":"unselected"}
    ])
}

fn carrier_b() -> Value {
    json!([metadata(
        "leaf-b",
        "p-b",
        "carrier-response-b",
        "Other part"
    )])
}

fn request_url(line: &str) -> reqwest::Url {
    let fields: Vec<_> = line.split_whitespace().collect();
    assert_eq!(fields[0], "GET");
    assert_eq!(fields[2], "HTTP/1.1");
    reqwest::Url::parse(&format!("https://cad.onshape.com{}", fields[1])).unwrap()
}

fn assert_query(line: &str, path: &str, expected: &[(&str, &str)]) {
    let url = request_url(line);
    assert_eq!(url.path(), path);
    let actual: BTreeMap<_, _> = url.query_pairs().into_owned().collect();
    let expected: BTreeMap<_, _> = expected
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect();
    assert_eq!(actual.len(), url.query_pairs().count(), "duplicate query");
    assert_eq!(actual, expected);
}

fn assert_version_call(line: &str) {
    assert_query(
        line,
        "/api/v16/documents/d/d/versions/v",
        &[("parents", "false")],
    );
}

fn assert_parts_call(line: &str, element: &str, configuration: &str) {
    assert_query(
        line,
        &format!("/api/v16/parts/d/d/m/m/e/{element}"),
        &[
            ("configuration", configuration),
            ("withThumbnails", "false"),
            ("includePropertyDefaults", "false"),
            ("includeFlatParts", "false"),
        ],
    );
}

fn assert_assembly_call(line: &str) {
    assert_query(
        line,
        "/api/v16/assemblies/d/d/m/m/e/e",
        &[
            ("configuration", ROOT_ENCODING),
            ("includeMateFeatures", "false"),
            ("includeNonSolids", "false"),
            ("includeMateConnectors", "false"),
            ("excludeSuppressed", "false"),
        ],
    );
}

#[tokio::test]
async fn part_studio_plan_preserves_caller_order_and_response_configuration_identity() {
    let server = ScriptedServer::new(vec![
        version_reply("m"),
        reply(json!([
            metadata("e", "p-a", "part-response-identity", "Alpha"),
            metadata("e", "p-b", "part-response-identity", "Beta")
        ])),
    ]);
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let request = request(
        &db,
        &server.api,
        SelectionElementKind::PartStudio,
        vec![part("p-b"), part("p-a")],
    )
    .await;
    let plan = plan_selection(&db, &server.api, &request).await.unwrap();
    assert_eq!(plan.root.version_id, "v");
    assert_eq!(plan.root.document_microversion, "m");
    assert_eq!(plan.root.configuration_identity, "part-response-identity");
    assert_eq!(plan.objects[0].configured_leaf.part_id, "p-b");
    assert_eq!(plan.objects[0].display_name, "Beta");
    assert_eq!(plan.objects[1].configured_leaf.part_id, "p-a");
    for object in &plan.objects {
        assert_eq!(
            object.configured_leaf.configuration_identity,
            "part-response-identity"
        );
        assert_eq!(object.expected_neutral_placement_matrix, IDENTITY_PLACEMENT);
        assert!(is_sha256(&object.plan_local_object_identity));
    }
    assert!(is_sha256(&plan.plan_identity));
    let serialized = serde_json::to_string(&plan).unwrap();
    assert!(!serialized.contains(ROOT_ENCODING));
    assert!(!serialized.contains("encodingContextHash"));
    let calls = server.finish();
    assert_eq!(calls.len(), 2);
    assert_version_call(&calls[0]);
    assert_parts_call(&calls[1], "e", ROOT_ENCODING);
}

#[tokio::test]
async fn assembly_plan_uses_exact_configured_carriers_once_in_first_selector_order() {
    let server = ScriptedServer::new(vec![
        version_reply("m"),
        reply(assembly()),
        reply(carrier_a()),
        reply(carrier_b()),
    ]);
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let request = request(
        &db,
        &server.api,
        SelectionElementKind::Assembly,
        vec![occurrence("i-a1"), occurrence("i-b"), occurrence("i-a2")],
    )
    .await;
    let plan = plan_selection(&db, &server.api, &request).await.unwrap();
    assert_eq!(plan.root.configuration_identity, "root-response-identity");
    assert_eq!(plan.objects.len(), 3);
    assert_eq!(
        plan.objects[0].configured_leaf,
        plan.objects[2].configured_leaf
    );
    assert_ne!(
        plan.objects[0].plan_local_object_identity,
        plan.objects[2].plan_local_object_identity
    );
    for (position, object) in plan.objects.iter().enumerate() {
        assert_eq!(
            object.expected_neutral_placement_matrix[3],
            (position + 1) as f64
        );
        assert_eq!(object.expected_neutral_placement_matrix[7].to_bits(), 0);
        let AuthoringSelector::AssemblyOccurrence {
            configuration_identity,
            ..
        } = &object.selector
        else {
            panic!("unexpected planned selector");
        };
        assert_eq!(configuration_identity, "root-response-identity");
    }
    assert_eq!(
        plan.objects[0].configured_leaf.configuration_identity,
        LEAF_A_CONFIGURATION
    );
    assert_eq!(
        plan.objects[1].configured_leaf.configuration_identity,
        LEAF_B_CONFIGURATION
    );
    assert_eq!(plan.objects[0].display_name, "Shared part");
    assert_eq!(plan.objects[2].display_name, "Shared part");
    let calls = server.finish();
    assert_eq!(calls.len(), 4);
    assert_version_call(&calls[0]);
    assert_assembly_call(&calls[1]);
    assert_parts_call(&calls[2], "leaf-a", LEAF_A_CONFIGURATION);
    assert_parts_call(&calls[3], "leaf-b", LEAF_B_CONFIGURATION);
    assert!(calls[2].contains("configuration=leaf%2BA%2525"));
}

#[tokio::test]
async fn invalid_preflight_never_contacts_onshape() {
    let server = ScriptedServer::new(Vec::new());
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let valid = request(
        &db,
        &server.api,
        SelectionElementKind::PartStudio,
        vec![part("p")],
    )
    .await;
    for selectors in [
        Vec::new(),
        vec![part("p"), part("p")],
        vec![occurrence("i")],
        vec![part("")],
    ] {
        let mut invalid = valid.clone();
        invalid.selectors = selectors;
        assert_eq!(
            plan_selection(&db, &server.api, &invalid)
                .await
                .unwrap_err()
                .kind,
            FailureKind::InvalidSelection
        );
    }
    let mut malformed = serde_json::to_value(&valid).unwrap();
    malformed["configurationEncoding"]["queryParam"] = json!("forbidden");
    assert_eq!(
        SelectionRequest::from_json(&serde_json::to_vec(&malformed).unwrap())
            .unwrap_err()
            .kind,
        FailureKind::InvalidSelection
    );
    assert!(server.finish().is_empty());
}

#[tokio::test]
async fn forged_or_stale_handoff_stops_after_independent_version_read() {
    for case in 0..5 {
        let server = ScriptedServer::new(vec![version_reply(if case == 4 {
            "changed-m"
        } else {
            "m"
        })]);
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let mut request = request(
            &db,
            &server.api,
            SelectionElementKind::PartStudio,
            vec![part("p")],
        )
        .await;
        match case {
            0 => request.configuration_encoding.source_hash = "a".repeat(64),
            1 => request.configuration_encoding.config_hash = "b".repeat(64),
            2 => request.configuration_encoding.encoding_context_hash = "c".repeat(64),
            3 => request.configuration_encoding.encoded_id = "forged".to_owned(),
            _ => {}
        }
        let failure = plan_selection(&db, &server.api, &request)
            .await
            .unwrap_err();
        assert_eq!(failure.kind, FailureKind::OperationalApiContractFailure);
        let calls = server.finish();
        assert_eq!(calls.len(), 1);
        assert_version_call(&calls[0]);
    }
}

#[tokio::test]
async fn unsupported_selected_state_stops_before_later_projection_and_carriers() {
    for case in 0..3 {
        let mut response = assembly();
        match case {
            0 => {
                response["rootAssembly"]["parametricInstances"] = json!([
                    {"id":"parametric","children":[{"instanceIds":["i-a1"]}]}
                ]);
                // Generated-state rejection precedes the type requirement.
                response["rootAssembly"]["instances"][2]
                    .as_object_mut()
                    .unwrap()
                    .remove("type");
            }
            1 => {
                response["rootAssembly"]["occurrences"][2]["hidden"] = json!(true);
                response["rootAssembly"]["occurrences"][2]
                    .as_object_mut()
                    .unwrap()
                    .remove("transform");
                response["rootAssembly"]["instances"][2]
                    .as_object_mut()
                    .unwrap()
                    .remove("documentId");
            }
            _ => {
                response["rootAssembly"]["instances"][2]["documentId"] = json!("external-document");
                // Cross-source rejection precedes projection of Assembly parts.
                response["parts"] = json!([null]);
            }
        }
        let server = ScriptedServer::new(vec![version_reply("m"), reply(response)]);
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let request = request(
            &db,
            &server.api,
            SelectionElementKind::Assembly,
            vec![occurrence("i-a1")],
        )
        .await;
        let failure = plan_selection(&db, &server.api, &request)
            .await
            .unwrap_err();
        assert_eq!(failure.kind, FailureKind::UnavailableSourceState);
        assert_eq!(failure.diagnostics[0].selector_position, Some(0));
        let calls = server.finish();
        assert_eq!(calls.len(), 2);
        assert_version_call(&calls[0]);
        assert_assembly_call(&calls[1]);
    }
}

#[tokio::test]
async fn first_carrier_authentication_failure_stops_before_the_next_carrier() {
    let server = ScriptedServer::new(vec![
        version_reply("m"),
        reply(assembly()),
        b"HTTP/1.1 401 Unauthorized\r\nContent-Type: application/octet-stream\r\nContent-Length: 3\r\nConnection: close\r\n\r\n\xff\xfe\xfd".to_vec(),
    ]);
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let request = request(
        &db,
        &server.api,
        SelectionElementKind::Assembly,
        vec![occurrence("i-a1"), occurrence("i-b")],
    )
    .await;
    let failure = plan_selection(&db, &server.api, &request)
        .await
        .unwrap_err();
    assert_eq!(failure.kind, FailureKind::AuthenticationFailure);
    assert_eq!(failure.diagnostics[0].selector_position, Some(0));
    let calls = server.finish();
    assert_eq!(calls.len(), 3);
    assert_version_call(&calls[0]);
    assert_assembly_call(&calls[1]);
    assert_parts_call(&calls[2], "leaf-a", LEAF_A_CONFIGURATION);
}

#[tokio::test]
async fn shuffled_equivalent_api_arrays_preserve_complete_plan_bytes_and_identities() {
    let mut plans = Vec::new();
    for shuffled in [false, true] {
        let mut definition = assembly();
        let mut first_carrier = carrier_a();
        if shuffled {
            for field in ["instances", "occurrences"] {
                definition["rootAssembly"][field]
                    .as_array_mut()
                    .unwrap()
                    .reverse();
            }
            definition["parts"].as_array_mut().unwrap().reverse();
            first_carrier.as_array_mut().unwrap().reverse();
        }
        let server = ScriptedServer::new(vec![
            version_reply("m"),
            reply(definition),
            reply(first_carrier),
            reply(carrier_b()),
        ]);
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let request = request(
            &db,
            &server.api,
            SelectionElementKind::Assembly,
            vec![occurrence("i-a1"), occurrence("i-b"), occurrence("i-a2")],
        )
        .await;
        plans.push(plan_selection(&db, &server.api, &request).await.unwrap());
        let calls = server.finish();
        assert_eq!(calls.len(), 4);
        assert_parts_call(&calls[2], "leaf-a", LEAF_A_CONFIGURATION);
        assert_parts_call(&calls[3], "leaf-b", LEAF_B_CONFIGURATION);
    }
    assert_eq!(plans[0], plans[1]);
    assert_eq!(
        cache_key::canonical_json_bytes(&plans[0]).unwrap(),
        cache_key::canonical_json_bytes(&plans[1]).unwrap()
    );
}
