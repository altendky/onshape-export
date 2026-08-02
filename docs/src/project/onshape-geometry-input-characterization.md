# Onshape Geometry Input Characterization

> **Status: source-neutral characterization, not a production protocol.** This
> report records sanitized observations made on 2026-07-30. It defines
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

Two authorized, immutable versioned sources were observed through official
Onshape APIs:

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

## Unproven Cases

The available immutable sources did not contain every required adversarial
shape. The following remain unproven and must fail closed when a later request
depends on them:

| Case | Evidence status | Required conclusion |
| --- | --- | --- |
| Duplicate Part Studio display names | Unproven | Do not map by name. |
| Duplicate Assembly instance names | Unproven | Do not map by name. |
| Nested subassembly occurrence paths | Unproven | Do not flatten or synthesize hierarchy. |
| Hidden or suppressed occurrences | Unproven | Do not assume omission or inclusion semantics. |
| Missing or invalid occurrence selection | Unproven | Reject unless the selected path and result are verified. |
| Valid Part Studio `partIds` selection | Unproven; observed body IDs were rejected | Reject selected-part input for these endpoints. |
| Non-default configuration mapping | Unproven | Scope every mapping to explicit configuration identity. |
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

## Decision

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
