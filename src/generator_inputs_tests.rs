use super::*;
use crate::generator_protocol::InputRole;

#[test]
fn object_identity_and_path_bytes_are_frozen_by_golden_vectors() {
    let first = manifest_object_identity(&"0".repeat(64)).unwrap();
    let second = manifest_object_identity(&"1".repeat(64)).unwrap();
    assert_eq!(
        first,
        "1878d7de38529bdc2805623db939e9f834dbee189325add96b5d8a1237422c2f"
    );
    assert_eq!(
        second,
        "6247f19bea97d870a6019e35f83b44353168a40ff736739d099684a5ae9a3a96"
    );
    assert_eq!(RETAINED_PATH_ALLOCATION_VERSION, 1);
    let identities = [first.clone(), second.clone()];
    let expected = [
        "inputs/geometry-v1/000-1878d7de38529bdc2805623db939e9f834dbee189325add96b5d8a1237422c2f.3mf",
        "inputs/geometry-v1/001-6247f19bea97d870a6019e35f83b44353168a40ff736739d099684a5ae9a3a96.3mf",
    ];
    assert_eq!(allocate_retained_paths(&identities).unwrap(), expected);
    assert_eq!(allocate_retained_paths(&identities).unwrap(), expected);
    let reversed = allocate_retained_paths(&[second, first]).unwrap();
    assert_ne!(reversed[0], expected[1]);
    assert_ne!(reversed[1], expected[0]);
    let changed = allocate_retained_paths(&["2".repeat(64), identities[1].clone()]).unwrap();
    assert_ne!(changed[0], expected[0]);
    assert_eq!(changed[1], expected[1]);
    let full: Vec<_> = (0..256).map(|index| format!("{index:064x}")).collect();
    let paths = allocate_retained_paths(&full).unwrap();
    assert_eq!(
        paths[255],
        format!("inputs/geometry-v1/255-{}.3mf", full[255])
    );
    let content = cache_key::hash_json(
        "onshape-export-retained-geometry-content-v1",
        &ContentIdentity {
            sha256: &"2".repeat(64),
            byte_length: 21,
        },
    )
    .unwrap();
    assert_eq!(
        content,
        "acd79dc89fdd7ffadace9d803fe09e6a6793b703d7b40a0638292e3d8c1385bd"
    );
}

#[test]
fn allocation_rejects_unsafe_identities_collisions_and_invalid_counts() {
    for identities in [vec![], vec!["0".repeat(64); 2], vec!["0".repeat(64); 257]] {
        assert!(allocate_retained_paths(&identities).is_err());
    }
    for identity in [
        "../escape".to_owned(),
        "A".repeat(64),
        "z".repeat(64),
        "0".repeat(63),
    ] {
        assert!(allocate_retained_paths(&[identity]).is_err());
    }
    for paths in [
        vec!["inputs/a.3mf".to_owned(); 2],
        vec!["inputs/../escape.3mf".to_owned()],
        vec!["/inputs/a.3mf".to_owned()],
        vec!["outputs/a.3mf".to_owned()],
        vec!["inputs/a\\b.3mf".to_owned()],
        vec!["inputs/a\0b.3mf".to_owned()],
    ] {
        assert!(validate_allocated_paths(&paths).is_err());
    }
}

fn contextual_fixture() -> (
    InputManifest,
    GeneratorSettingsV2,
    Vec<ExpectedPlacementSummaryV2>,
) {
    let mut manifest = generator_protocol::parse_input_manifest(include_bytes!(
        "../protocol/generator/v1/examples/input-manifest.json"
    ))
    .unwrap();
    let mut second = manifest.objects[0].clone();
    second.object_identity = "second-manifest-object".to_owned();
    second.retained_content.path = "inputs/second.3mf".to_owned();
    manifest.objects.push(second);
    manifest.input_set_identity = manifest.computed_input_set_identity().unwrap();
    manifest.manifest_identity = manifest.computed_manifest_identity().unwrap();
    let expected: Vec<_> = manifest
        .objects
        .iter()
        .map(|object| ExpectedPlacementSummaryV2 {
            object_identity: object.object_identity.clone(),
            transport_role: object.role,
            expected_neutral_placement_matrix: crate::onshape_selection::IDENTITY_PLACEMENT
                .to_vec(),
        })
        .collect();
    let settings = GeneratorSettingsV2 {
        schema_version: 2,
        blockers: vec![],
        placements: expected
            .iter()
            .map(|entry| GeneratorSettingsPlacementV2 {
                object_identity: entry.object_identity.clone(),
                matrix: entry.expected_neutral_placement_matrix.clone(),
            })
            .collect(),
    };
    (manifest, settings, expected)
}

