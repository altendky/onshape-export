# Configured-Leaf Geometry 3MF Acquisition

> **Status: #174 phase 1 provenance contract submitted for review; production
> acquisition remains unimplemented.**
> Bounded characterization in
> [#308](https://github.com/altendky/onshape-export/issues/308) rejected Part
> Studio direct binding and demonstrated the tested Assembly-leaf binding and
> original Part Studio encoding handoff. The contract below defines trusted
> recording and consumption of that handoff. Review and merge of the phase 1
> contract PR must precede phase 2 implementation; this PR does not close #174
> or enable production support.

## Scope And Ownership

Consume only successful [selection plans](onshape-selection-plans.md). Supported
candidate sources are same-document, immutable-version-rooted, configured
strict-solid Part Studios and flat Assemblies containing ordinary visible,
unsuppressed one-segment Part occurrences. Every acquisition addresses a Part
Studio leaf. Nested, linked, cross-document, flexible-subassembly,
pattern-created, reflected, and non-solid cases have no support claim.

The planner owns resolution, response-derived configured-leaf identities,
selectors, metadata, annotation, placement, and order. Acquisition owns exact
opaque bytes and the causal request/result/download evidence. #175 owns
deterministic protocol retained-path allocation, manifest/settings construction,
and validation. #168 stages those declared paths in an invocation directory.
Acquisition temporary filenames never allocate protocol paths.

Repeated occurrences retain separate ordered bindings, even when one exact
configured-leaf acquisition is reused. Content equality does not establish leaf
equality. Names, response positions, archive members, and hashes do not establish
source-object or configuration equivalence.

## API Baseline And Request

Use the exact reviewed OpenAPI 3.0.1 artifact from `onshape-mcp-io` 0.5.2 at
[commit 3bd1bf698818ade4ab286f0e3a7cc57289114b91](https://github.com/altendky/onshape-mcp/blob/3bd1bf698818ade4ab286f0e3a7cc57289114b91/crates/onshape-mcp-io/onshape-openapi.json).
Its SHA-256 is
`eadcea471568f8737f51fffe794dae51b72c35a700d4a7ac11c98fd345ef2eed`,
`info.version` is `1.216.80836-7d2542b69551`, and server is
`https://cad.onshape.com/api/v16`. No newer live artifact is implicitly adopted.

The baseline's `createPartStudioTranslation` admits workspace or version paths,
not a microversion translation path. The only candidate acquisition is:

```text
POST /api/v16/partstudios/d/{root.documentId}/v/{root.versionId}/e/{leaf.elementId}/translations
```

The query set is empty. The JSON object contains exactly:

```json
{
  "formatName": "3MF",
  "storeInDocument": false,
  "notifyUser": false,
  "triggerAutoDownload": false,
  "configuration": "<exact candidate configuration request value>",
  "partIds": "<exact planned partId>",
  "grouping": true,
  "resolution": "fine"
}
```

`partIds` is a string containing one exact official part ID, not an array or a
joined collection. Preserve the candidate configuration's decoded string in
JSON without parsing, normalization, prefix removal, or extra URL encoding.
Serialize once as compact UTF-8 JSON; ordinary JSON escaping preserves its
decoded string. Encode each path segment once. Do not send a configuration query
or duplicate source/element/version selectors in the body.

Every other translation field is omitted, including `linkDocumentId`,
`linkDocumentWorkspaceId`, `occurrencesToExport`, `elementId`, `elementIds`,
`unit`, `specifyUnits`, export-rule flags, destination/filename fields,
tolerances, cloud-storage fields, and format-specific STEP/STL options. Requested
format does not establish internal units or internal 3MF validity. `grouping:
true` requests one opaque selected-part payload; internal grouping is not a
service-side rejection criterion.

The pinned `configuration` description and
[official configuration guide](https://onshape-public.github.io/docs/api-adv/configs/)
document encoding-endpoint `encodedId` for bodies. They do not prove that Part
metadata `configurationId` or Assembly-part `fullConfiguration` is accepted as
that body's exact value. Carrier-query evidence does not prove translation-body
behavior. Each proposed direct binding needs independent differential evidence.

## Snapshot Barrier And Configuration Gate

Before the first create call, independently resolve the plan's reused root
version with the sole query `parents=false` and no `linkDocumentId`:

```text
GET /api/v16/documents/d/{root.documentId}/versions/{root.versionId}?parents=false
```

Require bounded exact `BTVersionInfo.documentId` and `id` to equal the requested
root document and version. Consume only `microversion`, never the historical
`microversionId` alias. Require its microversion to equal
every consumed planned leaf's `documentMicroversion`, and every leaf document
to equal the root document. A mismatch is unavailable with zero acquisition
calls. Do not discover another version or recapture metadata to repair it.

Wrong or malformed returned document/version identity is an operational
API-contract failure. Only a well-formed independently resolved snapshot that
differs from the planned leaf snapshot/source is classified as unavailable.

Part Studio plans use the selected Part metadata `configurationId` as leaf
`configurationIdentity`. Assembly plans use exact joined Assembly-part
`fullConfiguration`; the Assembly root's encoding request value cannot substitute
for an independently configured leaf. A successful plan alone proves neither
candidate translation-body binding.

The controlled trials below reject direct Part Studio `configurationId` use:
non-default identities returned default geometry. Exact Assembly-leaf
`fullConfiguration` use discriminated the tested configurations. These are
separate conclusions; no equality among configuration spaces is inferred.
The Part Studio alternative uses only the original encoding handoff under the
trusted provenance contract below. Phase 2 remains gated on review and merge of
that contract.

Unsupported or unproven bindings are unavailable before create. If direct use
is rejected or remains unproven, any alternative must separately retain trusted
leaf acquisition-request provenance bound to the exact source snapshot and leaf.
It must preserve the existing plan, leaf, source, and configuration hash scopes.
Do not derive an encoding by parsing a response identity or silently add an
encoding to a plan identity. The original coordinator handoff already has the
bounded differential evidence recorded below. A different unproven request
representation requires evidence before support is enabled; merely selecting
this provenance mechanism requires no repeat characterization.

## Trusted Acquisition Provenance Contract

### Producer And Successful Invocation Boundary

Issue #174 owns a trusted service wrapper around the existing coordinator and
`plan_selection` flow. It obtains the coordinator's validated four-field
handoff, freezes the complete `SelectionRequest` that contains that exact
handoff and caller-ordered selectors, and invokes the unchanged planner with
that immutable request. The wrapper retains the original handoff and its exact
validated encoding context throughout that invocation. Capture occurs before
planning, not from a caller-attached value after the plan returns.

Only a complete successful return from that same invocation can become a
provenance record. The producer retains the complete returned
`ResolvedSelectionPlan`, including its required `planIdentity`, authoring
identity, root, every object, annotation, display name, selector, and placement.
It verifies exact request/root document, version, element, and kind equality;
ordered equality of each request selector's `partId` or `occurrencePath` with
the corresponding full planned authoring selector's same field, plus exact
authoring-selector/root/leaf consistency; and the original handoff/context against the
successful snapshot and existing read-only encoding validation. It does not
resolve the version again, encode, or read new metadata to construct provenance.
Planner snapshot or provenance failure produces no association.

The wrapper's return is atomic: publish a successful trusted plan/provenance
handle only after the entire record has been validated and durably committed.
No caller-accessible constructor, deserialization path, record-insertion API,
or administrative import accepts an independently supplied plan/handoff pair
as proof of invocation. A caller who attaches another independently valid
encoding, even one in the trusted encoding cache, has not supplied the original
handoff consumed by this invocation.

### Closed Record And Canonical Identity

The exact camelCase record contains the following fields and no others:

```json
{
  "provenanceSchemaVersion": 1,
  "requestContract": "onshape-export-configured-leaf-geometry-request-v1",
  "configurationEncoding": "<original four-field EncodingHandoff object>",
  "encodingContext": "<complete closed encoding-context-v1 object>",
  "plan": "<complete four-field ResolvedSelectionPlan object>",
  "bindings": [
    {
      "position": 0,
      "planLocalObjectIdentity": "<exact object identity>",
      "selector": "<complete planned authoring selector object>",
      "configuredLeaf": "<complete closed planned configured-leaf object>",
      "representation": "part_studio_original_encoding_v1",
      "configurationRequestValue": "<exact original encodedId>"
    }
  ],
  "provenanceIdentity": "<lowercase SHA-256>"
}
```

Angle-bracket placeholders denote the referenced objects, not serialized JSON
strings. `configurationEncoding` and `encodingContext` have exactly the closed
schemas and identity rules in [selection plans](onshape-selection-plans.md).
`plan` is the unmodified complete returned plan. Require 1-256 bindings, exactly
one per object in plan order, with contiguous zero-based integer positions and
exact selector, object identity, and configured-leaf equality. There is no
subset, permutation, duplicate position, or extra binding.

For a Part Studio root, every binding has representation
`part_studio_original_encoding_v1`; its request value equals the retained
original handoff's `encodedId`. Every leaf has the root document, microversion,
element, and selected response-derived configuration identity, and its `partId`
equals that object's exact Part selector. The encoding context pins that same
document, caller version, resolved microversion, root element, and kind.

For an Assembly root, every binding instead has representation
`assembly_leaf_full_configuration_v1`; its request value equals that exact
planned leaf's `configurationIdentity`, which the planner obtained from the
joined `fullConfiguration`. Retain the original root handoff/context as
planning provenance only. Its `encodedId` is never an Assembly leaf translation
value. The leaf document/microversion equal the root; leaf element and part
come only from the complete successful plan, never from the root element or
occurrence-path tail. Mixed representations and unknown contract/version values
are rejected. Recording a plan does not itself approve an unproven binding.

`provenanceIdentity` is lowercase SHA-256 over RFC 8785/JCS UTF-8 bytes of
`{"domain":"onshape-export-acquisition-provenance-v1","payload":<payload>}`.
The closed payload contains exactly the first six fields above, excluding
`provenanceIdentity`. Use the planner's normalized matrices unchanged. Recompute
the existing complete-plan and object identities under their existing domains;
never add provenance to those preimages. Recompute the encoding context and
source/configuration bindings through existing read-only evidence validation.

The complete canonical record has a 16,777,216-byte ceiling, independently of
the unchanged 8,388,608-byte plan-hash envelope ceiling. Count while serializing
and reject before retaining byte limit plus one. Persisted/serialized records
require complete UTF-8 JSON, duplicate-member rejection, closed typed schemas,
at most 32 nested containers, 4,096 entries per array, 256 members per object,
and 262,144 total object members. Apply existing string/hash, selector,
annotation, matrix, and plan bounds. Validate already normalized matrices;
do not repair input or normalize it into a different successful record.

This identity detects unequal content; it does not authenticate the producer
or prove that planning consumed an encoding. All records and identities remain
outside existing #173 source, configuration, leaf, object, and plan hash scopes.

### Persistence, Concurrency, And Lifecycle

Phase 2 adds one service-owned table `acquisition_plan_provenance` through the
normal transactional migration runner. Its required columns are
`provenance_schema_version` (integer fixed to 1), `plan_identity` (text),
`provenance_identity` (text), and `record_json` (the complete canonical record).
The primary key is `(provenance_schema_version, plan_identity)`. Require the
row's keys and identity to equal the validated record. No legacy rows are
copied, no existing plan/cache schema is changed, and no provenance is inferred
for a historical plan.

Only the trusted producer writes this table. The database and the service
code/storage administration that can write it are trusted like existing
encoding evidence; a digest is not a defense against a malicious database
writer. No public or serialized-input path may insert or update provenance.
Internal producer/consumer APIs keep insertion authority separate from
read-only lookup, and the trusted handle cannot be constructed from JSON.

After complete planning success, validate the entire candidate before opening
a short recording transaction. Insert the complete record once, never a pending
row or individual per-object records. On a key conflict, reread the committed
winner and apply full record/evidence validation. Exact canonical equality of
the entire candidate and winner is an idempotent success. Unequal content,
even with an equal recomputed plan identity and independently valid encoding,
is an operational invariant conflict. Never overwrite, merge, select a newer
encoding, or repair the winner. A corrupted winner fails rather than becoming
a cache miss. Concurrent identical producers share one association; conflicting
producers cannot both obtain a successful association for that key.

The complete plan and sidecar are stored together in `record_json`; insertion
and commit failure publish no successful handle. Cancellation or process failure
before commit leaves no association. A crash after commit but before returning
may leave one complete valid association: restart/retry uses trusted lookup and
full validation, never reconstruction from a supplied pair. Failed or partial
planning attempts create no association; an earlier committed success is not
deleted by a later failed attempt. An outer retry is a separate execution under
the unchanged coordinator/planner policies, not an internal acquisition retry.

Retain successful associations with their exact encoding/typed-value evidence
for as long as they may be consumed or audited. There is no automatic expiry,
overwrite, upgrade, or provenance backfill. If evidence is missing or malformed
after restart, fail operationally without encoding or provenance repair. Future
schema changes, migration/retention policy, or external authenticated import
need separate review; version 1 provides no such import. Treat source values,
configuration encodings, plans, records, and storage handles as private service
data under existing access and sanitized-diagnostic policy.

### Consumer Validation And Zero-Create Ordering

The acquisition entry point accepts a trusted successful-plan handle. A
serialized lookup request is a closed camelCase object with required
`provenanceSchemaVersion: 1`, `planIdentity`, and `provenanceIdentity`, and only
optional `plan` and `record` fields. Both identities are lowercase SHA-256
strings. Decode under the record's complete JSON/byte bounds with duplicate and
unknown-field rejection; if present, `plan` and `record` must be complete closed
objects equal to the retrieved record's plan and entire record respectively.
Null, omitted required fields, wrong types, and unsupported schema versions
are malformed input. No serialized handoff pair or insertion operation exists.
Deserialization, matching a digest, or finding a valid encoding row cannot
create a trusted handle. Missing associations, unequal lookup identities, and
caller-forged records fail without fallback.

Before any geometry create, complete these barriers for the entire plan:

1. Validate input shape and bounds, then read the one committed association.
   Strictly validate its entire closed record, canonical identity, keys,
   complete-plan/object identities, root/source/selector consistency, and every
   ordered binding. Trusted internal values still require invariant validation;
   serialization never establishes invocation provenance.
2. From the retained planned snapshot and static trusted origin, validate the
   original handoff, context, and all existing encoding/typed-value evidence
   through the read-only validator. No encoding call, fresh metadata, alternate
   context, record creation, or provenance repair is permitted. A different
   independently valid encoding cannot replace the recorded original.
3. Check every leaf's representation against the separately reviewed
   demonstrated request contract and supported source profile. Explicit direct
   Part metadata `configurationId` acquisition remains rejected/unavailable;
   missing provenance for the original-handoff contract is operational, not an
   unsupported-binding fallback. Assembly root encoding is never substituted.
4. Start the 900-second plan-acquisition deadline immediately before the sole
   independent root-version barrier specified above. Validate returned exact
   document/version identity and well-formed microversion before comparing the
   resolved snapshot with the root and every consumed leaf. Do not use that
   fresh result to reinterpret or repair the recorded snapshot.
5. Only after every barrier succeeds, acquire distinct exact leaves sequentially
   in first-occurrence order under the pinned create/poll/download contract.
   Validate and retain the complete payload/evidence for each before reuse;
   publish the complete ordered occurrence handoff only after all succeed.

Malformed serialized input is an invalid acquisition handoff using the existing
`InvalidSelection` caller-input classification, with zero creates.
Missing, forged, corrupted, unequal, or contradictory provenance, storage
failure, and trusted-input invariant failure are
`OperationalApiContractFailure` with zero creates. Wrong/malformed independently
returned version identity is also operational. Reserve unavailable for a
well-formed independently resolved snapshot/source mismatch and rejected or
unproven source bindings. Authentication, transport, HTTP, and timeout failures
retain their existing typed classifications; free text never changes them.

### Complete Ordered Occurrence-To-Payload Handoff

A successful handoff contains exactly `plan`, `provenanceIdentity`, and
`bindings`. `plan` is the complete unchanged successful plan. Each binding
contains exactly `position`, `planLocalObjectIdentity`, `configuredLeaf`,
`retainedPayload`, and `acquisitionEvidence`. Positions and object/leaf values
must match the provenance record and plan one-for-one in caller order. The full
plan preserves each selector, display name, annotation, and placement; no
metadata is recaptured or moved into leaf identity.

`retainedPayload` contains exactly `storageReference`, `byteLength`, and
`sha256`. The reference names service-owned immutable opaque bytes, never an
upstream URL or a protocol path. Its internal storage locator is private and has
no logical identity role. Length is the actual positive integer byte count
within the download ceiling; hash is the lowercase service-computed SHA-256.

`acquisitionEvidence` contains exactly `requestContract`, `representation`,
`configurationRequestValue`, `documentId`, `versionId`, `configuredLeaf`,
`createRequestPath`, `createRequestQuery`, `createRequestBody`,
`createTranslationId`, `terminalTranslationId`, `externalDataId`,
`downloadDocumentId`, `transportMedia`, `byteLength`, and `sha256`.
The path, empty query object, and closed body are the exact pinned create
request below; store its decoded JSON object without interpreting configuration
strings. The evidence's representation/value and selectors equal the validated
binding/root. Create and terminal IDs are equal; the result and download
document follow the causal chain below. Length/hash equal the retained bytes.
`transportMedia` retains the actual accepted declaration. Request provenance
and transport media remain distinct from plan/source/configuration identities
and protocol-facing media. Poll/download paths and query sets follow uniquely
from these exact IDs under the pinned contract; no response URL is followed.

One distinct binding remains for every logical occurrence, including shared
leaves, equal bytes, different placements, and different annotations. Within
one invocation reuse only a fully successful acquisition for exactly equal
closed leaf keys under the same exact request contract, representation, request
value, and reused document/version. Content hashes do not authorize reuse.
Cross-invocation acquisition caching is not enabled by this contract: retained
evidence permits audit, not an implicit cache policy or broader support claim.

The handoff is all-or-nothing. If any acquisition, retention, or final binding
validation fails, return only the typed failure, release uncommitted payload
references, and publish no successful or partially reusable acquisition result.
The previously committed successful planning association may remain valid; it
is not evidence of a successful acquisition. #175 alone allocates deterministic
protocol retained paths and builds manifests/settings from this complete
handoff. #168 stages every occurrence at its declared unique path, even when
bindings share bytes. Acquisition filenames never determine either path.

## Causal Translation And Download Chain

Create and polling return `BTTranslationRequestInfo`; the OpenAPI schema marks
no fields required. Acquisition imposes a causal minimum: one nonempty
visible-ASCII `id` of at most 4,096 bytes and exact `requestState` in `ACTIVE`,
`DONE`, or `FAILED`. Poll only the
create response's ID at:

```text
GET /api/v16/translations/{translationId}
```

The query set is empty. Every poll must expose the same exact ID. Never follow
`href` or `viewRef`, accept an alias field, or substitute another translation.
Immediate `DONE` is terminal success; `FAILED` is a typed translation failure,
not absent geometry. Unknown or malformed states are operational contract
failures.

When exposed, nonempty `documentId`, `requestElementId`, and `versionId` must
equal the requested document, leaf element, and reused version respectively.
Nonempty `resultDocumentId` must equal that same document. Missing optional
echoes do not violate the baseline. A present null, wrong-type, or empty echo
is an operational contract failure; the observed successful responses exposed
all four as nonempty exact matches.
They never authorize a different source or download document. No response field
claims a mandatory configuration echo; request provenance and differential
evidence remain necessary.

At `DONE`, require `resultExternalDataIds` to be an array containing exactly one
nonempty visible-ASCII string of at most 4,096 bytes. Reject zero, multiple,
duplicate, empty, null, or non-string results without filtering or deduplicating
them into success. Require no result elements. Successful controlled responses
represented no result elements as JSON null. Therefore `resultElementIds`
alone accepts missing, null, or an empty array as no result elements; every other
present value fails. This exception does not apply to terminal
`resultExternalDataIds`. Do not zip result arrays or infer object mapping from
result cardinality.

Download only that exact external-data ID in the requested root document:

```text
GET /api/v16/documents/d/{root.documentId}/externaldata/{externalDataId}
```

The query set is empty; omit `If-None-Match`. Require one complete, nonempty,
bounded response. Record exact request provenance, create ID, terminal ID,
external-data ID, download document, actual byte length, service-computed
SHA-256, and transport media. Attempt IDs and content hashes do not become leaf
or occurrence identities. No partial acquisition output is reusable as success.

## Transport, Polling, And Opaque Media

Keep the selection contract's trusted origin, native TLS verification, zero
redirects, disabled decompression, and `Accept-Encoding: identity`. Send
`Content-Type: application/json` on create and `Accept: application/json` on
create/poll. Download requests send `Accept: application/octet-stream`.

Each HTTP operation has one attempt, a 10-second connect timeout, a 10-second
read-idle timeout, and a 60-second total deadline. Do not retry create after an
ambiguous response, retry failed polls/downloads internally, refresh credentials,
or switch endpoints. Outer job retries are separate executions.

Poll after intervals of 2, 4, 8, 15, and then 30 seconds, bounded by 32 poll
calls and a 600-second translation deadline starting immediately before create.
Completion and download must also fit a 660-second leaf-acquisition deadline
starting at that same instant. The whole plan has a 900-second acquisition
deadline starting immediately before the independent version barrier. Clip every
call's total allowance and every sleep to the remaining applicable deadlines;
starting before expiry does not permit completion after expiry. Never start a
call or sleep without positive remaining time. A sequence of successful
`ACTIVE` responses is polling, not an HTTP retry. Deadline or poll exhaustion is
an operational timeout and returns no payload binding.

Version and create/poll bodies have 65,536-byte ceilings. Raw downloads have a
134,217,728-byte ceiling. Apply the selection contract's strict declared/actual
length checks, independent streaming byte counts, UTF-8/duplicate-member JSON
validation, and structural limits to JSON responses. The production transport
policy accepts HTTP 200 for version/poll/download and HTTP 200 or 201 for create;
all other statuses fail. The MCP interface exposed HTTP 200 for downloads but
did not expose successful JSON-operation statuses; the create status allowlist
is a conservative implementation policy, not an observed upstream status claim.
Retain at most 65,536 diagnostic bytes and
never let diagnostic defects overwrite HTTP-derived authentication failures.

The controlled downloads exposed `application/octet-stream;charset=utf-8`;
the historical selected-object trials exposed `application/octet-stream`.
Accept that base media type with no parameters or with the sole `charset=utf-8`
parameter, using case-insensitive MIME token matching and optional whitespace.
Retain the actual declaration separately from requested format and
protocol-facing raw-input media. Reject other parameters, missing, malformed,
duplicate, or unsupported media declarations and non-identity content encoding
as operational contract failures. The pinned JSON-labeled binary response is
not accepted. A charset parameter on this observed binary response does not
authorize text decoding. Do not parse ZIP members, OPC parts, model XML, units,
meshes, build items, or internal object grouping in production acquisition.

The trusted generator owns raw Geometry 3MF interpretation and internal
validation. Acquisition establishes an opaque causal byte payload, not a
validated 3MF or fully placed object. Temporary source-neutral geometry
inspection is permitted only to establish bounded characterization results.

## Typed Failures And Synthetic Verification

| Condition | Outcome |
| --- | --- |
| Invalid plan/handoff shape or internal provenance contradiction | Invalid acquisition input or operational invariant failure; zero create calls |
| Proven snapshot mismatch or unsupported/unproven source binding | Unavailable acquisition; zero create calls |
| HTTP 401/403, including malformed diagnostic bodies | Authentication failure; no availability conclusion |
| DNS, TLS, connection, framing, premature EOF, or HTTP timeout | Transport failure |
| Other non-success HTTP status, including 429/5xx | Operational HTTP failure |
| Translation `FAILED` | Operational translation failure; do not infer unsupported source from free text |
| Poll/translation/acquisition deadline exhaustion | Operational timeout |
| Invalid JSON, identity contradiction, malformed/cardinality result, empty payload, media or size mismatch | Operational API-contract failure |

Every failure returns no successful ordered occurrence-to-payload handoff.
Diagnostics follow the selection contract's bounded, sanitized rules and may
retain operation and selector position, never source values or upstream text.

Issue #174 synthetic tests must capture the exact version/create/poll/download call
order, paths, query sets, JSON string serialization, options, and omissions.
Cover wrong, missing, or malformed barrier document/version/microversion fields
with zero creates; the rejected Part Studio direct binding and demonstrated
bounded Assembly binding;
immediate `DONE`, `ACTIVE` sequences, `FAILED`, unknown states, changed IDs,
optional echoes, contradictory sources, every malformed result array, declared
and actual size boundaries, empty downloads, media/encoding defects,
authentication precedence, transport interruption, HTTP failures, and every
deadline boundary. Require atomic failure, ordered distinct occurrence bindings,
reuse only for exact equal leaves, and no content-based occurrence collapse.

### Phase 2 Provenance Verification Matrix

Use only source-neutral synthetic requests, encoding-cache evidence, plans,
storage, and fake transport. The phase 1 PR specifies these obligations; phase 2
must implement them before production support. Existing controlled evidence is
sufficient for the original Part Studio handoff mechanism; no new fixture or
upstream slicer source is needed for this verification.

| Synthetic case | Required outcome |
| --- | --- |
| Successful original Part Studio invocation | Exact original handoff/context and complete returned plan recorded; one exact binding per object |
| Successful ordinary flat Assembly invocation | Root handoff retained only for planning provenance; each leaf request uses its exact joined identity |
| Another valid cached encoding attached after planning, with all hashes recomputed | No producer/insert authority; operational rejection and zero creates |
| Missing association or referenced encoding/typed-value evidence | Operational failure; zero creates, zero encode calls, no repair |
| Forged record with a correct self-computed provenance digest | Trusted lookup/equality required; operational rejection and zero creates |
| Different document/version/microversion/element/kind/origin/API/context/config/encoded ID | Full binding validation fails operationally before creates |
| Alter display, annotation, authoring identity, placement, selector, object, leaf, or order | Complete-plan/record equality or identity fails; zero creates |
| Omit, duplicate, reorder, append, or change any binding/position | Complete ordered binding validation fails; zero creates |
| Unknown field/version/contract/representation, duplicate JSON member, malformed scalar, or exceeded bound | Invalid serialized input or operational stored-record failure; zero creates |
| Non-normalized matrix supplied with a recomputed hash | Reject; do not repair it into a valid record |
| Hash preimage includes provenance in an existing identity | Existing #173 golden identity fails; all original preimages stay unchanged |
| Identical successful invocations racing to insert | One complete row; reread/full validation gives the same association |
| Same plan identity with different independently valid original handoffs | Unequal association conflict; no overwrite or second successful association |
| Corrupted concurrent winner or row/key/identity mismatch | Operational failure; not a miss, no replacement |
| Planner fails, is cancelled, or produces only partial objects | No new association or successful handle |
| Validation/insert/commit fails, or crash occurs before commit | No published successful handle; no partial committed association |
| Crash after commit before return, then restart lookup | One complete row reusable only after full read-only validation |
| Failed later attempt with an earlier successful association | Earlier row preserved; failed invocation publishes no success |
| Explicit rejected Part Studio direct binding or unproven representation | Unavailable before creates; original encoding is not an implicit fallback |
| Malformed independent version document/version/microversion | Operational failure, zero creates |
| Well-formed independent snapshot differs from any planned root/leaf | Unavailable, zero creates |
| Every provenance/binding validated before barrier and any create | Fake transport records exact order; no metadata or encoding call |
| Repeated equal leaves at distinct occurrences | One acquisition per exact eligible leaf; separate bindings, matrices, annotations, and order |
| Different leaf identities yielding equal bytes/SHA-256 | Separate causal acquisitions and occurrence bindings; no content collapse |
| Failure on a later acquisition, retention, or final handoff check | No complete or partial reusable handoff; uncommitted payload references released |
| Successful handoff consumed by #175/#168 | No acquisition-allocated protocol path; every declared occurrence path staged separately |

Fault injection must include transaction failure, cancellation around commit,
reopened persistent storage, concurrent identical and conflicting producers,
and a conflicting winner that has independently valid encoding evidence. Count
all calls: every provenance rejection makes zero creates and zero encoding or
metadata calls. Run unchanged #173 identity goldens and planner tests alongside
the full transport/translation matrix above. Acquisition failure may follow
earlier creates, but never returns their partial payloads as reusable success.

## Controlled Evidence And Binding Conclusions

### Fixture, Plans, And Observation Boundary

On 2026-10-06, authenticated trials used the published npm `onshape-mcp` 0.5.2
server through its normal OAuth flow. Credentials and refresh remained owned
by the server. One controlled synthetic document was created directly in the
verified `onshape-export/Agent Sandbox` folder. Post-creation checks confirmed
the parent, authenticated-user ownership, Free tier, and required public access.
The fixture and its historical versions remain persistent evidence.

The Part Studio contains two distinguishable strict solids: one changes under
a single quantity input and the other remains fixed with different bounds.
Aliases `D`, `A`, and `B` denote default and two geometrically distinguishable
non-default configurations, without publishing their values. Five ordinary
flat Assembly occurrences cover `D, D, A, B, A`. The extra default repeat came
from a setup retry after an empty successful insertion response; it was read
back and preserved. It does not change the bounded profile.

The unchanged configuration coordinator and #173 planner produced three
two-object Part Studio plans and one five-object Assembly plan. A temporary
fixture-restricted numeric-loopback adapter forwarded planner calls to the MCP
server, using the existing test transport hook and synthetic local signing
credentials. The Assembly plan had three distinct exact leaves, five distinct
ordered occurrence bindings, and three carrier reads. Every leaf matched the
independently resolved reused version microversion.

Explicit fixture-authored canonical typed intent was supplied to the normal
coordinator. The current metadata normalizer misclassifies this modern quantity
response as Boolean and misses its direct range units/default fields; fixing
that separate compatibility issue is outside this characterization. Synthetic
descriptions were set explicitly because the strict planner rejects null
descriptions. Neither raw API responses nor planner algorithms were altered.
This proves successful planner behavior after a trusted typed-intent handoff,
not the complete authoring-metadata normalization flow.

MCP reformats parsed JSON and hides successful JSON HTTP statuses and most
response headers. It exposes binary status, content type, byte length, and
base64 bytes. The experiment therefore does not establish upstream duplicate
JSON-member handling, raw HTTP framing, response encoding headers, redirect
behavior, streaming limits, or deadline boundaries. Those remain explicit
production transport policies requiring synthetic verification in #174.

### Differential Binding Matrix

Every positive/control trial used the exact frozen version-addressed request
above and one exact official planned part ID. Each encoding control was obtained
independently from the endpoint using retained fixture typed intent. Assembly
leaf query bounds independently distinguished the corresponding expected
control geometry, without parsing a configuration string. This comparison does
not equate configurations or replace the planner's exact source joins.

| Planned representation used unchanged in the JSON body | Default | Non-default A | Non-default B | A repeated after B | Conclusion |
| --- | --- | --- | --- | --- | --- |
| Part metadata `configurationId` | Matches default control | Exports default geometry, differs from A control | Exports default geometry, differs from B control | Again exports default geometry | **Rejected** for direct configured Part Studio acquisition; default coincidence is not proof |
| Joined Assembly-part `fullConfiguration` | Both repeated default leaves match controls | Matches A control | Matches B control | Matches A control again | **Demonstrated for the tested single-quantity ordinary flat leaves** |
| Independent encoding-endpoint controls | Default geometry | Distinct A geometry | Distinct B geometry | A geometry restored | Positive differential controls |

Temporary source-neutral inspection compared complete build geometry bounds,
including container units and build/component transforms. The independently
queried source bounds and exports distinguished all three configurations and
the other solid. No names, response positions, payload hashes, member names,
or object counts established part/configuration identity. Part IDs remained
equal across the three tested configurations, so the observed differences
cannot be attributed to changing selectors. All Assembly root configuration
identities differed from their leaf identities.

For each successful payload, the observed create state was `ACTIVE`, polling
reached `DONE` with the same exact translation ID, all four source/result
echoes matched, terminal external-data cardinality was one, and result elements
were null. The exact download document and external-data ID were retained
privately. Each download exposed HTTP 200, nonempty bounded bytes, matching
envelope byte length, and the media declaration above. Actual byte lengths and
SHA-256 values were measured privately; repeated requests are independent
attempts and content hashes confer no logical identity.

There were 18 completed direct/control payload trials. Each inspected payload
had one mesh, one object, and one build item; those counts are observations,
not service-side acceptance conditions. The repeated A control kept the same
geometry while its payload hash changed. Default direct/control geometry also
matched despite different byte hashes.

### Wrong And Stale Controls

| Trial against the intended A leaf | Observed result | Bounded conclusion |
| --- | --- | --- |
| Valid B encoding substituted | Complete B geometry payload | Successful lifecycle does not prove the intended configuration |
| Earlier default encoding substituted | Complete default geometry payload | Stale request provenance cannot be repaired by a stable part ID |
| Other exact solid's official part ID substituted | That other solid's geometry | Singleton part selection discriminated the two source solids |
| Absent synthetic part selector | HTTP 400; no payload | Operational rejection; upstream text is not an availability classifier |
| Malformed synthetic quantity configuration | Complete default geometry payload | Malformed configuration can silently fall back; success is insufficient |
| Assembly root encoding substituted for independently configured A leaf | Complete default geometry payload | Root encoding is not a leaf request substitute |
| Historical leaf microversion substituted before barrier | Independently resolved version mismatch; zero creates | Stale snapshot is unavailable before acquisition |

The exact encoding handoff returned by the unchanged coordinator for each
successful Part Studio plan was separately tested: default, A, and B all
exported their expected geometry. This supports the provenance contract
above; it does not add that handoff to the plan or implement its
required validation. Distinct occurrence bindings remain mandatory even when
repeated exact leaves reuse one acquisition or produce equal geometry.

### Part Studio Alternative And Delivery Gate

The trusted acquisition-provenance contract above replaces the earlier proposed
alternative with a reviewable recording, persistence, validation, and handoff
boundary owned by #174. Only the original coordinator `encodedId` consumed by
the same successful invocation is eligible for Part Studio acquisition. Direct
metadata `configurationId` remains rejected and unavailable; an arbitrary valid
encoding is not a substitute. Independently configured Assembly leaves retain
their separate demonstrated `fullConfiguration` representation.

Review and merge of #174's focused phase 1 contract PR satisfies the separate
handoff review required by the merged #308 document. Phase 2 then implements
and verifies this contract and the pinned acquisition/transport flow. Closing
issue #308 established bounded evidence; it did not approve provenance recording or
enable production acquisition. The phase 1 PR must not close #174, and #174 is
complete only after both phases are delivered and verified.

Record only sanitized aliases, request shapes, counts, booleans,
classifications, and bounded conclusions. Keep controlled document, version,
microversion, element, part, occurrence, configuration, translation,
external-data, raw-response, payload, payload-hash, credential, and account
identities outside this repository. Preserve controlled fixtures as persistent
reproducibility evidence. No slicer source or target-derived material is used.
