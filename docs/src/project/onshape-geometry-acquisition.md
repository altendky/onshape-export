# Configured-Leaf Geometry 3MF Acquisition

> **Status: bounded characterization completed; Part Studio direct binding
> rejected, Assembly-leaf direct binding demonstrated.**
> This source-neutral contract belongs to
> [#308](https://github.com/altendky/onshape-export/issues/308). It specifies the
> evidence required before
> [#174](https://github.com/altendky/onshape-export/issues/174) can acquire
> configured leaves. It does not implement production acquisition.

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
The complete #174 implementation remains blocked on a reviewed Part Studio
request-provenance handoff.

Unsupported or unproven bindings are unavailable before create. If direct use
is rejected or remains unproven, any alternative must separately retain trusted
leaf acquisition-request provenance bound to the exact source snapshot and leaf.
It must preserve the existing plan, leaf, source, and configuration hash scopes.
Do not derive an encoding by parsing a response identity or silently add an
encoding to a plan identity. An alternative requires its own reviewed handoff and
evidence; this characterization does not authorize it.

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
exported their expected geometry. This supports the proposed provenance
alternative below; it does not add that handoff to the plan or implement its
required validation. Distinct occurrence bindings remain mandatory even when
repeated exact leaves reuse one acquisition or produce equal geometry.

### Required Part Studio Alternative

Issue #174 remains blocked for the complete source profile. Direct Part Studio
metadata identities must return unavailable before geometry calls. The
demonstrated Assembly binding does not authorize a fallback for Part Studios.

The proposed alternative is a separately validated acquisition-provenance
sidecar retaining the exact trusted `encodedId` used to obtain the successful
Part Studio plan. Bind it to the originating encoding context, exact source
document/version/microversion/element, complete plan identity, and each exact
leaf/part. Validate that relationship atomically before acquisition; a sidecar
for another plan or snapshot is an operational provenance/invariant failure.
A well-formed independently resolved source snapshot mismatch is unavailable.
Store request provenance outside
the existing plan, leaf, source, and configuration hash scopes. Do not parse
`configurationId`, re-encode an identity, rediscover metadata, or change #173
identities. This is a proposed reviewed handoff, not implicit permission to use
an arbitrary root encoding. Independently configured Assembly leaves continue
to require their own demonstrated leaf representation.

After a separate sidecar contract is reviewed, synthetic verification must
cover its complete binding, forged/missing/mismatched provenance, zero creates
on rejection, atomic failures, and unchanged plan/hash identities.

Closing #308 records the rejected and demonstrated bindings; it does not clear
the remaining #174 handoff blocker or authorize production acquisition.

Record only sanitized aliases, request shapes, counts, booleans,
classifications, and bounded conclusions. Keep controlled document, version,
microversion, element, part, occurrence, configuration, translation,
external-data, raw-response, payload, payload-hash, credential, and account
identities outside this repository. Preserve controlled fixtures as persistent
reproducibility evidence. No slicer source or target-derived material is used.
