//! Immutable source-neutral selection planning. Projection is deliberately phase-local:
//! validate every reached field in a phase, then classify unavailable source state.

use std::{
    collections::{HashMap, HashSet},
    io::Write,
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::{
    cache_key,
    cache_model::ResolvedOnshapeSourceIdentity,
    catalog,
    configuration_encoding::{self, EncodingHandoff},
    db::Database,
    onshape_annotation::{
        self as authoring, AuthoringContextEntry, AuthoringDocument, AuthoringSelector,
        NormalizedAnnotation,
    },
    onshape_api::{ApiFailure, FailureKind, OnshapeApi, bounded_visible_ascii, is_sha256},
};

const MAX_SELECTORS: usize = 256;
const MAX_RELATION_IDS: usize = 16_384;
const MAX_PLAN_BYTES: usize = 8_388_608;
const EPSILON: f64 = 1e-12;
pub const IDENTITY_PLACEMENT: [f64; 16] = [
    1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionElementKind {
    PartStudio,
    Assembly,
}

impl SelectionElementKind {
    fn catalog_kind(self) -> catalog::ElementKind {
        match self {
            Self::PartStudio => catalog::ElementKind::PartStudio,
            Self::Assembly => catalog::ElementKind::Assembly,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged, rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum SelectionSelector {
    Part { part_id: String },
    Occurrence { occurrence_path: Vec<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectionRequest {
    pub document_id: String,
    pub version_id: String,
    pub element_id: String,
    pub element_kind: SelectionElementKind,
    pub configuration_encoding: EncodingHandoff,
    pub selectors: Vec<SelectionSelector>,
}

impl SelectionRequest {
    pub fn from_json(bytes: &[u8]) -> Result<Self, ApiFailure> {
        let value = crate::onshape_api::parse_json(bytes, MAX_PLAN_BYTES).map_err(|_| invalid())?;
        let request: Self = serde_json::from_value(value).map_err(|_| invalid())?;
        request.preflight()?;
        Ok(request)
    }

    pub fn preflight(&self) -> Result<(), ApiFailure> {
        let h = &self.configuration_encoding;
        if ![
            &self.document_id,
            &self.version_id,
            &self.element_id,
            &h.encoded_id,
        ]
        .into_iter()
        .all(|s| bounded_visible_ascii(s))
            || ![&h.source_hash, &h.config_hash, &h.encoding_context_hash]
                .into_iter()
                .all(|s| is_sha256(s))
            || !(1..=MAX_SELECTORS).contains(&self.selectors.len())
        {
            return Err(invalid());
        }
        let mut seen = HashSet::new();
        for (position, selector) in self.selectors.iter().enumerate() {
            let valid = match (self.element_kind, selector) {
                (SelectionElementKind::PartStudio, SelectionSelector::Part { part_id }) => {
                    bounded_visible_ascii(part_id)
                }
                (
                    SelectionElementKind::Assembly,
                    SelectionSelector::Occurrence { occurrence_path },
                ) => occurrence_path.len() == 1 && bounded_visible_ascii(&occurrence_path[0]),
                _ => false,
            };
            if !valid || !seen.insert(selector) {
                return Err(invalid().position(position));
            }
        }
        Ok(())
    }
}

/// The configured leaf has no version alias, occurrence, request representation, or metadata.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfiguredLeaf {
    pub document_id: String,
    pub document_microversion: String,
    pub element_id: String,
    pub element_kind: PartStudioLeafKind,
    pub configuration_identity: String,
    pub part_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartStudioLeafKind {
    PartStudio,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectionRoot {
    pub document_id: String,
    pub version_id: String,
    pub document_microversion: String,
    pub element_id: String,
    pub element_kind: SelectionElementKind,
    pub configuration_identity: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolvedSelectionObject {
    pub plan_local_object_identity: String,
    pub selector: AuthoringSelector,
    pub configured_leaf: ConfiguredLeaf,
    pub display_name: String,
    pub annotation: NormalizedAnnotation,
    pub expected_neutral_placement_matrix: [f64; 16],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolvedSelectionPlan {
    pub root: SelectionRoot,
    pub authoring_document_identity: String,
    pub objects: Vec<ResolvedSelectionObject>,
    pub plan_identity: String,
}

struct Candidate {
    leaf: ConfiguredLeaf,
    selector: AuthoringSelector,
    matrix: [f64; 16],
    name: String,
    description: String,
}

/// Planning never encodes or repairs evidence and returns no partial result.
pub async fn plan_selection(
    db: &Database,
    api: &OnshapeApi,
    request: &SelectionRequest,
) -> Result<ResolvedSelectionPlan, ApiFailure> {
    request.preflight()?;
    let microversion = api
        .resolve_version(&request.document_id, &request.version_id)
        .await?;
    let source = ResolvedOnshapeSourceIdentity {
        document_id: request.document_id.clone(),
        version_id: request.version_id.clone(),
        microversion_id: microversion.clone(),
        element_id: request.element_id.clone(),
        element_kind: request.element_kind.catalog_kind(),
        link_document_id: None,
    };
    configuration_encoding::validate_handoff(db, api, &source, &request.configuration_encoding)
        .await?;
    let configuration = &request.configuration_encoding.encoded_id;
    let (root, candidates) = match request.element_kind {
        SelectionElementKind::PartStudio => {
            let response = api
                .parts(
                    &request.document_id,
                    &microversion,
                    &request.element_id,
                    configuration,
                )
                .await?;
            resolve_part_studio(request, &microversion, &response)
                .map_err(|e| e.operation("getPartsWMVE"))?
        }
        SelectionElementKind::Assembly => {
            let response = api
                .assembly(
                    &request.document_id,
                    &microversion,
                    &request.element_id,
                    configuration,
                )
                .await?;
            let mut assembly = resolve_assembly(request, &microversion, &response)
                .map_err(|e| e.operation("getAssemblyDefinition"))?;
            for group in &assembly.carriers {
                let response = api
                    .parts(
                        &group.document_id,
                        &group.microversion,
                        &group.element_id,
                        &group.configuration,
                    )
                    .await
                    .map_err(|e| e.position(group.positions[0]))?;
                resolve_carrier(&response, group, &mut assembly.candidates).map_err(|e| {
                    position_if_missing(e, group.positions[0]).operation("getPartsWMVE")
                })?;
            }
            (assembly.root, assembly.candidates)
        }
    };
    construct_plan(root, candidates)
}

fn invalid() -> ApiFailure {
    ApiFailure::new(FailureKind::InvalidSelection, "invalid_selection")
}
fn operational() -> ApiFailure {
    ApiFailure::new(
        FailureKind::OperationalApiContractFailure,
        "malformed_source_projection",
    )
}
fn unavailable(position: usize) -> ApiFailure {
    ApiFailure::new(
        FailureKind::UnavailableSourceState,
        "unavailable_source_state",
    )
    .position(position)
}
fn position_if_missing(mut failure: ApiFailure, position: usize) -> ApiFailure {
    for diagnostic in &mut failure.diagnostics {
        if diagnostic.selector_position.is_none() {
            diagnostic.selector_position = Some(position);
        }
    }
    failure
}
fn annotation_failure(error: authoring::AnnotationError, position: Option<usize>) -> ApiFailure {
    let kind = if matches!(error, authoring::AnnotationError::Identity(_)) {
        FailureKind::OperationalApiContractFailure
    } else {
        FailureKind::UnavailableSourceState
    };
    let mut failure =
        ApiFailure::new(kind, "authoring_validation_failed").operation("authoringValidation");
    if let Some(position) = position {
        failure = failure.position(position);
    }
    failure
}
// These locate affected source entries only after the authoritative #176 validator
// has rejected the document. No private validator message is parsed or published.
fn document_failure_position(document: &AuthoringDocument) -> Option<usize> {
    let mut keys: HashMap<&str, Vec<usize>> = HashMap::new();
    for (p, obj) in document.objects.iter().enumerate() {
        if let Some(key) = obj.annotation.key.as_deref() {
            keys.entry(key).or_default().push(p);
        }
    }
    document.objects.iter().enumerate().find_map(|(p, obj)| {
        obj.annotation
            .targets
            .iter()
            .any(|target| match keys.get(target.as_str()) {
                Some(positions) if positions.len() == 1 => {
                    positions[0] == p
                        || document.objects[positions[0]].annotation.role
                            != authoring::SemanticRole::Printable
                }
                _ => true,
            })
            .then_some(p)
    })
}
fn context_failure_position(
    document: &AuthoringDocument,
    candidates: &[Candidate],
) -> Option<usize> {
    let mut first_by_leaf: HashMap<&ConfiguredLeaf, usize> = HashMap::new();
    let mut first_by_key: HashMap<&str, usize> = HashMap::new();
    let mut lowest = None;
    for (p, (obj, candidate)) in document.objects.iter().zip(candidates).enumerate() {
        if let Some(&first) = first_by_leaf.get(&candidate.leaf) {
            let previous = &document.objects[first];
            if previous.display_name != obj.display_name || previous.annotation != obj.annotation {
                lowest = Some(lowest.map_or(first, |old: usize| old.min(first)));
            }
        } else {
            first_by_leaf.insert(&candidate.leaf, p);
        }
        if let Some(key) = obj.annotation.key.as_deref() {
            if let Some(&first) = first_by_key.get(key) {
                if candidates[first].leaf != candidate.leaf
                    || document.objects[first].annotation != obj.annotation
                {
                    lowest = Some(lowest.map_or(first, |old: usize| old.min(first)));
                }
            } else {
                first_by_key.insert(key, p);
            }
        }
    }
    lowest
}
fn object(value: &Value) -> Result<&Map<String, Value>, ApiFailure> {
    value.as_object().ok_or_else(operational)
}
fn array<'a>(obj: &'a Map<String, Value>, key: &str) -> Result<&'a [Value], ApiFailure> {
    obj.get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(operational)
}
fn string<'a>(obj: &'a Map<String, Value>, key: &str) -> Result<&'a str, ApiFailure> {
    obj.get(key).and_then(Value::as_str).ok_or_else(operational)
}
fn source_string<'a>(obj: &'a Map<String, Value>, key: &str) -> Result<&'a str, ApiFailure> {
    let value = string(obj, key)?;
    if bounded_visible_ascii(value) {
        Ok(value)
    } else {
        Err(operational())
    }
}
fn boolean(obj: &Map<String, Value>, key: &str) -> Result<bool, ApiFailure> {
    obj.get(key)
        .and_then(Value::as_bool)
        .ok_or_else(operational)
}
fn root(request: &SelectionRequest, mid: &str, configuration: &str) -> SelectionRoot {
    SelectionRoot {
        document_id: request.document_id.clone(),
        version_id: request.version_id.clone(),
        document_microversion: mid.to_owned(),
        element_id: request.element_id.clone(),
        element_kind: request.element_kind,
        configuration_identity: configuration.to_owned(),
    }
}
fn leaf(d: &str, m: &str, e: &str, c: &str, p: &str) -> ConfiguredLeaf {
    ConfiguredLeaf {
        document_id: d.into(),
        document_microversion: m.into(),
        element_id: e.into(),
        element_kind: PartStudioLeafKind::PartStudio,
        configuration_identity: c.into(),
        part_id: p.into(),
    }
}
fn part_selector(leaf: &ConfiguredLeaf) -> AuthoringSelector {
    AuthoringSelector::PartStudioPart {
        document_id: leaf.document_id.clone(),
        document_microversion: leaf.document_microversion.clone(),
        element_id: leaf.element_id.clone(),
        configuration_identity: leaf.configuration_identity.clone(),
        part_id: leaf.part_id.clone(),
    }
}

fn solid_state(obj: &Map<String, Value>) -> Result<bool, ApiFailure> {
    let body = source_string(obj, "bodyType")?;
    let flattened = boolean(obj, "isFlattenedBody")?;
    let mesh = boolean(obj, "isMesh")?;
    let state = string(obj, "meshState")?;
    if !["NO_MESH", "MIXED", "ALL_MESH", "UNKNOWN"].contains(&state) {
        return Err(operational());
    }
    let hidden = boolean(obj, "isHidden")?;
    Ok(body == "solid" && !flattened && !mesh && state == "NO_MESH" && !hidden)
}

fn resolve_part_studio(
    request: &SelectionRequest,
    mid: &str,
    response: &Value,
) -> Result<(SelectionRoot, Vec<Candidate>), ApiFailure> {
    let rows = response.as_array().ok_or_else(operational)?;
    let mut index = HashMap::new();
    for row in rows {
        let obj = object(row)?;
        let id = source_string(obj, "partId")?;
        if index.insert(id, obj).is_some() {
            return Err(operational());
        }
    }
    let mut selected = Vec::new();
    for (position, selector) in request.selectors.iter().enumerate() {
        let SelectionSelector::Part { part_id } = selector else {
            return Err(invalid());
        };
        selected.push(
            *index
                .get(part_id.as_str())
                .ok_or_else(|| unavailable(position))?,
        );
    }
    let mut identities = Vec::new();
    for (position, obj) in selected.iter().enumerate() {
        let e = source_string(obj, "elementId").map_err(|e| e.position(position))?;
        let m = source_string(obj, "microversionId").map_err(|e| e.position(position))?;
        let c = source_string(obj, "configurationId").map_err(|e| e.position(position))?;
        if e != request.element_id || m != mid {
            return Err(operational().position(position));
        }
        identities.push(c);
    }
    if identities.iter().any(|c| *c != identities[0]) {
        return Err(operational());
    }
    let states: Vec<_> = selected
        .iter()
        .enumerate()
        .map(|(p, obj)| solid_state(obj).map_err(|e| e.position(p)))
        .collect::<Result<_, _>>()?;
    if let Some(p) = states.iter().position(|supported| !supported) {
        return Err(unavailable(p));
    }
    let mut candidates = Vec::new();
    for (position, obj) in selected.iter().enumerate() {
        let name = string(obj, "name").map_err(|e| e.position(position))?;
        let description = string(obj, "description").map_err(|e| e.position(position))?;
        let configured = leaf(
            &request.document_id,
            mid,
            &request.element_id,
            identities[position],
            source_string(obj, "partId")?,
        );
        candidates.push(Candidate {
            selector: part_selector(&configured),
            leaf: configured,
            matrix: IDENTITY_PLACEMENT,
            name: name.into(),
            description: description.into(),
        });
    }
    Ok((root(request, mid, identities[0]), candidates))
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CarrierKey {
    document_id: String,
    microversion: String,
    element_id: String,
    configuration: String,
}
struct CarrierGroup {
    key: CarrierKey,
    positions: Vec<usize>,
}
impl std::ops::Deref for CarrierGroup {
    type Target = CarrierKey;
    fn deref(&self) -> &Self::Target {
        &self.key
    }
}
struct AssemblyResolution {
    root: SelectionRoot,
    candidates: Vec<Candidate>,
    carriers: Vec<CarrierGroup>,
}

fn occurrence_path(obj: &Map<String, Value>) -> Result<Vec<String>, ApiFailure> {
    let values = array(obj, "path")?;
    if !(1..=64).contains(&values.len()) {
        return Err(operational());
    }
    values
        .iter()
        .map(|v| {
            v.as_str()
                .filter(|s| bounded_visible_ascii(s))
                .map(str::to_owned)
                .ok_or_else(operational)
        })
        .collect()
}

fn relation_ids(root: &Map<String, Value>) -> Result<HashSet<String>, ApiFailure> {
    let mut result = HashSet::new();
    let mut count = 0;
    let mut add = |id: &str| -> Result<(), ApiFailure> {
        if !bounded_visible_ascii(id) {
            return Err(operational());
        }
        count += 1;
        if count > MAX_RELATION_IDS {
            return Err(operational());
        }
        result.insert(id.to_owned());
        Ok(())
    };
    let mut parametric = HashSet::new();
    for relation in array(root, "parametricInstances")? {
        let relation = object(relation)?;
        let id = source_string(relation, "id")?;
        if !parametric.insert(id) {
            return Err(operational());
        }
        add(id)?;
        let mut children = HashSet::new();
        for child in array(relation, "children")? {
            let child = object(child)?;
            for id in array(child, "instanceIds")? {
                let id = id.as_str().ok_or_else(operational)?;
                if !children.insert(id) {
                    return Err(operational());
                }
                add(id)?;
            }
            if child.contains_key("seedOccurrence") {
                add(source_string(child, "seedOccurrence")?)?;
            }
        }
    }
    let mut patterns = HashSet::new();
    for relation in array(root, "patterns")? {
        let relation = object(relation)?;
        if !patterns.insert(source_string(relation, "id")?) {
            return Err(operational());
        }
        let seeds = object(
            relation
                .get("seedToPatternInstances")
                .ok_or_else(operational)?,
        )?;
        let mut generated = HashSet::new();
        for (seed, ids) in seeds {
            add(seed)?;
            for id in ids.as_array().ok_or_else(operational)? {
                let id = id.as_str().ok_or_else(operational)?;
                if !generated.insert(id) {
                    return Err(operational());
                }
                add(id)?;
            }
        }
    }
    Ok(result)
}

fn resolve_assembly(
    request: &SelectionRequest,
    mid: &str,
    response: &Value,
) -> Result<AssemblyResolution, ApiFailure> {
    let envelope = object(response)?;
    let assembly = object(envelope.get("rootAssembly").ok_or_else(operational)?)?;
    let d = source_string(assembly, "documentId")?;
    let m = source_string(assembly, "documentMicroversion")?;
    let e = source_string(assembly, "elementId")?;
    let configuration = source_string(assembly, "fullConfiguration")?;
    let instances = array(assembly, "instances")?;
    let occurrences = array(assembly, "occurrences")?;
    array(assembly, "parametricInstances")?;
    array(assembly, "patterns")?;
    let parts = array(envelope, "parts")?;
    if d != request.document_id || m != mid || e != request.element_id {
        return Err(operational());
    }

    let mut instance_index = HashMap::new();
    for instance in instances {
        let instance = object(instance)?;
        if instance_index
            .insert(source_string(instance, "id")?, instance)
            .is_some()
        {
            return Err(operational());
        }
    }
    let mut occurrence_index = HashMap::new();
    for occurrence in occurrences {
        let occurrence = object(occurrence)?;
        if occurrence_index
            .insert(occurrence_path(occurrence)?, occurrence)
            .is_some()
        {
            return Err(operational());
        }
    }
    let exclusions = relation_ids(assembly)?;
    let mut selected = Vec::new();
    for (position, selector) in request.selectors.iter().enumerate() {
        let SelectionSelector::Occurrence { occurrence_path } = selector else {
            return Err(invalid());
        };
        let instance = instance_index.get(occurrence_path[0].as_str());
        let occurrence = occurrence_index.get(occurrence_path);
        match (instance, occurrence) {
            (Some(i), Some(o)) => selected.push(Some((*i, *o))),
            (None, None) => selected.push(None),
            _ => return Err(operational().position(position)),
        }
    }
    if let Some(p) = selected.iter().position(Option::is_none) {
        return Err(unavailable(p));
    }
    let selected: Vec<_> = selected
        .into_iter()
        .map(|entry| entry.expect("validated associations"))
        .collect();
    if let Some(p) = request
        .selectors
        .iter()
        .position(|selector| match selector {
            SelectionSelector::Occurrence { occurrence_path } => {
                exclusions.contains(&occurrence_path[0])
            }
            _ => false,
        })
    {
        return Err(unavailable(p));
    }

    // Each gate validates its complete reached projection before classifying state.
    let deleted = selected
        .iter()
        .enumerate()
        .map(|(p, (i, _))| match i.get("status") {
            None => Ok(false),
            Some(Value::String(value)) if value == "DeletedElement" => Ok(true),
            _ => Err(operational().position(p)),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(p) = deleted.iter().position(|v| *v) {
        return Err(unavailable(p));
    }
    let types = selected
        .iter()
        .enumerate()
        .map(|(p, (i, _))| {
            let t = string(i, "type").map_err(|e| e.position(p))?;
            if !["Part", "Assembly", "Feature", "Unknown"].contains(&t) {
                return Err(operational().position(p));
            }
            Ok(t == "Part")
        })
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(p) = types.iter().position(|v| !*v) {
        return Err(unavailable(p));
    }
    let suppressed = selected
        .iter()
        .enumerate()
        .map(|(p, (i, _))| boolean(i, "suppressed").map_err(|e| e.position(p)))
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(p) = suppressed.iter().position(|v| *v) {
        return Err(unavailable(p));
    }
    let hidden = selected
        .iter()
        .enumerate()
        .map(|(p, (_, o))| boolean(o, "hidden").map_err(|e| e.position(p)))
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(p) = hidden.iter().position(|v| *v) {
        return Err(unavailable(p));
    }
    let leaves = selected
        .iter()
        .enumerate()
        .map(|(p, (i, _))| projected_leaf(i).map_err(|e| e.position(p)))
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(p) = leaves
        .iter()
        .position(|l| l.document_id != d || l.document_microversion != m)
    {
        return Err(unavailable(p));
    }
    let matrices = selected
        .iter()
        .enumerate()
        .map(|(p, (_, o))| project_transform(o).map_err(|e| e.position(p)))
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(p) = matrices.iter().position(|matrix| !proper_rigid(matrix)) {
        return Err(unavailable(p));
    }

    let mut part_index = HashMap::new();
    for part in parts {
        let part = object(part)?;
        let key = projected_leaf(part)?;
        if part_index.insert(key, part).is_some() {
            return Err(operational());
        }
    }
    let mut joined = Vec::new();
    for (p, l) in leaves.iter().enumerate() {
        joined.push(*part_index.get(l).ok_or_else(|| operational().position(p))?);
    }
    let bodies = joined
        .iter()
        .enumerate()
        .map(|(p, obj)| {
            let body = string(obj, "bodyType").map_err(|e| e.position(p))?;
            if !["solid", "sheet", "composite"].contains(&body) {
                return Err(operational().position(p));
            }
            Ok(body == "solid")
        })
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(p) = bodies.iter().position(|v| !*v) {
        return Err(unavailable(p));
    }

    let mut carriers: Vec<CarrierGroup> = Vec::new();
    let mut carrier_index = HashMap::new();
    let mut candidates = Vec::new();
    for (position, (l, matrix)) in leaves.into_iter().zip(matrices).enumerate() {
        let key = CarrierKey {
            document_id: l.document_id.clone(),
            microversion: l.document_microversion.clone(),
            element_id: l.element_id.clone(),
            configuration: l.configuration_identity.clone(),
        };
        let next = carriers.len();
        let group = *carrier_index.entry(key.clone()).or_insert(next);
        if group == next {
            carriers.push(CarrierGroup {
                key,
                positions: Vec::new(),
            });
        }
        carriers[group].positions.push(position);
        let SelectionSelector::Occurrence { occurrence_path } = &request.selectors[position] else {
            return Err(invalid());
        };
        candidates.push(Candidate {
            selector: AuthoringSelector::AssemblyOccurrence {
                document_id: d.into(),
                document_microversion: m.into(),
                element_id: e.into(),
                configuration_identity: configuration.into(),
                occurrence_path: occurrence_path.clone(),
            },
            leaf: l,
            matrix: normalize_matrix(matrix),
            name: String::new(),
            description: String::new(),
        });
    }
    Ok(AssemblyResolution {
        root: root(request, mid, configuration),
        candidates,
        carriers,
    })
}

fn projected_leaf(obj: &Map<String, Value>) -> Result<ConfiguredLeaf, ApiFailure> {
    Ok(leaf(
        source_string(obj, "documentId")?,
        source_string(obj, "documentMicroversion")?,
        source_string(obj, "elementId")?,
        source_string(obj, "fullConfiguration")?,
        source_string(obj, "partId")?,
    ))
}
fn project_transform(obj: &Map<String, Value>) -> Result<[f64; 16], ApiFailure> {
    let values = array(obj, "transform")?;
    if values.len() != 16 {
        return Err(operational());
    }
    let mut matrix = [0.; 16];
    for (target, value) in matrix.iter_mut().zip(values) {
        *target = value
            .as_f64()
            .filter(|n| n.is_finite())
            .ok_or_else(operational)?;
    }
    Ok(matrix)
}
fn proper_rigid(matrix: &[f64; 16]) -> bool {
    if matrix.iter().any(|n| !n.is_finite()) {
        return false;
    }
    for (value, expected) in matrix[12..].iter().zip([0., 0., 0., 1.]) {
        if (*value - expected).abs() > EPSILON {
            return false;
        }
    }
    for i in 0..3 {
        for j in 0..3 {
            let dot: f64 = (0..3).map(|k| matrix[4 * k + i] * matrix[4 * k + j]).sum();
            if (dot - if i == j { 1. } else { 0. }).abs() > EPSILON {
                return false;
            }
        }
    }
    let determinant = matrix[0] * (matrix[5] * matrix[10] - matrix[6] * matrix[9])
        - matrix[1] * (matrix[4] * matrix[10] - matrix[6] * matrix[8])
        + matrix[2] * (matrix[4] * matrix[9] - matrix[5] * matrix[8]);
    (determinant - 1.).abs() <= EPSILON
}
fn normalize_matrix(mut matrix: [f64; 16]) -> [f64; 16] {
    matrix[12..].copy_from_slice(&[0., 0., 0., 1.]);
    for scalar in &mut matrix {
        if *scalar == 0. {
            *scalar = 0.;
        }
    }
    matrix
}

fn resolve_carrier(
    response: &Value,
    group: &CarrierGroup,
    candidates: &mut [Candidate],
) -> Result<(), ApiFailure> {
    let rows = response.as_array().ok_or_else(operational)?;
    let mut index = HashMap::new();
    for row in rows {
        let row = object(row)?;
        let key = (
            source_string(row, "elementId")?,
            source_string(row, "microversionId")?,
            source_string(row, "partId")?,
        );
        if index.insert(key, row).is_some() {
            return Err(operational());
        }
    }
    let mut joined = Vec::new();
    for &p in &group.positions {
        let l = &candidates[p].leaf;
        let obj = *index
            .get(&(
                l.element_id.as_str(),
                l.document_microversion.as_str(),
                l.part_id.as_str(),
            ))
            .ok_or_else(|| operational().position(p))?;
        source_string(obj, "configurationId").map_err(|e| e.position(p))?;
        joined.push((p, obj));
    }
    let states = joined
        .iter()
        .map(|(p, obj)| solid_state(obj).map_err(|e| e.position(*p)))
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(i) = states.iter().position(|v| !*v) {
        return Err(unavailable(joined[i].0));
    }
    // Complete metadata projection precedes committing any private candidate data.
    let metadata = joined
        .iter()
        .map(|(p, obj)| {
            Ok((
                *p,
                string(obj, "name").map_err(|e| e.position(*p))?.to_owned(),
                string(obj, "description")
                    .map_err(|e| e.position(*p))?
                    .to_owned(),
            ))
        })
        .collect::<Result<Vec<_>, ApiFailure>>()?;
    for (p, name, description) in metadata {
        candidates[p].name = name;
        candidates[p].description = description;
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlanPayload<'a> {
    root: &'a SelectionRoot,
    authoring_document_identity: &'a str,
    objects: &'a [ResolvedSelectionObject],
}
#[derive(Serialize)]
struct Envelope<'a, T> {
    domain: &'a str,
    payload: T,
}
struct BoundedHash {
    digest: Sha256,
    count: usize,
    overflow: bool,
}
impl Write for BoundedHash {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_PLAN_BYTES.saturating_sub(self.count) {
            self.overflow = true;
            return Err(std::io::Error::other("selection plan bound"));
        }
        self.count += bytes.len();
        self.digest.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn construct_plan(
    root: SelectionRoot,
    candidates: Vec<Candidate>,
) -> Result<ResolvedSelectionPlan, ApiFailure> {
    let mut objects = Vec::new();
    for (p, candidate) in candidates.iter().enumerate() {
        objects.push(
            authoring::normalize_authoring_object(
                candidate.selector.clone(),
                &candidate.description,
                &candidate.name,
            )
            .map_err(|e| annotation_failure(e, Some(p)))?,
        );
    }
    let document = AuthoringDocument {
        schema_version: 1,
        objects,
    };
    authoring::validate_authoring_document(&document)
        .map_err(|e| annotation_failure(e, document_failure_position(&document)))?;
    let context = candidates
        .iter()
        .enumerate()
        .map(|(p, c)| {
            Ok(AuthoringContextEntry {
                selector: c.selector.clone(),
                plan_position: p,
                configured_part_identity: String::from_utf8(
                    cache_key::canonical_json_bytes(&c.leaf).map_err(|_| operational())?,
                )
                .map_err(|_| operational())?,
            })
        })
        .collect::<Result<Vec<_>, ApiFailure>>()?;
    authoring::validate_authoring_context(&document, &context)
        .map_err(|e| annotation_failure(e, context_failure_position(&document, &candidates)))?;
    let authoring_document_identity =
        authoring::authoring_document_identity(&document).map_err(|_| operational())?;
    let mut objects = Vec::new();
    for (candidate, authored) in candidates.into_iter().zip(document.objects) {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct ObjectIdentity<'a> {
            selector: &'a AuthoringSelector,
            configured_leaf: &'a ConfiguredLeaf,
        }
        let identity = cache_key::hash_json(
            "onshape-export-selection-plan-object-v1",
            &ObjectIdentity {
                selector: &candidate.selector,
                configured_leaf: &candidate.leaf,
            },
        )
        .map_err(|_| operational())?;
        objects.push(ResolvedSelectionObject {
            plan_local_object_identity: identity,
            selector: candidate.selector,
            configured_leaf: candidate.leaf,
            display_name: authored.display_name,
            annotation: authored.annotation,
            expected_neutral_placement_matrix: normalize_matrix(candidate.matrix),
        });
    }
    let envelope = Envelope {
        domain: "onshape-export-selection-plan-v1",
        payload: PlanPayload {
            root: &root,
            authoring_document_identity: &authoring_document_identity,
            objects: &objects,
        },
    };
    let mut hash = BoundedHash {
        digest: Sha256::new(),
        count: 0,
        overflow: false,
    };
    if serde_jcs::to_writer(&mut hash, &envelope).is_err() {
        return Err(if hash.overflow {
            unavailable(0)
        } else {
            operational()
        });
    }
    let plan_identity = hash
        .digest
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok(ResolvedSelectionPlan {
        root,
        authoring_document_identity,
        objects,
        plan_identity,
    })
}

#[cfg(test)]
#[path = "onshape_selection_tests.rs"]
mod integration_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request(kind: SelectionElementKind, ids: &[&str]) -> SelectionRequest {
        SelectionRequest {
            document_id: "d".into(),
            version_id: "v".into(),
            element_id: "root".into(),
            element_kind: kind,
            configuration_encoding: EncodingHandoff {
                source_hash: "a".repeat(64),
                config_hash: "b".repeat(64),
                encoding_context_hash: "c".repeat(64),
                encoded_id: "encoded;opaque=%2F".into(),
            },
            selectors: ids
                .iter()
                .map(|id| match kind {
                    SelectionElementKind::PartStudio => SelectionSelector::Part {
                        part_id: (*id).into(),
                    },
                    SelectionElementKind::Assembly => SelectionSelector::Occurrence {
                        occurrence_path: vec![(*id).into()],
                    },
                })
                .collect(),
        }
    }
    fn part(id: &str, e: &str, c: &str) -> Value {
        json!({"partId":id,"elementId":e,"microversionId":"m","configurationId":c,
            "bodyType":"solid","isFlattenedBody":false,"isMesh":false,"meshState":"NO_MESH","isHidden":false,
            "name":format!("Name {id}"),"description":""})
    }
    fn instance(id: &str, c: &str) -> Value {
        json!({"id":id,"type":"Part","suppressed":false,"documentId":"d","documentMicroversion":"m",
            "elementId":"leaf","fullConfiguration":c,"partId":"p"})
    }
    fn occurrence(id: &str) -> Value {
        json!({"path":[id],"hidden":false,"transform":IDENTITY_PLACEMENT})
    }
    fn assembly(ids: &[&str]) -> Value {
        json!({"rootAssembly":{"documentId":"d","documentMicroversion":"m","elementId":"root","fullConfiguration":"root-config",
            "instances":ids.iter().map(|id|instance(id,"leaf-config")).collect::<Vec<_>>(),
            "occurrences":ids.iter().map(|id|occurrence(id)).collect::<Vec<_>>(),"parametricInstances":[],"patterns":[]},
            "parts":[{"documentId":"d","documentMicroversion":"m","elementId":"leaf","fullConfiguration":"leaf-config","partId":"p","bodyType":"solid"}]})
    }
    fn kind<T>(result: Result<T, ApiFailure>) -> FailureKind {
        result.err().expect("expected typed failure").kind
    }
    fn asm_result(response: &Value) -> Result<AssemblyResolution, ApiFailure> {
        resolve_assembly(
            &request(SelectionElementKind::Assembly, &["a", "b"]),
            "m",
            response,
        )
    }

    #[test]
    fn request_shape_bounds_and_selector_kind_are_closed() {
        let valid = request(SelectionElementKind::Assembly, &["a"]);
        let mut value = serde_json::to_value(&valid).unwrap();
        SelectionRequest::from_json(&serde_json::to_vec(&value).unwrap()).unwrap();
        value["configurationEncoding"]["queryParam"] = json!("private");
        assert_eq!(
            kind(SelectionRequest::from_json(
                &serde_json::to_vec(&value).unwrap()
            )),
            FailureKind::InvalidSelection
        );
        let cases = [
            request(SelectionElementKind::PartStudio, &[]),
            request(SelectionElementKind::PartStudio, &["p", "p"]),
            request(SelectionElementKind::Assembly, &[""]),
        ];
        for case in cases {
            assert_eq!(kind(case.preflight()), FailureKind::InvalidSelection);
        }
        for length in [4096, 4097] {
            let mut case = valid.clone();
            case.document_id = "x".repeat(length);
            assert_eq!(case.preflight().is_ok(), length == 4096);
        }
        for value in [" ", "a b", "é", "a\n", "\u{7f}"] {
            let mut case = valid.clone();
            case.document_id = value.into();
            assert!(case.preflight().is_err());
        }
        let mut nested = valid.clone();
        nested.selectors = vec![SelectionSelector::Occurrence {
            occurrence_path: vec!["a".into(), "b".into()],
        }];
        assert!(nested.preflight().is_err());
        let mut mismatch = valid.clone();
        mismatch.selectors = vec![SelectionSelector::Part {
            part_id: "p".into(),
        }];
        assert!(mismatch.preflight().is_err());
        let mut upper = valid.clone();
        upper.configuration_encoding.source_hash = "A".repeat(64);
        assert!(upper.preflight().is_err());
        let mut count = request(SelectionElementKind::PartStudio, &[]);
        for n in 0..256 {
            count.selectors.push(SelectionSelector::Part {
                part_id: format!("p{n}"),
            });
        }
        count.preflight().unwrap();
        count.selectors.push(SelectionSelector::Part {
            part_id: "p256".into(),
        });
        assert!(count.preflight().is_err());
        assert_eq!(
            kind(SelectionRequest::from_json(
                br#"{"documentId":"d","documentId":"e"}"#
            )),
            FailureKind::InvalidSelection
        );
    }

    #[test]
    fn part_studio_phases_precedence_and_exact_identity() {
        let req = request(SelectionElementKind::PartStudio, &["p", "q"]);
        let mut rows = json!([
            part("p", "root", "returned-c"),
            part("q", "root", "returned-c")
        ]);
        let (r, candidates) = resolve_part_studio(&req, "m", &rows).unwrap();
        assert_eq!(r.configuration_identity, "returned-c");
        assert_eq!(candidates[0].matrix, IDENTITY_PLACEMENT);
        rows.as_array_mut().unwrap().reverse();
        let (_, reversed) = resolve_part_studio(&req, "m", &rows).unwrap();
        assert_eq!(reversed[0].leaf.part_id, "p");
        let absent = request(SelectionElementKind::PartStudio, &["absent"]);
        assert_eq!(
            kind(resolve_part_studio(&absent, "m", &rows)),
            FailureKind::UnavailableSourceState
        );
        let duplicate = rows[0].clone();
        rows.as_array_mut().unwrap().push(duplicate);
        assert_eq!(
            kind(resolve_part_studio(&absent, "m", &rows)),
            FailureKind::OperationalApiContractFailure
        );
        for key in [
            "elementId",
            "microversionId",
            "configurationId",
            "meshState",
            "isHidden",
        ] {
            let mut malformed = json!([part("p", "root", "c"), part("q", "root", "c")]);
            malformed[0]["bodyType"] = json!("sheet");
            malformed[1].as_object_mut().unwrap().remove(key);
            assert_eq!(
                kind(resolve_part_studio(&req, "m", &malformed)),
                FailureKind::OperationalApiContractFailure,
                "{key}"
            );
        }
        let mut unsupported = json!([part("p", "root", "c"), part("q", "root", "c")]);
        unsupported[0]["bodyType"] = json!("sheet");
        unsupported[0].as_object_mut().unwrap().remove("name");
        unsupported[1]
            .as_object_mut()
            .unwrap()
            .remove("description");
        assert_eq!(
            kind(resolve_part_studio(&req, "m", &unsupported)),
            FailureKind::UnavailableSourceState
        );
        let mut contradiction = json!([part("p", "wrong", "c"), part("q", "root", "c")]);
        assert_eq!(
            kind(resolve_part_studio(&req, "m", &contradiction)),
            FailureKind::OperationalApiContractFailure
        );
        contradiction[0]["elementId"] = json!("root");
        contradiction[1]["configurationId"] = json!("other");
        assert_eq!(
            kind(resolve_part_studio(&req, "m", &contradiction)),
            FailureKind::OperationalApiContractFailure
        );
    }

    #[test]
    fn assembly_early_gates_do_not_project_later_fields() {
        let mut generated = assembly(&["a", "b"]);
        generated["rootAssembly"]["patterns"] =
            json!([{"id":"pattern","seedToPatternInstances":{"a":["generated"]}}]);
        generated["rootAssembly"]["instances"][0] = json!({"id":"a"});
        generated["rootAssembly"]["occurrences"][0] = json!({"path":["a"]});
        assert_eq!(
            kind(asm_result(&generated)),
            FailureKind::UnavailableSourceState
        );
        let mut deleted = assembly(&["a", "b"]);
        deleted["rootAssembly"]["instances"][0] = json!({"id":"a","status":"DeletedElement"});
        assert_eq!(
            kind(asm_result(&deleted)),
            FailureKind::UnavailableSourceState
        );
        for t in ["Assembly", "Feature", "Unknown"] {
            let mut response = assembly(&["a", "b"]);
            response["rootAssembly"]["instances"][0] = json!({"id":"a","type":t});
            response["rootAssembly"]["instances"][1]
                .as_object_mut()
                .unwrap()
                .remove("partId");
            assert_eq!(
                kind(asm_result(&response)),
                FailureKind::UnavailableSourceState
            );
        }
        let mut suppressed = assembly(&["a", "b"]);
        suppressed["rootAssembly"]["instances"][0] =
            json!({"id":"a","type":"Part","suppressed":true});
        assert_eq!(
            kind(asm_result(&suppressed)),
            FailureKind::UnavailableSourceState
        );
        let mut hidden = assembly(&["a", "b"]);
        hidden["rootAssembly"]["occurrences"][0] = json!({"path":["a"],"hidden":true});
        hidden["rootAssembly"]["instances"][0]
            .as_object_mut()
            .unwrap()
            .remove("partId");
        assert_eq!(
            kind(asm_result(&hidden)),
            FailureKind::UnavailableSourceState
        );
        let mut cross = assembly(&["a", "b"]);
        cross["rootAssembly"]["instances"][0]["documentId"] = json!("external");
        cross["parts"] = json!([null]);
        assert_eq!(
            kind(asm_result(&cross)),
            FailureKind::UnavailableSourceState
        );
    }

    #[test]
    fn assembly_global_and_same_phase_malformed_data_wins() {
        let absent = request(SelectionElementKind::Assembly, &["absent"]);
        let mut duplicate = assembly(&["a", "a"]);
        assert_eq!(
            kind(resolve_assembly(&absent, "m", &duplicate)),
            FailureKind::OperationalApiContractFailure
        );
        duplicate = assembly(&["a", "b"]);
        duplicate["rootAssembly"]["occurrences"][1] = occurrence("a");
        assert_eq!(
            kind(resolve_assembly(&absent, "m", &duplicate)),
            FailureKind::OperationalApiContractFailure
        );
        let mut relation = assembly(&["a", "b"]);
        relation["rootAssembly"]["instances"][0]["type"] = json!("Assembly");
        relation["rootAssembly"]["parametricInstances"] =
            json!([{"id":"relation","children":[{"instanceIds":null}]}]);
        assert_eq!(
            kind(asm_result(&relation)),
            FailureKind::OperationalApiContractFailure
        );
        let mut types = assembly(&["a", "b"]);
        types["rootAssembly"]["instances"][0]["type"] = json!("Assembly");
        types["rootAssembly"]["instances"][1]["type"] = json!("future-type");
        assert_eq!(
            kind(asm_result(&types)),
            FailureKind::OperationalApiContractFailure
        );
        let mut association = assembly(&["a", "b"]);
        association["rootAssembly"]["occurrences"] = json!([occurrence("a")]);
        assert_eq!(
            kind(asm_result(&association)),
            FailureKind::OperationalApiContractFailure
        );
        for status in [json!(null), json!(true), json!("Unknown")] {
            let mut response = assembly(&["a", "b"]);
            response["rootAssembly"]["instances"][0]["status"] = status;
            assert_eq!(
                kind(asm_result(&response)),
                FailureKind::OperationalApiContractFailure
            );
        }
    }

    #[test]
    fn exact_joins_configuration_discrimination_and_sequential_groups() {
        let req = request(SelectionElementKind::Assembly, &["a", "b", "c"]);
        let mut response = assembly(&["a", "b", "c"]);
        response["rootAssembly"]["instances"][1]["fullConfiguration"] = json!("configuration-B");
        let mut second = response["parts"][0].clone();
        second["fullConfiguration"] = json!("configuration-B");
        response["parts"].as_array_mut().unwrap().push(second);
        let mut resolved = resolve_assembly(&req, "m", &response).unwrap();
        assert_eq!(resolved.carriers.len(), 2);
        assert_eq!(resolved.carriers[0].positions, vec![0, 2]);
        assert_eq!(resolved.carriers[1].positions, vec![1]);
        resolve_carrier(
            &json!([part("p", "leaf", "carrier-A-evidence")]),
            &resolved.carriers[0],
            &mut resolved.candidates,
        )
        .unwrap();
        resolve_carrier(
            &json!([part("p", "leaf", "carrier-B-evidence")]),
            &resolved.carriers[1],
            &mut resolved.candidates,
        )
        .unwrap();
        let plan = construct_plan(resolved.root, resolved.candidates).unwrap();
        assert_eq!(
            plan.objects[0].configured_leaf.configuration_identity,
            "leaf-config"
        );
        assert_eq!(
            plan.objects[1].configured_leaf.configuration_identity,
            "configuration-B"
        );
        assert_eq!(
            plan.objects[0].configured_leaf,
            plan.objects[2].configured_leaf
        );
        assert_ne!(
            plan.objects[0].plan_local_object_identity,
            plan.objects[2].plan_local_object_identity
        );
        let AuthoringSelector::AssemblyOccurrence {
            configuration_identity,
            ..
        } = &plan.objects[1].selector
        else {
            panic!()
        };
        assert_eq!(configuration_identity, "root-config");
        response["parts"] = json!([]);
        assert_eq!(
            kind(resolve_assembly(&req, "m", &response)),
            FailureKind::OperationalApiContractFailure
        );
    }

    #[test]
    fn carrier_exact_index_and_phase_precedence() {
        let mut resolved = asm_result(&assembly(&["a", "b"])).unwrap();
        let group = &resolved.carriers[0];
        assert_eq!(
            kind(resolve_carrier(&json!([]), group, &mut resolved.candidates)),
            FailureKind::OperationalApiContractFailure
        );
        assert_eq!(
            kind(resolve_carrier(
                &json!([part("p", "leaf", "c"), part("p", "leaf", "c")]),
                group,
                &mut resolved.candidates
            )),
            FailureKind::OperationalApiContractFailure
        );
        let mut hidden = part("p", "leaf", "c");
        hidden["isHidden"] = json!(true);
        hidden.as_object_mut().unwrap().remove("description");
        assert_eq!(
            kind(resolve_carrier(
                &json!([hidden]),
                group,
                &mut resolved.candidates
            )),
            FailureKind::UnavailableSourceState
        );
        let mut malformed = part("p", "leaf", "c");
        malformed.as_object_mut().unwrap().remove("description");
        assert_eq!(
            kind(resolve_carrier(
                &json!([malformed]),
                group,
                &mut resolved.candidates
            )),
            FailureKind::OperationalApiContractFailure
        );
    }

    #[test]
    fn leaf_states_and_part_joins_fail_without_carrier_scheduling() {
        for body in ["sheet", "composite"] {
            let mut response = assembly(&["a", "b"]);
            response["parts"][0]["bodyType"] = json!(body);
            assert_eq!(
                kind(asm_result(&response)),
                FailureKind::UnavailableSourceState
            );
        }
        let mut unknown = assembly(&["a", "b"]);
        unknown["parts"][0]["bodyType"] = json!("future");
        assert_eq!(
            kind(asm_result(&unknown)),
            FailureKind::OperationalApiContractFailure
        );
        for key in [
            "documentId",
            "documentMicroversion",
            "elementId",
            "fullConfiguration",
            "partId",
        ] {
            let mut nearby = assembly(&["a", "b"]);
            nearby["parts"][0][key] = json!("nearby");
            assert_eq!(
                kind(asm_result(&nearby)),
                FailureKind::OperationalApiContractFailure
            );
        }
        let mut duplicate = assembly(&["a", "b"]);
        let extra = duplicate["parts"][0].clone();
        duplicate["parts"].as_array_mut().unwrap().push(extra);
        assert_eq!(
            kind(asm_result(&duplicate)),
            FailureKind::OperationalApiContractFailure
        );
        let req = request(SelectionElementKind::PartStudio, &["p"]);
        for (field, value) in [
            ("bodyType", json!("surface")),
            ("isMesh", json!(true)),
            ("meshState", json!("MIXED")),
            ("meshState", json!("ALL_MESH")),
            ("meshState", json!("UNKNOWN")),
            ("isFlattenedBody", json!(true)),
            ("isHidden", json!(true)),
        ] {
            let mut row = part("p", "root", "c");
            row[field] = value;
            row.as_object_mut().unwrap().remove("name");
            assert_eq!(
                kind(resolve_part_studio(&req, "m", &json!([row]))),
                FailureKind::UnavailableSourceState
            );
        }
        let mut row = part("p", "root", "c");
        row["meshState"] = json!("FUTURE");
        assert_eq!(
            kind(resolve_part_studio(&req, "m", &json!([row]))),
            FailureKind::OperationalApiContractFailure
        );
        let mut row = part("p", "root", "c");
        row.as_object_mut().unwrap().remove("description");
        assert_eq!(
            kind(resolve_part_studio(&req, "m", &json!([row]))),
            FailureKind::OperationalApiContractFailure
        );
    }

    #[test]
    fn authoring_failures_are_atomic_private_and_positioned() {
        let req = request(SelectionElementKind::PartStudio, &["p", "q"]);
        let mut rows = json!([part("p", "root", "c"), part("q", "root", "c")]);
        rows[1]["description"] = json!(
            r#"onshape-export:v1 {"role":"supportBlocker","targets":["private-missing-target"]}"#
        );
        let (root, c) = resolve_part_studio(&req, "m", &rows).unwrap();
        let failure = construct_plan(root, c).unwrap_err();
        assert_eq!(failure.kind, FailureKind::UnavailableSourceState);
        assert_eq!(failure.diagnostics[0].selector_position, Some(1));
        assert!(
            !serde_json::to_string(&failure)
                .unwrap()
                .contains("private-missing-target")
        );
        let failure = annotation_failure(
            authoring::AnnotationError::Identity("private-internal-value".into()),
            Some(1),
        );
        assert_eq!(failure.kind, FailureKind::OperationalApiContractFailure);
        assert!(
            !serde_json::to_string(&failure)
                .unwrap()
                .contains("private-internal-value")
        );
        let mut resolved = asm_result(&assembly(&["a", "b"])).unwrap();
        resolve_carrier(
            &json!([part("p", "leaf", "carrier-evidence")]),
            &resolved.carriers[0],
            &mut resolved.candidates,
        )
        .unwrap();
        resolved.candidates[1].name = "Conflicting private name".into();
        let failure = construct_plan(resolved.root, resolved.candidates).unwrap_err();
        assert_eq!(failure.kind, FailureKind::UnavailableSourceState);
        assert_eq!(failure.diagnostics[0].selector_position, Some(0));
        let mut rows = json!([part("p", "root", "c"), part("q", "root", "c")]);
        for row in rows.as_array_mut().unwrap() {
            row["description"] =
                json!(r#"onshape-export:v1 {"role":"printable","key":"shared","targets":[]}"#);
        }
        let (root, c) = resolve_part_studio(&req, "m", &rows).unwrap();
        assert_eq!(
            kind(construct_plan(root, c)),
            FailureKind::UnavailableSourceState
        );
    }

    #[test]
    fn transforms_preserve_absolute_meter_values_and_only_normalize_zero_and_final_row() {
        let mut matrix = IDENTITY_PLACEMENT;
        matrix[3] = 1.3;
        matrix[7] = -2.5;
        matrix[11] = 0.08;
        matrix[1] = -0.;
        matrix[12] = -EPSILON;
        assert!(proper_rigid(&matrix));
        let normalized = normalize_matrix(matrix);
        assert_eq!(
            (normalized[3], normalized[7], normalized[11]),
            (1.3, -2.5, 0.08)
        );
        assert_eq!(normalized[1].to_bits(), 0f64.to_bits());
        assert_eq!(normalized[12], 0.);
        matrix[12] = -EPSILON.next_up();
        assert!(!proper_rigid(&matrix));
        let mut edge = IDENTITY_PLACEMENT;
        edge[1] = EPSILON;
        assert!(proper_rigid(&edge));
        edge[1] = EPSILON.next_up();
        assert!(!proper_rigid(&edge));
        for (index, value) in [(0, -1.), (0, 2.), (1, 0.1), (15, 0.), (3, f64::INFINITY)] {
            let mut bad = IDENTITY_PLACEMENT;
            bad[index] = value;
            assert!(!proper_rigid(&bad));
        }
        let mut rotation = IDENTITY_PLACEMENT;
        rotation[0] = 0.;
        rotation[1] = -1.;
        rotation[4] = 1.;
        rotation[5] = 0.;
        assert!(proper_rigid(&rotation));
        for transform in [json!(null), json!([1, 2]), json!("matrix")] {
            let mut response = assembly(&["a", "b"]);
            response["rootAssembly"]["occurrences"][0]["transform"] = transform;
            assert_eq!(
                kind(asm_result(&response)),
                FailureKind::OperationalApiContractFailure
            );
        }
        let mut nonrigid = assembly(&["a", "b"]);
        nonrigid["rootAssembly"]["occurrences"][0]["transform"][0] = json!(-1.);
        assert_eq!(
            kind(asm_result(&nonrigid)),
            FailureKind::UnavailableSourceState
        );
    }

    #[test]
    fn plan_golden_context_identity_scopes_and_array_order() {
        let configured = leaf("d", "m", "e", "c", "p");
        assert_eq!(
            String::from_utf8(cache_key::canonical_json_bytes(&configured).unwrap()).unwrap(),
            r#"{"configurationIdentity":"c","documentId":"d","documentMicroversion":"m","elementId":"e","elementKind":"part_studio","partId":"p"}"#
        );
        let req = request(SelectionElementKind::PartStudio, &["p", "q"]);
        let response = json!([part("p", "root", "c"), part("q", "root", "c")]);
        let build = |r: &Value| {
            let (root, c) = resolve_part_studio(&req, "m", r).unwrap();
            construct_plan(root, c).unwrap()
        };
        let original = build(&response);
        let mut renamed = response.clone();
        renamed[0]["name"] = json!("Renamed");
        let renamed = build(&renamed);
        assert_eq!(
            original.objects[0].plan_local_object_identity,
            renamed.objects[0].plan_local_object_identity
        );
        assert_ne!(
            original.authoring_document_identity,
            renamed.authoring_document_identity
        );
        assert_ne!(original.plan_identity, renamed.plan_identity);
        let reordered = json!([response[1].clone(), response[0].clone()]);
        assert_eq!(original, build(&reordered));
        let mut opposite = req.clone();
        opposite.selectors.reverse();
        let (root, c) = resolve_part_studio(&opposite, "m", &response).unwrap();
        let opposite = construct_plan(root, c).unwrap();
        assert_ne!(original.plan_identity, opposite.plan_identity);
        let payload = PlanPayload {
            root: &original.root,
            authoring_document_identity: &original.authoring_document_identity,
            objects: &original.objects,
        };
        assert_eq!(
            original.plan_identity,
            cache_key::hash_json("onshape-export-selection-plan-v1", &payload).unwrap()
        );
        assert_ne!(
            original.plan_identity,
            cache_key::hash_json("onshape-export-selection-plan-v1", &original).unwrap()
        );
        let mut missing = serde_json::to_value(&original).unwrap();
        missing.as_object_mut().unwrap().remove("planIdentity");
        assert!(serde_json::from_value::<ResolvedSelectionPlan>(missing).is_err());
    }

    #[test]
    fn relation_shapes_duplicates_and_exact_aggregate_limit() {
        for relations in [
            json!([{"id":"x","children":[]},{"id":"x","children":[]}]),
            json!([{"id":"x","children":[{"instanceIds":["y","y"]}]}]),
            json!([{"id":"x","children":[{"instanceIds":[],"seedOccurrence":""}]}]),
        ] {
            let root = json!({"parametricInstances":relations,"patterns":[]});
            assert!(relation_ids(object(&root).unwrap()).is_err());
        }
        let make = |generated: usize| {
            let lists = (0..4)
                .map(|seed| {
                    (
                        format!("seed{seed}"),
                        json!(
                            (0..generated / 4)
                                .map(|n| format!("id{seed}-{n}"))
                                .collect::<Vec<_>>()
                        ),
                    )
                })
                .collect::<Map<_, _>>();
            json!({"parametricInstances":[],"patterns":[{"id":"pattern","seedToPatternInstances":lists}]})
        };
        let exact = make(MAX_RELATION_IDS - 4);
        assert!(relation_ids(object(&exact).unwrap()).is_ok());
        let mut over = exact.clone();
        over["patterns"][0]["seedToPatternInstances"]["seed0"]
            .as_array_mut()
            .unwrap()
            .push(json!("extra"));
        assert!(relation_ids(object(&over).unwrap()).is_err());
    }

    #[test]
    fn bounded_plan_writer_aborts_before_over_limit_bytes() {
        let mut writer = BoundedHash {
            digest: Sha256::new(),
            count: 0,
            overflow: false,
        };
        writer.write_all(&vec![b'x'; MAX_PLAN_BYTES]).unwrap();
        assert_eq!(writer.count, MAX_PLAN_BYTES);
        assert!(writer.write_all(b"x").is_err());
        assert_eq!(writer.count, MAX_PLAN_BYTES);
        assert!(writer.overflow);
    }
}
