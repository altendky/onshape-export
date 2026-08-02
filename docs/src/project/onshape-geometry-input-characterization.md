# Onshape Geometry Input Characterization

> **Status: source-neutral characterization, not a production protocol.** This
> report records sanitized observations made on 2026-07-30 and 2026-08-02. It
> defines
> fail-closed requirements consumed by the neutral protocol, but it does not
> itself define a production schema, migration, or generator interface.

## Scope And Boundary

This report examines whether versioned Onshape Part Studio parts and Assembly
occurrences can be mapped deterministically to retained STEP, STL, raw Onshape
geometry 3MF, and grouped glTF/GLB preview results. GLB/glTF is included only as
an existing preview control, not as a promised generator input.

The work is source-neutral. No slicer archive, slicer implementation source,
target-derived schema, target fixture, generator, or generated slicer project
was inspected. The repository-root `AGENTS.md` boundary and the
[Slicer Project Generator Integration Policy](slicer-project-generator-integration.md#repository-ingress-boundary)
govern this work. Production `InputManifest` and `InputObject` definitions are
now provided by the [Neutral Generator Protocol](neutral-generator-protocol.md);
this report remains the source-neutral evidence and requirements input to that
contract.

The initial 2026-07-30 study observed two authorized, immutable versioned sources
through official Onshape APIs:

- `PS-01`: a configured Part Studio containing multiple solid bodies and a
  composite body.
- `ASM-01`: a configured root Assembly containing repeated references to some
  parts.

These aliases are report-local. Real document, version, element, part,
occurrence, translation, external-data, and content identities are not included.
No real model payload, response body, header value, filename, or content hash is
committed.

## Method And Evidence

The observations used the official body-details, Assembly-definition,
configuration, asynchronous export, translation-polling, and external-data
download APIs. Export requests explicitly selected `grouping=true` or
`grouping=false`, `storeInDocument=false`, and the existing repository format
settings. STEP used AP242; STL used binary encoding and `fine` resolution; raw
Onshape geometry 3MF used `fine` resolution. Grouped glTF/GLB used the existing
`FINE` preview setting.

Every whole-source export was run twice. Private evidence captured complete API
responses, response headers, exact payloads, content hashes, and archive
inventories outside the repository. The checked-in results retain only aliases,
counts, booleans, classifications, and conclusions. Full private evidence is
not required to build, test, or operate this repository.

The observations characterize only the named source versions and the API
behavior seen on the observation date. A documented API field is not treated as
proof of server behavior, and a matching count is not treated as an identity
mapping.

## Identity Layers

| Layer | Meaning | Identity rule |
| --- | --- | --- |
| Source | Versioned Part Studio or Assembly element | Resolve to document microversion, element, kind, and linked access context; do not use a display name. |
| Source object | Part or occurrence in one resolved source/configuration | Keep its source-scoped ID or ordered occurrence path; do not assume global or cross-microversion stability. |
| Display metadata | Part, instance, object, or file name | Optional and mutable; never an immutable key. |
| Translation | One asynchronous request lifecycle | `translationId` is diagnostic attempt identity, not content or source-object identity. |
| Translation result | One result slot returned by Onshape | Preserve result index and external-data/result-element IDs; do not infer correspondence between parallel arrays. |
| Retained content | Exact bytes downloaded by the service | Identify by a service-computed content hash and media/detected kind. |
| Input object | One declared neutral input in a later manifest | Requires an explicit, proven relation to retained content; use a manifest-local identity. |
| Ordered input set | Ordered collection passed to one invocation | Requires an explicit set identity over declared ordered members; archive order is insufficient. |

Translation and external-data IDs changed on every repeated request in this
study. They are therefore unsuitable as deterministic input identity. Exact
payload bytes were also not generally repeatable, so the retained-content hash
identifies bytes actually retained, not logical source equivalence.

## Source Metadata Observations

### Part Studio `PS-01`

- Body details returned three distinct body IDs: two solids and one composite.
- The body-details response did not provide a non-empty body name for any of
  those records. This does not prove that no display name exists elsewhere.
- The source exposed 18 configuration parameters.
- Whole-source individual STEP, STL, and geometry 3MF exports each contained two
  outer members, matching the solid count but not the body-record count. Count
  agreement alone does not establish which source body produced which member.
- Supplying the observed solid body-detail IDs as documented `partIds` was
  rejected with HTTP 400 for both STEP and geometry 3MF, for one-part grouped
  and two-part individual requests. The applicable part-selection identity is
  therefore unproven for this source and these endpoints.

### Assembly `ASM-01`

- The root definition returned 21 distinct instance IDs and 21 distinct
  one-segment occurrence paths.
- It returned 13 referenced-part records and no subassembly records.
- Some part references were repeated across occurrences.
- The observed source had no hidden occurrences, suppressed instances, missing
  instance names, duplicate instance names, or nested occurrence paths.
- The source exposed four configuration parameters.
- Whole-source individual STEP, STL, and geometry 3MF exports each contained 23
  outer members. This matches neither the 21 root occurrences nor the 13
  referenced-part records.
- None of the 23 member stems exactly matched any of the 21 observed instance
  display names.
- Selecting one root occurrence produced one grouped STEP payload and one
  grouped geometry 3MF payload. Selecting two root occurrences with individual
  output produced two outer members for both formats. This proves selection
  cardinality only for these cases; it does not prove a stable member-to-path
  identity mapping.

## Export Matrix

All completed rows returned exactly one `resultExternalDataId`, no
`resultElementIds`, and a `Content-Disposition`, `Content-Type`, and `ETag`
header. One external-data result can contain either direct bytes or an archive
with many members; result cardinality and object cardinality are independent.

| Case | Source | Format and mode | Observed retained shape | Mapping conclusion |
| --- | --- | --- | --- | --- |
| `PS-STEP-G` | `PS-01` | STEP grouped | Direct payload | Acceptable only as one opaque grouped geometry input. |
| `PS-STEP-I` | `PS-01` | STEP individual | ZIP with 2 STEP members | Per-part mapping unproven. |
| `PS-STL-G` | `PS-01` | STL grouped | Direct payload | Acceptable only as one opaque grouped geometry input. |
| `PS-STL-I` | `PS-01` | STL individual | ZIP with 2 STL members | Per-part mapping unproven. |
| `PS-3MF-G` | `PS-01` | Geometry 3MF grouped | One 3MF container | Acceptable only as one opaque grouped geometry input. |
| `PS-3MF-I` | `PS-01` | Geometry 3MF individual | ZIP with 2 geometry 3MF members | Per-part mapping unproven. |
| `PS-GLTF-G` | `PS-01` | glTF/GLB grouped | Direct binary payload | Preview control only; no per-part mapping. |
| `ASM-STEP-G` | `ASM-01` | STEP grouped | Direct payload | Acceptable only as one opaque grouped geometry input. |
| `ASM-STEP-I` | `ASM-01` | STEP individual | ZIP with 23 STEP members | Occurrence/part mapping rejected as ambiguous. |
| `ASM-STL-G` | `ASM-01` | STL grouped | Direct payload | Acceptable only as one opaque grouped geometry input. |
| `ASM-STL-I` | `ASM-01` | STL individual | ZIP with 23 STL members | Occurrence/part mapping rejected as ambiguous. |
| `ASM-3MF-G` | `ASM-01` | Geometry 3MF grouped | One 3MF container | Acceptable only as one opaque grouped geometry input. |
| `ASM-3MF-I` | `ASM-01` | Geometry 3MF individual | ZIP with 23 geometry 3MF members | Occurrence/part mapping rejected as ambiguous. |
| `ASM-GLTF-G` | `ASM-01` | glTF/GLB grouped | Direct binary payload | Preview control only; no occurrence mapping. |

### Geometry 3MF Container Observations

Inspection was limited to source-neutral 3MF container structure:

- Grouped `PS-01` geometry 3MF contained two objects and two build items. Neither
  object had a name.
- Each of the two individual `PS-01` geometry 3MF members contained one unnamed
  object and one build item.
- Grouped `ASM-01` geometry 3MF contained 16 objects and one build item. Twelve
  objects were named, and duplicate object names were present.
- Each of the 23 individual `ASM-01` geometry 3MF members contained one object
  and one build item. Nineteen objects were named and four were unnamed.

These counts differ from source part/occurrence counts in several ways. Object
names are incomplete or duplicated. Neither names nor 3MF object order proves a
source-object relationship.

## Repeatability

The second attempt used the same immutable source versions and explicit request
options:

- Translation IDs and external-data IDs changed for all 14 cases.
- Response `Content-Disposition` values remained equal for all 14 cases.
- Archive member path sequences remained equal for every ZIP/container case.
- Grouped STL and grouped glTF/GLB bytes were equal for both sources.
- STEP bytes changed in every grouped and individual case even where byte length
  remained equal.
- Individual STL package bytes changed even where byte length remained equal.
- Grouped and individual geometry 3MF bytes and lengths changed for both
  sources.

Stable filenames, lengths, member paths, or bytes in a small sample are useful
diagnostics but are not source-object identity guarantees. Nondeterministic raw
bytes also mean a new retained-content hash can represent the same logical
request without establishing semantic equivalence.

## Selected-Object Follow-Up

On 2026-08-02, authenticated follow-up trials tested whether an official Part
Studio part ID or complete Assembly occurrence path could cause exactly one
retained payload. The work reused `PS-01` and `ASM-01` and added `ASM-02`, a
disposable Free-account document containing two repeated subassembly references.
Each subassembly contained one available part and one suppressed part. The root
definition therefore exposed distinct two-segment paths to repeated available
and suppressed occurrences.

The follow-up used official part IDs from the parts endpoint rather than body
IDs from body details. Default and non-default configurations were encoded with
the official configuration-encoding endpoint. Each catalog version was resolved
to its immutable microversion before enumeration and export; the report-local
source alias binds the version, resolved microversion, element, kind, and access
context. Requests used the generic Part Studio or Assembly translation endpoint,
`grouping=true`, `storeInDocument=false`, notifications and automatic download
disabled, and an explicit millimeter unit request. STEP requested AP242, STL
requested binary fine output, and 3MF requested fine output. GLB was a preview
control. All real identifiers, raw requests and responses, support codes,
payloads, filenames, headers, and hashes remain outside the repository.

### Result Contract

For this study, one unique result means one completed translation lifecycle with
one unique nonempty external-data ID and no result-element IDs. The external-data
ID identifies the sole downloadable payload; it is not a second source-object
identity. A translation that fails before producing external data is a bounded
negative result, not an empty geometry input.

Every successful selected-object row below returned exactly one external-data
ID, no result-element IDs, and one downloadable payload. Every download response
used the generic `application/octet-stream` media type, so requested format and
response media type were insufficient validation by themselves.

| Case | Exact selector | Observed result | Causal conclusion |
| --- | --- | --- | --- |
| Official Part Studio part, STEP | One part ID from the configured parts response | One AP242 STEP payload | Proven for the observed part/configuration/request. |
| Official Part Studio part, STL | One part ID from the configured parts response | One structurally consistent binary STL payload | Proven for the observed part/configuration/request. |
| Official Part Studio part, geometry 3MF | One part ID from the configured parts response | One structurally valid 3MF payload | Proven for the observed part/configuration/request. |
| Official Part Studio part, GLB | One part ID from the configured parts response | One valid GLB v2 payload | Preview control proven for the observed part/configuration/request. |
| Root Assembly occurrence, STEP | One one-segment occurrence ID | One AP242 STEP payload | Proven for the observed root occurrence/configuration/request. |
| Root Assembly occurrence, STL | One one-segment occurrence ID | One structurally consistent binary STL payload | Proven for the observed root occurrence/configuration/request. |
| Root Assembly occurrence, geometry 3MF | One one-segment occurrence ID | One structurally valid 3MF payload | Proven for the observed root occurrence/configuration/request. |
| Root Assembly occurrence, GLB | One one-segment occurrence ID | One valid GLB v2 payload | Preview control proven for the observed root occurrence/configuration/request. |
| Non-default configured Part Studio part, geometry 3MF | One official part ID resolved under the non-default configuration | One structurally valid 3MF payload | Proven only for that explicit configuration; the observed part ID differed from default. |
| Non-default configured root Assembly occurrence, geometry 3MF | One root occurrence ID resolved under the non-default configuration | One structurally valid 3MF payload | Proven only for that explicit configuration. |
| Repeated root request, geometry 3MF | The same exact selector and options submitted again | One structurally valid payload with different translation, external-data, and byte identities | Logical request identity stayed fixed; attempt and retained-content identities changed. |
| Invalid Part Studio part ID | One nonexistent ID | Request rejected with HTTP 400 | Missing/stale part selection fails before retention. |
| Invalid root Assembly occurrence ID | One nonexistent ID | Request failed with a server error | Missing/stale occurrence selection fails before retention. |
| Pattern-generated repeated occurrence | A distinct one-segment occurrence ID referencing the same source as a successful seed occurrence | Translation failed with no translatable geometry | Repeated occurrences are not uniformly selectable by the documented field. |
| Nested available occurrence | Tail ID alone and an attempted serialized complete two-segment path | Both requests failed before translation creation | No supported complete ordered-path encoding was proven. |
| Nested suppressed occurrence | Suppressed tail ID | Request failed before translation creation | Suppressed selection cannot produce a valid input and must fail closed. |

The generic translation request schema describes `occurrencesToExport` as one
string containing comma-separated occurrence IDs. Assembly definitions instead
represent nested identity as an ordered array of instance IDs. The published
request contract does not define an encoding from that array to one nested
selection. Tail-only selection is also insufficient because the same tail can
occur beneath multiple repeated subassembly paths. The observed failures rule
out treating either tested representation as a proven complete-path selector.

### Payload Validation

- Selected 3MF payloads had intact ZIP containers, the expected OPC parts, one
  parseable primary model part, mesh and build elements, and an explicit `meter`
  model unit. The millimeter request option therefore must not be substituted
  for validation of the payload's declared unit. Full schema and mesh-topology
  conformance were not claimed.
- The observed Part Studio source object was an official composite part. Its
  selected 3MF contained two mesh objects and two build items; the selected root
  Assembly leaf contained one of each. Internal object counts do not change the
  causal request-to-payload relation and are not source identity evidence.
- Selected STL payloads satisfied the binary facet-count/byte-length relation,
  but STL carries no standard embedded unit. Any STL profile would need an
  external unit contract.
- Selected STEP payloads began with the ISO 10303-21 exchange-file marker and
  declared AP242 Edition 2. STEP preserves product geometry rather than
  providing the direct triangular manufacturing mesh required by the neutral
  MVP preference.
- Selected GLB payloads had valid glTF binary v2 headers. glTF remains a runtime
  asset/preview format rather than the preferred manufacturing-geometry
  boundary.

### Candidate Evaluation

| Candidate | Source-neutral suitability | Selected-object status | MVP decision |
| --- | --- | --- | --- |
| Geometry 3MF | Self-contained manufacturing container, explicit units, and constrained triangular meshes | Official parts and root occurrences succeeded; one generated repeated occurrence and the tested nested occurrences failed | Best neutral geometry candidate, but rejected because the required Assembly selector is unproven. |
| STL | Simple triangular geometry, but no embedded unit or object structure | Same Assembly selector limitation applies before format translation | Rejected. |
| STEP AP242 | Explicit product geometry and units, but requires downstream tessellation | Same Assembly selector limitation applies before format translation | Rejected. |
| GLB | Self-contained validated mesh/scene container with defined meters | Same Assembly selector limitation applies; retained as preview control | Rejected. |
| Other advertised export formats | No documented selector-bearing endpoint was found beyond the common generic translation request | Not trialed after that common selector failed the required nested cases | Rejected for this bounded MVP study, not claimed impossible in future APIs. |

## Unproven Cases

The available immutable sources did not contain every required adversarial
shape. The following remain unproven and must fail closed when a later request
depends on them:

| Case | Evidence status | Required conclusion |
| --- | --- | --- |
| Duplicate Part Studio display names | No exact duplicate-name source was exercised | This failed coverage criterion independently prevents profile selection; do not map by name. |
| Duplicate Assembly instance names | Repeated source names were automatically disambiguated by Onshape; no exact duplicate-name source was exercised | This failed coverage criterion independently prevents profile selection; do not map by name or generated suffix. |
| Nested subassembly occurrence paths | Complete paths were enumerated, but tested request representations failed | Reject nested selected-occurrence input. |
| Hidden or suppressed occurrences | Suppressed nested occurrence was enumerated and selection failed | Reject suppressed selected-occurrence input. |
| Missing or invalid occurrence selection | Invalid root and nested selections failed before retention | Preserve the failure; never broaden selection. |
| Valid Part Studio `partIds` selection | Proven for one official composite part under default and non-default configurations | Scope proof to the exact source/configuration/request. |
| Non-default configuration mapping | Proven for one selected Part Studio part and one root Assembly occurrence | Scope every mapping to explicit configuration identity. |
| Zero, multiple, or duplicate external-data IDs | Unproven live; synthetic runtime paths reject zero/multiple | Reject rather than selecting the first result. |
| Present or cardinality-mismatched result-element IDs | Unproven | Do not zip arrays or infer a sideband mapping. |
| Cross-microversion object stability | Unproven | Treat object IDs as resolved-source scoped. |
| Linked-document source context | Unproven | Require the complete authorized access context. |

## Fail-Closed Mapping Rules

1. Do not derive source/source-object identity or semantic equivalence from a
   display name, filename, array position, archive order, matching count, byte
   length, or coincidentally equal content hash.
2. Do not select the first translation result. A future protocol must treat
   zero, multiple, duplicate, empty, non-string, or otherwise malformed
   external-data IDs as making the requested mapping unavailable. The current
   parser is proven to reject zero or multiple parsed string IDs only.
3. Do not infer correspondence between `resultExternalDataIds` and
   `resultElementIds`; the official response shape does not declare such a
   relation.
4. Preserve an ordered occurrence path as source metadata. Do not replace it
   with an instance name or silently flatten nested hierarchy.
5. Treat a grouped export as one opaque geometry object only when the requested
   use does not require a part/occurrence mapping.
6. Treat each archive member as unmapped until a source-neutral sideband relation
   is proven for that exact source kind, endpoint, format, configuration, and
   grouping policy.
7. Keep observed filenames and object names as optional display metadata. Missing
   or duplicate names are valid observations, not reasons to synthesize names.
8. Compute retained-content identity from exact downloaded bytes. Keep logical
   request identity, retained-content identity, and ordered input-set identity
   separate.
9. Record `unproven`, `missing`, `duplicate`, or `ambiguous` explicitly and block
   generator input preparation when the requested relation depends on it.

## Manifest Requirements Consumed By Protocol V1

The following requirements were consumed by protocol v1. The normative field
names and schema are defined by the
[Neutral Generator Protocol](neutral-generator-protocol.md).

An `InputManifest` must declare:

- A protocol/requirements revision and manifest-local immutable identity.
- Resolved source and explicit configuration identity references.
- The selected source-neutral export kind, grouping policy, and observation
  status.
- An explicitly ordered list of `InputObject` records.
- Claimed hierarchy and occurrence relations by manifest-local identity.
- Mapping status and evidence classification for every required relation.
- A fail-closed result and reason when any required relation is unavailable.

An `InputObject` must declare:

- A manifest-local object identity and source-neutral role.
- Exactly one retained-content reference, content hash, media type, and detected
  kind when it represents bytes.
- Optional source-object, occurrence-path, translation-result, filename, and
  display-name metadata, each identified as provenance or display metadata
  rather than immutable object identity.
- Parent/occurrence links by manifest-local identity.
- Explicit `proven`, `unproven`, `missing`, `duplicate`, or `ambiguous` mapping
  status; no fallback identity may be synthesized.

### Synthetic Accepted Example

```text
manifest identity: manifest-synthetic-001
requirements revision: requirements-synthetic-v1
source reference: source-synthetic-001
configuration reference: configuration-synthetic-default
export kind: geometry-synthetic
grouping policy: grouped
observation status: proven-for-declared-opaque-use
ordered objects: [object-synthetic-001]

object-synthetic-001:
  role: geometry_input
  retained content: content-synthetic-a
  content hash: hash-synthetic-a
  media type: model/example
  detected kind: binary
  mapping: proven as one opaque grouped payload
```

### Synthetic Rejected Example

```text
manifest identity: unavailable
source reference: source-synthetic-002
configuration reference: configuration-synthetic-nondefault
translation result slots: [result-synthetic-a, result-synthetic-b]
source-object relation: ambiguous
decision: reject; do not select the first result or infer array correspondence
```

The examples use synthetic values and generic roles deliberately. Target
capabilities, dialects, schemas, fixtures, and validation facts remain outside
this repository. Production schema and protocol decisions are defined by
protocol v1.

## Prior Opaque-Payload Decision

For the tested source versions, endpoints, default configuration requests, and
retained shapes, the supported neutral boundary is one retained opaque grouped
geometry payload. It is identified by its exact retained-content hash and bound
to its resolved source, configuration, request, and processing identities. A
future retained shape still requires validation before use. The study does not
prove a general Part Studio part-to-member or Assembly occurrence-to-member
mapping for STEP, STL, geometry 3MF, or preview output.

Per-part/per-occurrence inputs, hierarchy preservation, semantic name/role
mapping, and multi-result support remain blocked until controlled evidence proves
the required sideband relationships. Protocol v1 carries this limitation
explicitly rather than hiding it through naming or ordering conventions.

## Selected-Object Profile Decision

No selected-object geometry input profile satisfies the MVP requirements as of
the 2026-08-02 observation. Official Part Studio part selection and flat root
Assembly occurrence selection can each cause one translation and one retained
payload for STEP, STL, geometry 3MF, and GLB. Geometry 3MF is the strongest
source-neutral geometry candidate among those observed.

The tested common Assembly translation request does not define or demonstrate
a complete ordered nested occurrence-path encoding. Tested nested and suppressed
selectors failed before translation creation, and a distinct generated repeated
occurrence failed even though its seed occurrence succeeded. Selecting geometry
3MF through that request therefore does not provide the required causal-selection
contract. Exact duplicate display-name coverage also remains incomplete.
Production selected-object planning, acquisition, manifest construction, and
generator dispatch must remain unavailable rather than falling back to a root
occurrence, tail ID, name, member order, count, or equal content. A future API
or documented selector-bearing endpoint can be characterized separately.