#[test]
fn construction_context_rejects_cardinality_order_identity_role_and_matrix_mismatches() {
    let (manifest, settings, expected) = contextual_fixture();
    validate_construction(&manifest, &settings, &expected).unwrap();
    let mutations: [fn(&mut Vec<ExpectedPlacementSummaryV2>); 7] = [
        |entries| {
            entries.pop();
        },
        |entries| entries.push(entries[0].clone()),
        |entries| entries.swap(0, 1),
        |entries| entries[0].object_identity = "wrong-object".to_owned(),
        |entries| entries[0].transport_role = InputRole::AuxiliaryGeometry,
        |entries| entries[0].expected_neutral_placement_matrix[3] = 0.25,
        |entries| {
            entries[0].expected_neutral_placement_matrix.pop();
        },
    ];
    for mutate in mutations {
        let mut entries = expected.clone();
        mutate(&mut entries);
        assert!(validate_construction(&manifest, &settings, &entries).is_err());
    }
    let mutations: [fn(&mut GeneratorSettingsV2); 7] = [
        |settings| {
            settings.placements.pop();
        },
        |settings| settings.placements.push(settings.placements[0].clone()),
        |settings| settings.placements.swap(0, 1),
        |settings| settings.placements[0].object_identity = "wrong-object".to_owned(),
        |settings| settings.placements[0].matrix[3] = 0.25,
        |settings| settings.placements[0].matrix[0] = f64::NAN,
        |settings| settings.placements[0].matrix[15] = 2.,
    ];
    for mutate in mutations {
        let mut changed = settings.clone();
        mutate(&mut changed);
        assert!(validate_construction(&manifest, &changed, &expected).is_err());
    }
    let mut normalized = settings.clone();
    normalized.placements[0].matrix[3] = -0.0;
    validate_construction(&manifest, &normalized, &expected).unwrap();
}

#[test]
fn construction_enforces_protocol_document_bounds_and_rehashed_shared_path_rejection() {
    let (mut manifest, settings, expected) = contextual_fixture();
    manifest.objects[1].retained_content.path = manifest.objects[0].retained_content.path.clone();
    manifest.input_set_identity = manifest.computed_input_set_identity().unwrap();
    manifest.manifest_identity = manifest.computed_manifest_identity().unwrap();
    assert!(validate_construction(&manifest, &settings, &expected).is_err());

    let (mut manifest, _, _) = contextual_fixture();
    let original = manifest.objects[0].clone();
    manifest.objects = (0..256)
        .map(|index| {
            let mut object = original.clone();
            object.object_identity = format!("object-{index:03}");
            object.retained_content.path = format!("inputs/geometry-{index:03}.3mf");
            object.display_name = Some("😀".repeat(1024));
            object
        })
        .collect();
    manifest.input_set_identity = manifest.computed_input_set_identity().unwrap();
    manifest.manifest_identity = manifest.computed_manifest_identity().unwrap();
    // All typed semantic rules pass; serialized protocol bounds still fail.
    manifest.validate().unwrap();
    let bytes = cache_key::canonical_json_bytes(&manifest).unwrap();
    assert!(bytes.len() > generator_protocol::MAX_INPUT_MANIFEST_BYTES);
    assert!(matches!(
        generator_protocol::parse_input_manifest(&bytes),
        Err(generator_protocol::ProtocolError::DocumentTooLarge { .. })
    ));
    assert!(validate_construction(&manifest, &settings, &expected).is_err());
}
