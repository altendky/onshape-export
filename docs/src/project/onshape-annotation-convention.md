# Onshape Annotation And Generator Settings Convention

> **Status: Normative v1 contract.** This page, the checked-in schemas, and the
> Rust validators define the source-neutral Onshape authoring annotation and
> generator-settings exchange. The earlier
> [carrier characterization](onshape-annotation-carrier-characterization.md)
> is evidence, not this convention.

## Scope And Boundaries

V1 classifies already resolved Onshape source objects. It does not discover
objects, choose their order, establish source identity, bind exported geometry,
assign manifest identities, or interpret target-specific behavior. Names,
metadata, authoring keys, response order, and content equality never establish
identity, ordering, deduplication, causal mapping, or geometry mapping.

The sole author-authored sources are the API Part Description and API Part Name
values. There is no sidecar or merge source. V1 does not use exported-file
metadata and does not repurpose part number, material, appearance, Exclude from
all BOMs, Unit of measure, or another native CAD semantic. It requires no custom
property definition.

The API integration owns HTTP and outer-JSON decoding. It must reject invalid
HTTP/JSON UTF-8 and malformed outer Onshape JSON before passing decoded carrier
strings to this parser. This parser strictly decodes its own embedded JSON and
authoring/settings document bytes, including duplicate-member rejection.

