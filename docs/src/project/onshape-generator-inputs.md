# Onshape Generator Input Construction

Issue [#175](https://github.com/altendky/onshape-export/issues/175) connects the
successful [selection plan](onshape-selection-plans.md) and
[opaque geometry acquisition](onshape-geometry-acquisition.md) to unchanged
[protocol v1](neutral-generator-protocol.md) and
[generator settings v2](neutral-generator-settings-v2.md).
The implementation is `src/generator_inputs.rs`.

## Trusted Input And Complete Output

`construct_generator_inputs` consumes a complete successful acquisition and its
trusted planning handle. Before constructing objects it verifies the retained
plan, exact plan/provenance correspondence, binding order and cardinality,
configured leaves, request/result/download evidence, and content declarations.
Private acquisition-time snapshots additionally bind each original retention
reference and causal evidence identity. Mutating public handoff metadata,
swapping equal-byte files between leaves, or attaching another acquisition's
retention reference fails closed. This check performs no API requests or byte
reads and does not repeat source resolution or interpret geometry.

The constructor returns one read-only bundle containing:

- A complete available protocol-v1 manifest.
- Normalized settings v2 and their canonical bytes, document identity, and
  schema identity.
- One expected-placement summary entry per manifest object, in manifest order.
- One retained-payload reference per occurrence, accessible by manifest index.
- The complete ordered plan, provenance association, and causal acquisition
  evidence, retained for service use only.

Every occurrence survives independently, even when names, leaves, bytes,
SHA-256, length, or content identities are equal. Exact-leaf acquisition may
share a private retention reference; each occurrence still declares its own
unique protocol path and must be staged separately by
[#168](https://github.com/altendky/onshape-export/issues/168).

Failed or unavailable planning/acquisition has no successful handoff and cannot
produce this bundle. Constructor validation failure also returns no bundle or
partial manifest. Protocol-representation bounds fail closed; source strings
are never truncated, transformed, or substituted to fit a protocol field.

## Deterministic Retained Paths V1

The allocation version is exactly `1`. It consumes only the complete manifest
object-identity list in manifest order, containing 1 through 256 unique
lowercase SHA-256 hexadecimal identities. It does not inspect names, bytes,
content identities, acquisition directories, temporary paths, process state,
or traversal timing.

For zero-based index `i`, concatenate these ASCII bytes without whitespace:

1. The literal `inputs/geometry-v1/`.
2. `i` encoded as exactly three decimal digits, with leading zeroes: `000`
   through `255`.
3. One hyphen, `-`.
4. The object's exactly 64 lowercase hexadecimal identity bytes.
5. The literal `.3mf`.

For example, the first golden identity produces exactly:

```text
inputs/geometry-v1/000-1878d7de38529bdc2805623db939e9f834dbee189325add96b5d8a1237422c2f.3mf
```

No locale, escaping, normalization, additional separator, or trailing newline
participates. Each path is exactly 91 ASCII bytes. Duplicate object identities,
invalid identities, invalid cardinality, path collisions, and any output failing
the existing protocol-v1 safe-relative-path rules are rejected before returning
the bundle. Allocation does not create directories or files.

Reordering changes the index portion of each moved object's path. Changing an
object identity changes its identity portion. Renaming or changing temporary
workspaces does not affect allocation. Equal raw content never collapses paths.
`src/generator_inputs_tests.rs` freezes object-hash and path bytes, including
index `255`, and tests collision and unsafe-output rejection.

The versioned path participates in identities only as the existing
`retainedContent.path` field of each complete ordered protocol-v1 object. No
protocol-v1 hash domain or preimage is changed. Any future encoding change
requires a new allocation version and new frozen vectors.

## Identity And Policy Rules

New service identities use the existing RFC 8785/JCS UTF-8 and SHA-256 envelope
`{"domain":"domain-name","payload":...}`. The exact domains and payloads are:

| Identity | Domain | Payload |
| --- | --- | --- |
| Manifest-local object | `onshape-export-manifest-object-v1` | Object with only `planLocalObjectIdentity`, the exact existing plan-local identity |
| Retained content | `onshape-export-retained-geometry-content-v1` | Object with only `sha256` and integer `byteLength` |
| Causal mapping evidence | `onshape-export-geometry-acquisition-evidence-v1` | Complete serialized acquisition evidence, with every original field and value |
| Export observation | `onshape-export-geometry-observation-v1` | Ordered array of objects containing only `objectIdentity` and `evidenceIdentity` |

Object identity excludes name, role, annotation, placement, order, content,
plan/provenance identity, and translation attempt. Content identity excludes
path and occurrence. Mapping identity excludes authoring metadata and hashes
the causal evidence rather than the name-sensitive complete plan/provenance.
Observation identity preserves the ordered causal bindings.

The first object golden vector hashes the following envelope, whose string
value contains exactly 64 ASCII zeroes:

```json
{"domain":"onshape-export-manifest-object-v1","payload":{"planLocalObjectIdentity":"0000000000000000000000000000000000000000000000000000000000000000"}}
```

Its lowercase SHA-256 is
`1878d7de38529bdc2805623db939e9f834dbee189325add96b5d8a1237422c2f`.

Manifest `sourceIdentity` reuses `source-v2` over the validated root's resolved
source, with no linked document. `configurationIdentity` preserves the exact
planned root configuration identity; it is neither the original request
encoding nor cache `configHash`. Each object's optional `sourceObjectIdentity`
is the existing opaque plan-local identity. Source selectors, complete
occurrence paths, leaf records, configurations, and raw causal evidence stay in
service provenance. No source filename or parent relationship is invented.

`GeneratorInputPolicy` supplies reviewed requirements, input-kind,
input-schema, detected-kind, and parameterless protocol-media identities. These
values are validated and identity-bound, not discovered from filenames or raw
bytes. Their compatibility remains the static deployed-generator approval's
responsibility. Original transport media remains in acquisition evidence,
including its allowed charset parameter; it is not copied blindly into a
parameterless protocol media field.

`groupingPolicy: individual` describes one exact configured-part acquisition
per logical object. Mapping classification
`immutable-leaf-causal-acquisition-v1` describes the demonstrated causal binding.
Neither statement claims internal 3MF units, meshes, grouping, placement, or
validity. The service performs no raw 3MF parsing.

## Roles, Blockers, And Placements

The unchanged annotation role mapping supplies `rawGeometry` for `printable`
and `auxiliaryGeometry` for `supportBlocker`. The existing settings-v1 builder
rewrites authoring keys to manifest-local object identities, preserving blocker
and declared target order. Its membership, role, ambiguity, cardinality, and
failure rules remain unchanged. Blocker edges remain settings edges;
`parentObjectIdentity` is absent.

Each manifest object contributes exactly one settings-v2 placement and one
expected summary entry `(objectIdentity, transportRole,
expectedNeutralPlacementMatrix)`, in the same order. Both copy the corresponding
validated plan matrix directly: Part Studio identity placement, or Assembly
exact complete-path-matched absolute placement already converted to neutral
row-major meters by #173. Construction resolves no transforms and composes no
ancestors. Settings contain only manifest-local identities, unchanged blocker
data, and neutral matrices.

Renaming preserves source/object/content/mapping identity, paths, settings, and
settings identity, while changing display metadata, authoring/plan/provenance,
ordered input-set, and manifest identities. A moved occurrence retains its
object identity and matrix but changes path and ordered document identities.

## Validation And Runner Handoff

Construction serializes and parses the manifest through the existing
protocol-v1 validator, enforcing its document bounds as well as semantic and
identity rules. It validates settings v2 standalone and then invokes the pure
contextual validator from #185 against the independently constructed ordered expected
summary. Identity, role, order, cardinality, and normalized matrix scalar
mismatches fail closed.

When a later owner constructs a `GeneratorRequest`, it must call the bundle's
`validate_request` before dispatch. This rechecks document construction and
bounded protocol request validation, invokes `validate_with_manifest`, and
requires settings document/schema identities and canonical settings length and
SHA-256 to match the bundle. Declared manifest/settings paths must remain
distinct from every retained geometry path under unchanged protocol rules.

Issue #168 owns staging each occurrence at its declared path, independently measuring
staged bytes, invocation, and candidate/output validation. #175 creates no
filesystem layout, compatibility decision, processing recipe/cache entry,
generator process, publication, or UI route. Trusted generator raw-3MF
interpretation and final target-aware self-validation remain external.
