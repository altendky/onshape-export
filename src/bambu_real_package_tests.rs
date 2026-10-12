//! Opt-in black-box checks of the exact released CLI through product dispatch.
//! Run in a dedicated process with BAMBU_REAL_GENERATOR_CONFIG and
//! TRUSTED_GENERATOR_CONFIG_PATH naming the same reviewed installation document.
//! Output archives stay opaque. Geometry below is independently authored from
//! the 3MF Consortium Core specification, never a slicer or generator fixture:
//! https://github.com/3MFConsortium/spec_core/blob/master/3MF%20Core%20Specification.md

use std::{
    collections::{HashMap, HashSet, VecDeque},
    io::{Cursor, Write},
    path::PathBuf,
    sync::{Arc, Mutex},
};

use serde_json::{Value, json};

use crate::{
    bambu_project,
    bambu_project::tests::{FreshApiReply, FreshApiScript, RuntimeFixture, fresh_api, fresh_json},
    catalog::{self, ElementKind},
    deployed_generator::{DeployedGenerator, DeployedGeneratorAvailability},
    onshape_selection::SelectionSelector,
    parameters::{Parameter, ParameterKind, ParameterSchema},
};

#[derive(Clone, Copy)]
enum SourceCase {
    PartStudio,
    Assembly,
    UnsupportedAssembly,
}

/// One closed, outward-oriented tetrahedron in millimeters. Only standard core
/// model/package fields are present; names and blocker roles travel separately.
fn neutral_geometry_3mf() -> Vec<u8> {
    let model = r#"<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xml:lang="en-US" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
<resources><object id="1" type="model"><mesh><vertices>
<vertex x="0" y="0" z="0"/><vertex x="10" y="0" z="0"/>
<vertex x="0" y="10" z="0"/><vertex x="0" y="0" z="10"/>
</vertices><triangles><triangle v1="0" v2="2" v3="1"/>
<triangle v1="0" v2="1" v3="3"/><triangle v1="0" v2="3" v3="2"/>
<triangle v1="1" v2="2" v3="3"/></triangles></mesh></object></resources>
<build><item objectid="1"/></build></model>"#;
    let content_types = r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/>
</Types>"#;
    let relationships = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="model" Target="/3D/3dmodel.model" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/>
</Relationships>"#;
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, body) in [
        ("[Content_Types].xml", content_types),
        ("_rels/.rels", relationships),
        ("3D/3dmodel.model", model),
    ] {
        archive.start_file(name, options).unwrap();
        archive.write_all(body.as_bytes()).unwrap();
    }
    archive.finish().unwrap().into_inner()
}

fn real_generator() -> DeployedGenerator {
    let config = PathBuf::from(
        std::env::var_os("BAMBU_REAL_GENERATOR_CONFIG")
            .expect("set BAMBU_REAL_GENERATOR_CONFIG to the reviewed installed document"),
    );
    assert_eq!(
        Some(config.as_os_str()),
        std::env::var_os("TRUSTED_GENERATOR_CONFIG_PATH").as_deref(),
        "real tests require normal production admission in a dedicated process"
    );
    let generator = DeployedGenerator::load(&config).unwrap();
    crate::bambu_deployment::validate_availability(&DeployedGeneratorAvailability::Available(
        Box::new(generator.clone()),
    ))
    .unwrap();
    generator
}

fn version() -> FreshApiReply {
    fresh_json(
        json!({"documentId":"real-test-document","id":"real-test-version","microversion":"real-test-microversion"}),
    )
}

fn description(role: &str, key: &str, targets: &[&str]) -> String {
    format!(
        "onshape-export:v1 {}",
        json!({"role":role,"key":key,"targets":targets})
    )
}

fn metadata(
    element: &str,
    part: &str,
    configuration: &str,
    name: &str,
    description: String,
) -> Value {
    json!({"elementId":element,"microversionId":"real-test-microversion","partId":part,
        "configurationId":configuration,"bodyType":"solid","isFlattenedBody":false,
        "isMesh":false,"meshState":"NO_MESH","isHidden":false,"name":name,"description":description})
}

