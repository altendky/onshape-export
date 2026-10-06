# Onshape Selection Plans

> **Status: Normative MVP contract.** The selection planner resolves one
> version-rooted configured source into a complete immutable source-neutral
> plan. It does not acquire geometry, build generator inputs, or publish output.

The bounded carrier evidence and rejected direct Assembly translation-selector
conclusion in the
[characterization report](onshape-annotation-carrier-characterization.md) and
[#180](https://github.com/altendky/onshape-export/pull/180) remain preserved.
Resolving an exact Part Studio leaf does not prove that an Assembly translation
selector causally exports a complete occurrence path.

## Caller And Source Profile

The closed camelCase request contains exactly `documentId`, `versionId`,
`elementId`, `elementKind`, `configurationEncoding`, and `selectors`.
`elementKind` is `part_studio` or `assembly`. The closed encoding handoff is:

```json
{
  "sourceHash": "<lowercase SHA-256>",
  "configHash": "<lowercase SHA-256>",
  "encodingContextHash": "<lowercase SHA-256>",
  "encodedId": "<exact encodedId>"
}
```

Selectors are closed objects: `{"partId":"p"}` for a Part Studio, or
`{"occurrencePath":["i"]}` for an Assembly. Require 1-256 selectors with no
duplicate complete selector, the correct root kind, and exactly one Assembly
path segment. Every caller source string, including `encodedId`, is nonempty
visible ASCII and at most 4,096 bytes; hashes are lowercase 64-character SHA-256
values. Validate the complete request before any planner Onshape call. Unknown
fields, invalid bounds, and malformed selectors or handoffs are
`InvalidSelection`. The wider authoring-schema 1-64 path bound remains unchanged.

Supported sources are one same-document immutable configured Part Studio with
selected solid parts, or one flat same-document immutable configured Assembly
with ordinary solid `Part` instances at exact one-segment root paths. Repeated
occurrences remain separate objects. Caller selector order alone determines
object and authoring-document order.

External/linked documents, workspaces/latest, nested paths, subassemblies,
parametric or patterned instances, deleted/suppressed/hidden selections,
non-solid leaves, ambiguous state, and non-proper-rigid transforms are
unavailable. The pinned API exposes no field proving subassembly rigidity;
snapshot transform validity does not establish rigidity. Every selected
`Assembly` instance is therefore outside this profile. Names, tail IDs,
ordinals, flattened paths, response positions, and content equality never
select or substitute an object.

`configurationRequestValue` is exactly handoff `encodedId`. Preserve it without
parsing, prefix stripping, alias normalization, or reconstruction, and pass it
as the structured root `configuration` query value with exactly one HTTP
encoding pass. Request provenance is separate from every response-derived
`configurationIdentity`.

## Configuration Provenance

The pre-handoff coordinator alone owns `resolve_configuration_encoding` and an
encoding call on a true active-cache miss. Planning never encodes or repairs
evidence.

Parse the trusted configured API base as an absolute HTTPS URL with a host and
without user information, query, or fragment. Canonical origin is `https://`
plus the parser's canonical ASCII host and a port only when it is not 443.
Host case and explicit 443 normalize identically; base path is excluded. Resolve
no DNS for identity. Production origin must equal static trusted
`https://cad.onshape.com` before any Onshape request. A caller, response, cache
row, or DNS answer cannot establish origin trust. Build operations from that
origin and exact absolute paths.

`sourceHash` retains domain `source-v2` over exact document, resolved
microversion, element, kind, and `linkDocumentId: null`, excluding caller version.
`configHash` retains `config-v2` over canonicalization version 2, `sourceHash`,
parameter-schema version 4, and exact typed canonical configuration values.

`encodingContextHash` is SHA-256 over RFC 8785/JCS UTF-8 bytes of
`{"domain":"onshape-export-configuration-encoding-context-v1","payload":<context>}`.
The exact closed context is:

```json
{
  "contextSchemaVersion": 1,
  "apiOrigin": "https://cad.onshape.com",
  "apiMajorVersion": 16,
  "openApiSha256": "eadcea471568f8737f51fffe794dae51b72c35a700d4a7ac11c98fd345ef2eed",
  "operationId": "encodeConfigurationMap",
  "method": "POST",
  "pathTemplate": "/api/v16/elements/d/{did}/e/{eid}/configurationencodings",
  "documentId": "<exact documentId>",
  "versionId": "<exact caller versionId>",
  "resolvedMicroversionId": "<exact resolved microversionId>",
  "elementId": "<exact elementId>",
  "elementKind": "<part_studio or assembly>",
  "linkDocumentId": null,
  "query": {"versionId": "<exact caller versionId>"},
  "configCanonicalizationVersion": 2,
  "parameterSchemaVersion": 4
}
```

Both version values are equal. `query` proves the sole query parameter;
`linkDocumentId: null` requires HTTP omission. Body, typed values, hashes,
encoded ID, response, timestamps, retry count, and cache metadata are excluded.
Same-microversion version aliases retain equal source/config hashes but have
distinct encoding contexts.

The only encoding operation is `POST
/api/v16/elements/d/{did}/e/{eid}/configurationencodings` with sole query
`versionId`, exact canonical parameter body, and `Content-Type: application/json`.
Use no workspace, microversion query, alternate operation, or unversioned path.

The active key is `(sourceHash, configHash, encodingContextHash)`. Persist full
canonical context, canonical request and strict response evidence, exact
`encodedId`, and `queryParam` evidence needed by other callers. On every hit,
strictly decode all evidence with duplicate rejection, recompute hashes from
current trusted typed inputs, require exact key/context and canonical request
equality, and require response `encodedId` to equal the persisted bounded
nonempty value. Validate `queryParam` for callers that need it; it never enters
the planning handoff.

Only an absent active row is a miss. A present stale, malformed, missing, unequal,
or contradictory value is `OperationalApiContractFailure` with zero encoding
calls. A miss permits exactly one encoding call; validate all evidence before
insert or handoff. Insert once, never overwrite. If another insert wins, reread
and fully validate the winner; contradictory evidence fails.

The transactional one-time migration retains the old two-key table separately
as legacy storage and rebuilds the active table with required
`encoding_context_hash`, `encoding_context_json`, and a triple primary key.
Copy no legacy row, infer no context, and reuse no legacy encoded ID. Active
read/insert paths require all three keys. Unrelated source, selection, export,
artifact, and generator records remain intact.

The coordinator first resolves version and trusted source/context, then returns
the handoff from a validated active hit or one validated encoding call. Planning
independently resolves the version exactly once with `GET
/api/v16/documents/d/{documentId}/versions/{versionId}?parents=false`, with no
other query. Require exact bounded `BTVersionInfo.documentId`, `id`, and
`microversion`, caller equality, and no historical `microversionId` alias.
Recompute source/context identity and validate handoff `configHash` binding and
all active context/request/response evidence through the hardened resolver
before reading the configured root. Typed `config-v2` recomputation belongs to
the coordinator/cache path. Missing, forged, cross-version, cross-origin,
cross-schema, or otherwise unequal provenance is operational; planning never
falls back, parses `queryParam`, substitutes a legacy row, or retries.

Existing preview/download exports have a scoped exception to the coordinator's
fresh-version ordering: they may reuse a fully validated active encoding hit
against the exact version-specific cached source resolution and current typed
values. This keeps ready artifacts available during a version-endpoint outage.
Every source field must match the catalog source; all hashes and stored evidence
are revalidated, including `queryParam`. Invalid evidence fails without a network
request. A missing cached source resolution or active row enters the fresh
coordinator; only a true active triple-key miss permits its single encoding
attempt. This exception does not apply to public planning-handoff coordination
or the planner's independent version read.

## Transport And Projection

Pin the reviewed OpenAPI 3.0.1 artifact from `onshape-mcp-io` 0.5.2 at
[release commit 3bd1bf698818ade4ab286f0e3a7cc57289114b91](https://github.com/altendky/onshape-mcp/blob/3bd1bf698818ade4ab286f0e3a7cc57289114b91/crates/onshape-mcp-io/onshape-openapi.json).
Its `info.version` is `1.216.80836-7d2542b69551`, server is
`https://cad.onshape.com/api/v16`, and exact artifact SHA-256 is
`eadcea471568f8737f51fffe794dae51b72c35a700d4a7ac11c98fd345ef2eed`.
Changing API version, artifact identity, enum acceptance, or projection requires
a reviewed baseline update and compatibility tests. Use explicit `/api/v16`
for every coordinator and planner request.

Every operation has one attempt, zero redirects, 10-second connect and read-idle
timeouts, and a 60-second total deadline covering DNS through streaming. Send
`Accept: application/json` and `Accept-Encoding: identity`; disable automatic
decompression. There is no internal retry, backoff, alternate host, credential
refresh, or second attempt. Outer job retries are distinct executions and reuse
only normally validated prior committed evidence.

TLS retains every parsable certificate from the native trust store and fails
if none can be added. Successful TLS configuration is shared for the process
lifetime; native certificate additions or removals require a process restart.
Initialization failures are not cached. Certificate and hostname verification
remain required.

Require HTTP 200. Once status exists, 401/403 remains `AuthenticationFailure`
even with malformed diagnostics; other non-200/redirect responses are
operational. DNS, connection, TLS, timeout, premature EOF, interrupted transfer,
and framing failures are `TransportFailure`. Successful media type is
`application/json`, charset absent or UTF-8, with absent or exact `identity`
content encoding. Other valid media parameters are accepted.

Absent `Content-Length` is accepted. A present length must be exactly one
nonnegative decimal `u64`; reject duplicate fields, comma lists, signs,
malformed/overflow values, and declarations above the operation ceiling.
Independently count encoded bytes and abort at limit plus one. After complete
delivery, require exact declared/actual equality; incomplete framing remains
transport failure. Retain at most 65,536 diagnostic bytes, and never replace
status-derived classification with a diagnostic defect.

| Response | Encoded byte ceiling |
| --- | ---: |
| Version or encoding | 65,536 |
| Parts/carrier | 33,554,432 |
| Assembly | 16,777,216 |

Validate complete UTF-8 JSON with duplicate-member rejection before projection,
including unknown fields: at most 32 nested containers, 4,096 entries per array,
256 members per object, and 262,144 total object members. Non-success JSON
diagnostics use the same structural bounds; text diagnostics require UTF-8.

Requiredness is phase-local. Validate every reached required field for the whole
phase before evaluating unavailable predicates. Malformed required data wins
within that phase; a validated unavailable phase stops all later projections,
joins, and calls at the lowest affected caller position. Unreached later fields
are unconsumed. Unknown unconsumed fields are additive; unknown consumed enums,
historical aliases, and synthesized defaults fail operationally. Use staged
projections rather than one eager Assembly DTO. Request DTOs are closed.

## Part Studio Resolution

Read exactly one `GET /api/v16/parts/d/{did}/m/{mid}/e/{eid}` with root handoff
`configuration`, `withThumbnails=false`, `includePropertyDefaults=false`, and
`includeFlatParts=false`; omit `linkDocumentId`.

1. Validate the complete array of objects, then every bounded visible-ASCII
   `partId`. Any duplicate is operational, including identical rows.
2. Resolve exact selected IDs. After index validation, absent IDs are stale
   unavailable selections.
3. Validate selected `elementId`, `microversionId`, and `configurationId`.
   Element/microversion contradiction is operational; selected nonempty
   configuration identities must all be exactly equal.
4. Validate selected `bodyType`, `isFlattenedBody`, `isMesh`, `meshState`, and
   `isHidden`. Only exact `solid`/false/false/`NO_MESH`/false continues; other
   well-typed supported enum states are unavailable.
5. Only survivors require string `name` and `description` for authoring.

## Flat Assembly Resolution

Read exactly one `GET /api/v16/assemblies/d/{did}/m/{mid}/e/{eid}` with root
handoff `configuration`, `includeMateFeatures=false`, `includeNonSolids=false`,
`includeMateConnectors=false`, and `excludeSuppressed=false`. Omit linked context,
`explodedViewId`, and other queries.

Apply these phases in order, validating the complete reached phase first:

1. Require one `rootAssembly`, a `parts` array, exact root `documentId`,
   `documentMicroversion`, `elementId`, and bounded `fullConfiguration`, plus
   `instances`, `occurrences`, `parametricInstances`, and `patterns` arrays.
   Root/request contradiction is operational. Ignore `partStudioFeatures` and
   `subAssemblies`; never traverse them.
2. Index every instance `id` and occurrence complete `path`; duplicate IDs or
   paths are operational. Do not yet require type, visibility, or transform.
3. Fully validate all relations: parametric `id`, `children`, child
   `instanceIds`, optional nonempty `seedOccurrence`, pattern `id`, and
   `seedToPatternInstances`. Exclude parametric IDs, child IDs, seeds, pattern
   map keys, and generated IDs. Reject relation duplicates/malformed values;
   the aggregate relation bound is 16,384 entries across all these categories.
4. Each selector must have both one exact instance and one exact `[instanceId]`
   occurrence, or neither. Asymmetry is operational; absence of both is stale
   unavailable state after all associations validate.
5. A selected exclusion-set member is unavailable before any later fields.
6. Optional `status` absence passes; `DeletedElement` is unavailable, and
   null/wrong/unknown status is operational.
7. Validate `type`; only `Part` continues. `Assembly`, `Feature`, and `Unknown`
   are unavailable; unrecognized values are operational.
8. Require Boolean `suppressed`; true is unavailable.
9. Require occurrence Boolean `hidden`; true is unavailable.
10. Require selected instance document, microversion, element,
    `fullConfiguration`, and `partId`. Document/microversion mismatch is
    unavailable before part projection or carrier calls.
11. Validate the exact selected transform as described below.
12. Index every Assembly `parts` entry by exact document, microversion, element,
    `fullConfiguration`, and `partId`. Reject duplicates; require one exact
    five-field match per survivor. Missing, partial, nearby, or unequal matches
    are operational. Joined pinned `bodyType` `sheet` or `composite` is
    unavailable; unknown values are operational.
13. Deduplicate surviving carrier keys `(documentId, documentMicroversion,
    elementId, fullConfiguration)` and issue them sequentially in first caller
    appearance order. Finish each before the next; first failure wins.

Each carrier uses the same microversion parts endpoint and explicit false query
defaults as the Part Studio read, with exact joined `fullConfiguration` passed
unchanged as structured `configuration` with one encoding pass. Require every
row's bounded `elementId`, `microversionId`, and `partId`, index exact triples,
reject duplicates, and require exactly one selected join. Absence is operational.
Require nonempty carrier `configurationId` as evidence only; never compare or
substitute it for `fullConfiguration`.

Validate all joined carrier solid/mesh/flattened/hidden fields before unavailable
classification, with the same strict-solid values as Part Studio. Only survivors
require `name` and `description`. Occurrence visibility comes from occurrence
`hidden`; carrier `isHidden=false` is an additional conservative bound.
Unsupported or mismatched selections cause no carrier call. Equivalent API
array reordering changes neither joins, exclusions, object order, nor identities.

## Leaves And Placement

The exact configured-leaf key contains `documentId`, `documentMicroversion`,
`elementId`, `elementKind: "part_studio"`, `configurationIdentity`, and `partId`.
For Part Studio roots, root and leaf identities are selected metadata
`configurationId`. Assembly root and authoring selector identity are root
`fullConfiguration`; leaf identity is exact joined Assembly-part
`fullConfiguration`. Every surviving leaf reports the exact root document and
microversion. Preserve configuration strings without interpreting or equating
their distinct request/response spaces. Version alias, occurrence, name,
metadata, request representation, and content identity are absent from leaf
keys. Equal leaf keys never collapse occurrences.

Part Studio placement is exact 4x4 identity. Assembly placement comes only from
the occurrence whose complete path equals the exact one-segment selector. Use
the pinned [official absolute Assembly transform contract](https://github.com/onshape-public/onshape-public.github.io/blob/c9cc2e8e86e2c304a638e670bbd56c64ae79b1db/docs/api-adv/assemblies/index.html)
once: row-major object-to-world matrix, column-vector neutral output, meters.
Copy translations unchanged; do not transpose, compose ancestors, rescale, or
apply geometry/target-derived transforms.

Require exactly 16 finite binary64 values; malformed projection is operational.
With inclusive absolute tolerance `1e-12`, require affine final row
`[0,0,0,1]`, every entry of `R^T R` equal to identity, and `det(R)` equal to 1.
A complete finite matrix failing these checks is unavailable. Reflection,
scale, and shear do not pass. Do not repair, clamp, snap, or orthogonalize.
After validation, set only the affine final row exactly, normalize every signed
zero to positive zero, and preserve every other scalar. Return this matrix as
`expectedNeutralPlacementMatrix`.

## Authoring And Identities

Capture exact decoded selected carrier Name and Description after source-state
phases; use no instance name, Assembly metadata, or synthesized Description.
Run the [authoring convention](onshape-annotation-convention.md) standalone and
contextual validators in caller order before assigning object identities.
Preserve display name, annotation role/key, and target order. Malformed required
metadata is operational; source-caused parser/validation rejection makes the
complete plan unavailable. Sanitize validator diagnostics.

Part Studio authoring selectors use selected metadata `configurationId`.
Assembly selectors use root source context and root `fullConfiguration`, with
the exact one-segment path. Entry context binds selector, zero-based position,
and `configuredPartIdentity`: direct JCS UTF-8 serialization of the closed leaf
key interpreted as an opaque string, without hash, domain, prefix, or encoding.
The synthetic golden string is:

```text
{"configurationIdentity":"c","documentId":"d","documentMicroversion":"m","elementId":"e","elementKind":"part_studio","partId":"p"}
```

Hash identities as lowercase SHA-256 of JCS UTF-8
`{"domain":"<domain>","payload":<payload>}` after signed-zero normalization:

| Identity | Domain | Exact payload |
| --- | --- | --- |
| Plan-local object | `onshape-export-selection-plan-object-v1` | `selector`, `configuredLeaf` |
| Complete plan | `onshape-export-selection-plan-v1` | `root`, `authoringDocumentIdentity`, ordered `objects` |

The returned plan contains exactly `root`, `authoringDocumentIdentity`,
`objects`, and required `planIdentity`. Root contains exactly document, caller
version, resolved `documentMicroversion`, root element, kind, and
response-derived `configurationIdentity`. Each object contains exactly
`planLocalObjectIdentity`, `selector`, `configuredLeaf`, `displayName`,
`annotation`, and normalized `expectedNeutralPlacementMatrix`.
`planIdentity` itself is absent from its hash preimage. All encoding provenance
and request representations are absent from the plan and plan hash.

Limit the complete three-field plan hash envelope to 8,388,608 JCS UTF-8 bytes;
count while hashing and abort before retaining byte limit plus one. Source-caused
overflow is unavailable; bounded typed canonicalization failure is operational.
The returned fourth field does not enter this ceiling.

Object identity excludes position, display, annotation, keys, placement, and
acquisition; distinct occurrence selectors distinguish shared leaves. Every
included field and object/target order affects complete plan identity. Rename
changes display metadata, authoring-document, complete-plan, protocol
input/manifest, invocation, and cache identities; it preserves source, leaf,
plan-local/manifest-local object, retained-content, causal-mapping, and settings
identities, as well as settings content.

## Atomic Outcomes And Ownership

Return one complete plan or one typed failure:

| Outcome | Meaning |
| --- | --- |
| `InvalidSelection` | Malformed caller request/handoff; zero planner calls |
| `AuthenticationFailure` | HTTP 401/403, preserving status classification |
| `TransportFailure` | Failed or incomplete HTTP transfer |
| `OperationalApiContractFailure` | Invalid provenance, evidence, reached API contract, or internal invariant |
| `UnavailableSourceState` | Well-formed source outside the profile, stale absence, authoring rejection, or plan overflow |

Barriers are preflight, independent version read, provenance validation,
configured-root HTTP/JSON validation, ordered phase resolution, sequential
carriers, then authoring and identities. Operational failure never degrades to
absence or unsupported state. Return no partial plan, skipped selector,
warning-plus-plan, reusable partial identity, or partial authoring document.

Diagnostics retain operation and selector position where known, never secrets,
raw bodies, encoding payloads, private source strings, fixture identifiers, or
unbounded upstream text. Limit to 64 entries; codes to 128 lowercase ASCII code
characters, messages to 2,048 characters, JSON Pointers to 1,024 characters, and
16 unique context entries with visible-ASCII keys of at most 64 and values of at
most 512 characters.

[#174](https://github.com/altendky/onshape-export/issues/174) consumes only
successful plans. It uses root document/version and exact planned leaves,
independently resolves that reused version before acquisition, and requires its
microversion to equal every consumed leaf. Mismatch is unavailable with zero
acquisition calls. It owns causal raw Geometry 3MF request/result/payload
evidence, without rediscovering metadata, selectors, configurations, transforms,
or order.

The [configured-leaf acquisition characterization](onshape-geometry-acquisition.md)
separately establishes the translation-body binding for each response-derived
leaf configuration space. Successful planning and carrier-query discrimination
alone do not authorize geometry acquisition. Unproven bindings remain
unavailable without changing these plan identities.

Issue #174's [trusted acquisition-provenance contract](onshape-geometry-acquisition.md#trusted-acquisition-provenance-contract)
specifies a service-owned wrapper that retains the exact original encoding
handoff/context and complete successful return from the same planning invocation.
It atomically records that association outside every existing #173 hash scope;
the planner's request, returned plan, and identity preimages remain unchanged.
A separately supplied valid encoding or self-computed sidecar hash cannot prove
the invocation relationship. Acquisition uses trusted lookup and complete
read-only validation before the independent version barrier and any create.
Missing or contradictory provenance is operational; only a well-formed
independently resolved snapshot mismatch or rejected/unproven binding is
unavailable. The maintainer approved reviewing the contract and implementation
together in #310, overriding the separate contract-merge gate while preserving
all binding and verification requirements.

[#175](https://github.com/altendky/onshape-export/issues/175) consumes successful
plans and acquisition inputs. It owns manifest identities/order, deterministic
retained paths, manifest/settings construction, role rewrite, placement
summaries, and protocol/contextual validation. It preserves planned matrices;
[#168](https://github.com/altendky/onshape-export/issues/168) stages declared
invocation paths. Generator implementation and target-aware validation remain
outside this repository under the
[integration policy](slicer-project-generator-integration.md).

Tests and examples are synthetic and source-neutral. This work uses no live
fixtures, authenticated characterization, raw geometry parsing, target source,
target-derived fixtures, acquisition, generator execution, upload, publication,
or UI. Existing bounded controlled carrier observations establish only the
recorded same-document flat-profile request/response discrimination, with no
cross-configuration identity equivalence or broader availability claim.