No target implementation source, target-derived schema, fixture, constant, or
compatibility claim is part of this contract. The repository ingress boundary
in the [generator integration policy](slicer-project-generator-integration.md#repository-ingress-boundary)
continues to apply.

## Carrier Syntax

V1 reserves exactly one marker, `onshape-export:v1`.

### Description

Description is primary. Split the decoded API string on LF (`U+000A`). A marker
is recognized only at the beginning of the string or immediately after LF when
the line begins `onshape-export:`. The one valid line has this exact shape:

```text
onshape-export:v1 {"role":"supportBlocker","key":"block","targets":["part-a"]}
```

Exactly one ASCII space separates the marker from a strict JSON object. The
object starts immediately after that space and ends at the line end. Other
Description lines remain ordinary text. A second marker line, unsupported or
incomplete version, duplicate or unknown JSON member, malformed JSON, trailing
content or whitespace, invalid role/key/targets combination, or exceeded bound
fails closed.

The complete Description is at most 65,536 UTF-8 bytes. The JSON substring is
at most 4,096 UTF-8 bytes. Its closed object contains exactly:

- Required `role`: `printable` or `supportBlocker`.
- Optional `key`: ASCII `[A-Za-z0-9][A-Za-z0-9._-]{0,31}`.
- Required `targets`: an ordered unique key list with at most 64 entries.

A printable has no targets. A support blocker has one or more targets.

### Name Fallback

Part Name supplies `displayName` and may carry fallback markup only when the
Description has no recognized annotation. The exact terminal suffix is:

```text
 [onshape-export:v1;role=<role>[;key=<key>];targets=<target-list>]
```

Directives occur once in that order. The role and key grammars match
Description. The target list is empty or an ordered comma-separated list of at
most four keys. The suffix is ASCII, literal `%` is forbidden, and its complete
leading-space-through-closing-bracket length is at most 192 bytes. Unknown,
duplicate, reordered, extra, malformed, or unsupported directives fail closed.

A terminal suffix beginning with an ASCII space followed by
`[onshape-export:` is recognized even when its closing delimiter is missing,
and therefore fails rather than becoming display text. Marker-like text followed
by ordinary text is nonterminal and remains literal.

The complete API Part Name before suffix removal and the resulting
`displayName` after removal must each contain 1-256 Unicode scalar values.
Unicode General Category `Cc` is forbidden. There is no separate 256-byte
limit. Every remaining scalar is preserved exactly; implementations neither
normalize Unicode nor infer equivalence. Unicode is forbidden inside the ASCII
suffix.

### Precedence

Every recognized carrier is parsed independently. If Description has no marker,
one valid Name fallback may supply the annotation. If both carriers have valid
annotations, their normalized annotations must be semantically equal. A
conflict or malformed recognized carrier fails even when the other carrier is
valid. Description is preferred but never silently overrides a conflict.

An unannotated selected object normalizes to `printable`, no key, and an empty
target list. Its exact API Part Name becomes `displayName` under the same scalar
and `Cc` rules. It cannot be targeted until it has a unique key.

## Authoring Document

The Draft 2020-12 schema is
[`protocol/authoring/v1/onshape-authoring.schema.json`](../../../protocol/authoring/v1/onshape-authoring.schema.json).
Its `$id` is the same repository URL. The document contains exactly
`schemaVersion: 1` and the explicit ordered `objects` array. Each object contains
exactly `selector`, `displayName`, and `annotation`; `displayName` never appears
inside `annotation`.

A Part Studio selector contains exactly:

- `kind: "partStudioPart"`
- `documentId`
- `documentMicroversion`
- `elementId`
- `configurationIdentity`
- `partId`

An Assembly selector replaces `partId` with an ordered `occurrencePath` and has
`kind: "assemblyOccurrence"`. A path has 1-64 segments. Every selector string is
nonempty visible ASCII and at most 4,096 bytes. Complete selectors are unique.
The array order is supplied by the resolved selection plan and is significant.

The standalone validator enforces schema shape and all context-free semantics.
Target keys must resolve to exactly one printable document object. Missing,
ambiguous, duplicate, self, or blocker targets fail. It never chooses a target
by name, response order, generated suffix, or tail instance ID.

## Repeated Occurrences

Part Description and Part Name are part-scoped and fan out identically to each
selected occurrence of one configured part. Repeated occurrences remain
distinct through their complete source selectors.

The pure contextual validator receives an entry-aligned summary containing each
selector, its plan position, and one opaque configured-part identity. It uses
that identity only for these checks:

- Every occurrence of one configured part has byte-identical RFC 8785/JCS
  canonical normalized annotation bytes and an exactly equal `displayName`
  scalar sequence.
- A duplicate nonempty key is permitted only for occurrences of that same
  configured part with byte-identical canonical normalized annotations.
- Every duplicated key is forbidden as a target, even when its duplicate fan-out
  is otherwise valid.

Permitted unreferenced duplicates do not identify, order, or deduplicate
anything. V1 has no occurrence-specific override. A future occurrence carrier
requires a separately reviewed schema revision.

## Generator Settings

The Draft 2020-12 schema is
[`protocol/generator-settings/v1/generator-settings.schema.json`](../../../protocol/generator-settings/v1/generator-settings.schema.json).
Its `$id` is the same repository URL. The document contains exactly
`schemaVersion: 1` and ordered `blockers`. Each blocker has exactly one
manifest-local `objectIdentity` and an ordered unique list of 1-64 manifest-local
target identities.

Settings contain no Onshape IDs, selectors, occurrence paths, plan-local IDs,
authoring keys, configured-part identities, filenames, or display-name fields
or references. Opaque identity text is never heuristically classified.

Semantic roles map exactly as follows:

| Authoring role | Protocol v1 transport role |
| --- | --- |
| `printable` | `rawGeometry` |
| `supportBlocker` | `auxiliaryGeometry` |

Blocker edges remain settings edges. They never use protocol
`parentObjectIdentity`. This convention gives that protocol field no hierarchy
semantics; protocol v1 continues to validate only existing, acyclic parent
references.

The pure settings contextual validator receives only synthetic or resolved
`(objectIdentity, transportRole)` manifest entries. The settings blocker set
must equal the manifest `auxiliaryGeometry` set exactly. Every blocker occurs
once and references an existing auxiliary object; every target references an
existing `rawGeometry` object. Raw objects may be untargeted. Missing, extra,
duplicate, wrong-role, or unresolved references fail closed.

## Limits And Validation

Authoring and settings documents are each at most 1 MiB of UTF-8 JSON and at
most 256 objects or blockers. Settings contain at most 16,384 total target
edges. Manifest-local identities are nonempty visible ASCII with at most 256
characters. Fixed closed shapes prohibit recursive values.

Schema validation is necessary but not sufficient. Standard JSON Schema cannot
express selector-property uniqueness, key resolution, configured-part fan-out,
plan-position correspondence, or equality with a manifest role set. The Rust
standalone and contextual validators are therefore normative and required.
Unknown fields, versions, enum values, duplicate JSON members, unsupported
combinations, and all exceeded limits fail closed before generator dispatch.

## Canonical Identities

Identities are lowercase SHA-256 hex over RFC 8785/JCS UTF-8 bytes of:

```json
{"domain":"domain-name","payload":{}}
```

| Identity | Domain | Payload |
| --- | --- | --- |
| Authoring schema | `onshape-export-authoring-schema-v1` | Complete parsed authoring schema, including `$schema` and `$id` |
| Authoring document | `onshape-export-authoring-document-v1` | Complete validated authoring document |
| Settings schema | `onshape-export-generator-settings-schema-v1` | Complete parsed settings schema, including `$schema` and `$id` |
| Settings document | `onshape-export-generator-settings-v1` | Complete validated settings document |

Identity fields are not embedded in these payloads. JCS preserves array order
and exact accepted strings and does not normalize Unicode. Consequently every
schema-significant scalar, object order, target order, schema rule, bound,
`$schema`, and `$id` participates in the applicable identity.

## Lifecycle And Integration

Authoring keys are document-local references, never immutable identities. After
immutable source resolution, the selection-plan owner constructs this document
in explicit plan order from exact captured carriers, constructs the aligned
configured-part context, and invokes contextual validation before assigning
plan-local IDs. This contract does not discover or reorder those objects.

The manifest owner later maps plan-local objects to manifest-local identities,
copies exact validated display names into protocol `InputObject.displayName`,
constructs settings in blocker order and declared target order, and invokes the
settings contextual validator before generator input becomes available.

A rename does not change source identity, configured-part identity, plan-local
or manifest-local object identity, retained-content identity, causal mapping,
settings, or settings identity. It does change exact protocol display metadata,
authoring-document identity, plan identity, protocol `inputSetIdentity`,
`manifestIdentity`, and downstream invocation/cache identity. Actual source
resolution, export binding, manifest construction, and runtime invocation remain
owned by their downstream integrations.