fn script(case: SourceCase) -> VecDeque<FreshApiReply> {
    let mut replies = VecDeque::from([
        version(),
        fresh_json(
            json!({"encodedId":"real-test-original-encoding","queryParam":"configuration=real-test-original-encoding"}),
        ),
        version(),
    ]);
    let leaf_count = match case {
        SourceCase::PartStudio => {
            replies.push_back(fresh_json(json!([
                metadata(
                    "real-test-element",
                    "part-a",
                    "configured-response",
                    "Same synthetic display",
                    description("printable", "a", &[])
                ),
                metadata(
                    "real-test-element",
                    "part-b",
                    "configured-response",
                    "Same synthetic display",
                    description("printable", "b", &[])
                ),
                metadata(
                    "real-test-element",
                    "part-block",
                    "configured-response",
                    "Synthetic blocker",
                    description("supportBlocker", "block", &["a", "b"])
                )
            ])));
            3
        }
        SourceCase::Assembly | SourceCase::UnsupportedAssembly => {
            let instance = |id: &str, element: &str, part: &str, config: &str| {
                json!({
                "id":id,"type":"Part","suppressed":false,"documentId":"real-test-document",
                "documentMicroversion":"real-test-microversion","elementId":element,
                "fullConfiguration":config,"partId":part,"bodyType":"solid"})
            };
            let occurrence = |id: &str, x: f64| {
                let mut matrix = crate::onshape_selection::IDENTITY_PLACEMENT;
                matrix[3] = x;
                if matches!(case, SourceCase::UnsupportedAssembly) {
                    matrix[0] = 2.0;
                }
                json!({"path":[id],"hidden":false,"transform":matrix})
            };
            replies.push_back(fresh_json(json!({
                "rootAssembly":{"documentId":"real-test-document","documentMicroversion":"real-test-microversion",
                    "elementId":"real-test-element","fullConfiguration":"assembly-response",
                    "instances":[instance("instance-a2","leaf-a","part-a","leaf-a-config"),instance("instance-target","leaf-target","part-target","leaf-target-config"),instance("instance-block","leaf-block","part-block","leaf-block-config"),instance("instance-a1","leaf-a","part-a","leaf-a-config")],
                    "occurrences":[occurrence("instance-a2",0.04),occurrence("instance-target",0.06),occurrence("instance-block",0.02),occurrence("instance-a1",0.0)],
                    "parametricInstances":[],"patterns":[]},
                "parts":[{"documentId":"real-test-document","documentMicroversion":"real-test-microversion","elementId":"leaf-a","fullConfiguration":"leaf-a-config","partId":"part-a","bodyType":"solid"},
                    {"documentId":"real-test-document","documentMicroversion":"real-test-microversion","elementId":"leaf-block","fullConfiguration":"leaf-block-config","partId":"part-block","bodyType":"solid"},
                    {"documentId":"real-test-document","documentMicroversion":"real-test-microversion","elementId":"leaf-target","fullConfiguration":"leaf-target-config","partId":"part-target","bodyType":"solid"}],
                "subAssemblies":[],"partStudioFeatures":[]
            })));
            replies.push_back(fresh_json(json!([metadata(
                "leaf-a",
                "part-a",
                "configured-a",
                "Repeated synthetic part",
                String::new()
            )])));
            replies.push_back(fresh_json(json!([metadata(
                "leaf-block",
                "part-block",
                "configured-block",
                "Synthetic blocker",
                description("supportBlocker", "block", &["target"])
            )])));
            replies.push_back(fresh_json(json!([metadata(
                "leaf-target",
                "part-target",
                "configured-target",
                "Unique synthetic target",
                description("printable", "target", &[])
            )])));
            3
        }
    };
    if !matches!(case, SourceCase::UnsupportedAssembly) {
        replies.push_back(version());
        let geometry = neutral_geometry_3mf();
        for index in 0..leaf_count {
            replies.push_back(fresh_json(json!({"id":format!("real-test-translation-{index}"),"requestState":"DONE","resultExternalDataIds":[format!("real-test-external-{index}")]})));
            replies.push_back(FreshApiReply {
                media: "application/octet-stream",
                bytes: geometry.clone(),
            });
        }
    }
    replies
}

