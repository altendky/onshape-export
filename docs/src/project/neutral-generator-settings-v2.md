# Neutral Generator Settings V2

> **Status: Normative settings v2.** This contract extends generator settings
> without changing generator-settings v1 or neutral generator protocol v1.

## Document Contract

The Draft 2020-12 schema is
[`protocol/generator-settings/v2/generator-settings.schema.json`](../../../protocol/generator-settings/v2/generator-settings.schema.json).
Its `$id` is the same repository URL. A closed document contains exactly:

- `schemaVersion: 2`.
- The unchanged settings-v1 ordered `blockers` contract.
- An ordered `placements` array with zero to 256 entries.

Each placement contains exactly `objectIdentity` and `matrix`. Object identities
are unique within `placements`, and placement order is protocol-v1 manifest
object order. Standalone validation can preserve and check uniqueness of that
order but cannot infer manifest correspondence.

Each matrix is exactly 16 finite binary64 JSON numbers in row-major order:

```text
[m00, m01, m02, tx,
 m10, m11, m12, ty,
 m20, m21, m22, tz,
 0,   0,   0,   1]
```

Matrices use column-vector multiplication and meters. They map the fully
realized local input frame to the shared project frame. The settings contract
does not derive or prove a source transform.

## Normalization And Validation

Before standalone or contextual validation, canonicalization, or identity
calculation, every scalar numerically equal to zero is normalized to `+0.0`.
This includes `-0.0` in linear, translation, and final-row positions. Signed
zero has no semantic distinction. NaN and infinity are invalid, and the final
row after normalization must equal `[0,0,0,1]` exactly.

Standalone validation owns only:

- The closed schema, document and collection bounds, matrix length, finite
  scalars, and normalized affine final row.
- Unique valid placement object identities and the order represented by the
  document.
- The unchanged settings-v1 blocker syntax, bounds, uniqueness, and failure
  semantics.

The pure contextual validator receives a complete ordered expected-placement
summary. Every entry contains `objectIdentity`, `transportRole`, and
`expectedNeutralPlacementMatrix`. Validation requires exact identity, order,
and cardinality correspondence. The unchanged v1 blocker membership rules must
match transport roles, and each matrix scalar must equal the corresponding
expected scalar after both matrices are normalized.

Missing, extra, duplicate, reordered, wrong-identity, wrong-role, malformed, or
scalar-mismatched entries fail closed.

## Canonical Identities

Identities are lowercase SHA-256 hex over RFC 8785/JCS UTF-8 bytes of
`{"domain":"domain-name","payload":{}}`.

| Identity | Domain | Payload |
| --- | --- | --- |
| Settings-v2 schema | `onshape-export-generator-settings-schema-v2` | Complete parsed v2 schema, including `$schema` and `$id` |
| Settings-v2 document | `onshape-export-generator-settings-v2` | Complete validated settings document after signed-zero normalization |

The normalized document's JCS bytes preserve blocker, target, and placement
array order. A nonzero scalar sign remains identity-significant.

## Context Ownership

Issue [#173](https://github.com/altendky/onshape-export/issues/173) derives and
proves each expected matrix from resolved source evidence. That work owns exact
source and occurrence-path matching and proof that no ancestor composition
occurred. Settings v2 does not repeat those proofs.

Issue [#175](https://github.com/altendky/onshape-export/issues/175) owns manifest
order and transport roles, deterministic logical retained-path allocation,
settings construction, orchestration, and invocation of contextual validation.
Equal bytes, content identity, hash, and length may occur at distinct retained
paths as distinct logical occurrences. Protocol v1 rejects sharing one retained
path; settings v2 defines no path allocator or naming scheme.

Generator raw-input bounds and final target-aware self-validation are owned by
[`slicer-project-generators#8`](https://github.com/altendky/slicer-project-generators/issues/8)
and
[`slicer-project-generators#9`](https://github.com/altendky/slicer-project-generators/issues/9).
Protocol request/output bounds, retained-file verification, runtime invocation,
cache, publication, and revocation remain outside this settings contract.

The bounded evidence and rejected direct-selector conclusion remain documented
in the source-neutral
[Onshape Geometry Input Characterization](onshape-geometry-input-characterization.md).
This contract adds no target schema, fixture, implementation fact, or
compatibility claim.