async fn run_real(case: SourceCase) {
    let generator = real_generator();
    let mut fixture = RuntimeFixture::new().await;
    fixture.state.deployed_generator = Some(DeployedGeneratorAvailability::Available(Box::new(
        generator.clone(),
    )));
    fixture.model.onshape = catalog::OnshapeSource {
        document_id: "real-test-document".to_owned(),
        version_id: "real-test-version".to_owned(),
        element_id: "real-test-element".to_owned(),
        element_kind: if matches!(case, SourceCase::PartStudio) {
            ElementKind::PartStudio
        } else {
            ElementKind::Assembly
        },
        link_document_id: None,
    };
    let selectors = if matches!(case, SourceCase::PartStudio) {
        ["part-a", "part-b", "part-block"]
            .into_iter()
            .map(|id| SelectionSelector::Part {
                part_id: id.to_owned(),
            })
            .collect()
    } else {
        [
            "instance-a1",
            "instance-block",
            "instance-a2",
            "instance-target",
        ]
        .into_iter()
        .map(|id| SelectionSelector::Occurrence {
            occurrence_path: vec![id.to_owned()],
        })
        .collect()
    };
    fixture.model.exports.bambu_project = Some(catalog::BambuProjectConfig { selectors });
    fixture.model.exports.downloads = vec![
        catalog::DownloadFormat::Step,
        catalog::DownloadFormat::Stl,
        catalog::DownloadFormat::ThreeMf,
    ];
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
            microversion_id: "real-test-microversion".to_owned(),
            element_id: fixture.model.onshape.element_id.clone(),
            element_kind: fixture.model.onshape.element_kind.clone(),
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
            microversion_id: "real-test-microversion",
            element_id: &fixture.model.onshape.element_id,
            element_kind: if matches!(case, SourceCase::PartStudio) {
                "part_studio"
            } else {
                "assembly"
            },
            link_document_id: None,
            diagnostics_json: "{}",
        })
        .await
        .unwrap();
    let schema = ParameterSchema {
        schema_version: crate::parameters::SCHEMA_VERSION,
        source: fixture.model.onshape.clone(),
        parameters: vec![Parameter {
            id: "wide".to_owned(),
            label: "Wide".to_owned(),
            description: None,
            kind: ParameterKind::Boolean,
            required: true,
            default_value: Some("false".to_owned()),
            options: vec![],
            hidden: false,
            visibility_condition: None,
            precision: None,
            widget: None,
            units: None,
            raw: json!({}),
        }],
    };
    fixture.objects.lock().unwrap().insert(
        "/synthetic-bucket/real-test-parameters".to_owned(),
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
            "real-test-parameters-raw",
            "real-test-parameters",
            "real-test-schema",
            crate::parameters::SCHEMA_VERSION as i64,
        )
        .await
        .unwrap();
    let api = FreshApiScript {
        replies: Arc::new(Mutex::new(script(case))),
        requests: Arc::new(Mutex::new(Vec::new())),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    fixture
        .state
        .onshape
        .set_test_selection_origin(format!("http://{}", listener.local_addr().unwrap()));
    let router = axum::Router::new()
        .fallback(fresh_api)
        .with_state(api.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let validated = crate::validate_values(
        &schema,
        &HashMap::from([("wide".to_owned(), "true".to_owned())]),
        false,
    )
    .unwrap();
    let request = bambu_project::enqueue(
        &fixture.state,
        &fixture.model,
        &validated,
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
    // Drive the real worker transition too: terminal unavailable outcomes must
    // finish their queue row before status can cease reporting queued/running.
    let outcome = crate::process_next_job(&fixture.state).await;
    server.abort();
    assert!(outcome.unwrap());
    {
        let requests = api.requests.lock().unwrap();
        let encoding = requests
            .iter()
            .find(|request| request.path.ends_with("/configurationencodings"))
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&encoding.bytes).unwrap(),
            json!({"parameters":[{"parameterId":"wide","parameterValue":"true"}]})
        );
        if !matches!(case, SourceCase::UnsupportedAssembly) {
            let translations: Vec<_> = requests
                .iter()
                .filter(|request| {
                    request.method == "POST" && request.path.ends_with("/translations")
                })
                .collect();
            assert_eq!(translations.len(), 3);
            if matches!(case, SourceCase::PartStudio) {
                for (translation, part) in
                    translations.iter().zip(["part-a", "part-b", "part-block"])
                {
                    let body: Value = serde_json::from_slice(&translation.bytes).unwrap();
                    assert_eq!(body["configuration"], "real-test-original-encoding");
                    assert_eq!(body["partIds"], part);
                }
            }
        }
    }
    if matches!(case, SourceCase::UnsupportedAssembly) {
        let unavailable =
            bambu_project::status(&fixture.state, &fixture.model, &request.request_identity)
                .await
                .unwrap();
        assert_eq!(unavailable.status, "unavailable");
        assert!(unavailable.public_url.is_none() && unavailable.object_key.is_none());
        assert!(
            !fixture
                .calls
                .lock()
                .unwrap()
                .iter()
                .any(|call| call == "PUT")
        );
        assert!(
            !api.requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| request.path.ends_with("/translations"))
        );
        return;
    }
    assert!(api.replies.lock().unwrap().is_empty());
    let ready = bambu_project::status(&fixture.state, &fixture.model, &request.request_identity)
        .await
        .unwrap();
    assert_eq!(ready.status, "ready");
    assert_eq!(ready.media_type.as_deref(), Some("model/3mf"));
    assert!(ready.public_url.is_some());
    let object_key = ready.object_key.unwrap();
    {
        let objects = fixture.objects.lock().unwrap();
        let published = &objects[&format!("/synthetic-bucket/{object_key}")].0;
        assert_eq!(Some(crate::cache_key::hex_sha256(published)), ready.sha256);
        assert_eq!(Some(published.len() as i64), ready.byte_length);
    }
    let record = fixture
        .state
        .db
        .bambu_project_request(&request.request_identity)
        .await
        .unwrap()
        .unwrap();
    let prepared = fixture
        .state
        .db
        .reopen_generator_processing(&generator, record.processing_hash.as_deref().unwrap())
        .await
        .unwrap();
    let manifest = &prepared.recipe().input_manifest;
    let object_count = if matches!(case, SourceCase::PartStudio) {
        3
    } else {
        4
    };
    assert_eq!(manifest.objects.len(), object_count);
    assert_eq!(
        manifest
            .objects
            .iter()
            .map(|object| &object.object_identity)
            .collect::<HashSet<_>>()
            .len(),
        object_count
    );
    assert_eq!(
        manifest
            .objects
            .iter()
            .map(|object| &object.retained_content.path)
            .collect::<HashSet<_>>()
            .len(),
        object_count
    );
    assert_eq!(
        manifest
            .objects
            .iter()
            .map(|object| &object.retained_content.sha256)
            .collect::<HashSet<_>>()
            .len(),
        1
    );
    assert_eq!(prepared.recipe().settings.blockers.len(), 1);
    assert_eq!(
        prepared.recipe().settings.blockers[0].targets.len(),
        if matches!(case, SourceCase::PartStudio) {
            2
        } else {
            1
        }
    );
    let names: Vec<_> = manifest
        .objects
        .iter()
        .map(|object| object.display_name.as_deref().unwrap())
        .collect();
    assert_eq!(
        names,
        if matches!(case, SourceCase::PartStudio) {
            vec![
                "Same synthetic display",
                "Same synthetic display",
                "Synthetic blocker",
            ]
        } else {
            vec![
                "Repeated synthetic part",
                "Synthetic blocker",
                "Repeated synthetic part",
                "Unique synthetic target",
            ]
        }
    );
    if matches!(case, SourceCase::Assembly) {
        let positions: Vec<_> = prepared
            .recipe()
            .settings
            .placements
            .iter()
            .map(|placement| placement.matrix[3])
            .collect();
        assert_eq!(positions, vec![0.0, 0.02, 0.04, 0.06]);
    }
    let calls_before = fixture.calls.lock().unwrap().len();
    let cached = bambu_project::enqueue(
        &fixture.state,
        &fixture.model,
        &validated,
        &request.selectors,
    )
    .await
    .unwrap();
    assert_eq!(cached.request_identity, request.request_identity);
    assert_eq!(fixture.calls.lock().unwrap().len(), calls_before);
    let ordinary = crate::render_download_buttons(&fixture.model);
    assert!(
        ordinary.contains("/exports/step")
            && ordinary.contains("/exports/stl")
            && ordinary.contains("/exports/3mf")
            && !ordinary.contains("/projects/bambu")
    );
    assert_eq!(
        crate::download_label(catalog::DownloadFormat::ThreeMf),
        "Geometry 3MF"
    );
}

#[tokio::test]
#[ignore = "requires the reviewed real external package and dedicated admission environment"]
async fn released_package_configured_part_studio_publishes_verified_project() {
    run_real(SourceCase::PartStudio).await;
}

#[tokio::test]
#[ignore = "requires the reviewed real external package and dedicated admission environment"]
async fn released_package_rigid_assembly_preserves_distinct_occurrences_and_blocker_targets() {
    run_real(SourceCase::Assembly).await;
}

#[tokio::test]
#[ignore = "requires the reviewed real external package and dedicated admission environment"]
async fn released_package_nonrigid_assembly_stays_unavailable_without_translation_or_publication() {
    run_real(SourceCase::UnsupportedAssembly).await;
}
