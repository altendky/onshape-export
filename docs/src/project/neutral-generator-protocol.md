# Neutral Generator Protocol

> **Status: Normative protocol v1.** This page and the checked-in schema define
> the source-neutral, file-backed exchange. Generator execution, approval,
> validation, cache, publication, and revocation remain outside this protocol.

## Scope And Artifacts

Protocol v1 consists of three JSON document types:

- `inputManifest` declares an ordered set of retained input objects and whether
  their required mappings are available.
- `generatorRequest` binds one available manifest, one settings file, opaque
  generator and policy identities, and exactly one generated-project output.
- `generatorResult` reports either one candidate output or structured failure.

The Draft 2020-12 schema is
[`protocol/generator/v1/generator-protocol.schema.json`](../../../protocol/generator/v1/generator-protocol.schema.json).
Synthetic examples are stored beside it. The Rust types and semantic validator
are in `src/generator_protocol.rs`.

All documents use `protocolVersion: 1`; manifests also use
`manifestVersion: 1`. Unknown versions, document types, enum values, fields, or
unsupported combinations fail closed. A compatible extension requires a new
protocol version rather than an optional unknown field.

## Validation Model

Schema validation is necessary but not sufficient. A consumer must also apply
the semantic and cross-document rules implemented by the v1 validator. Those
rules include:

- Exact supported versions and bounded document sizes.
- Safe relative paths under the declared `inputs/` or `outputs/` directory.
- Unique object identities and retained-content paths, valid parent references,
  and no parent cycles.
- A nonempty available manifest containing at least one `rawGeometry` object,
  with proven export observation and proven mapping for every object.
- Exact manifest, ordered input-set, and invocation identities.
- Unique lexicographically sorted capability identities.
- Exact settings, manifest, input-kind, input-schema, invocation, reported
  identity, output-role, output-path, and output-media-type bindings across
  documents.
- A success result with one declared `generatedProject` output and no errors, or
  a failure result with at least one structured error and no output.
- The declared candidate size not exceeding the request limit.

Schema constraints duplicate semantic rules where Draft 2020-12 can express
them clearly. The semantic validator remains required for ordering,
cross-document equality, identity recomputation, and graph rules.

Protocol validation does not prove that declared files exist or that their
bytes match their declared length and SHA-256. The service-side runner must
perform those checks when execution is implemented.

## Identities And Ordering

Package, build, binary, dialect, capability, input kind, input schema, settings,
settings schema, provenance, normalization, and validation identities are
opaque visible-ASCII strings. The protocol transports and compares them; it
does not define target-specific values or infer compatibility from their text.

SHA-256 values are lowercase 64-character hexadecimal strings. Computed
protocol identities use RFC 8785 JSON Canonicalization Scheme bytes for this
preimage:

```json
{"domain":"domain-name","payload":{}}
```

The domains and payloads are:

| Identity | Domain | Payload |
| --- | --- | --- |
| Ordered input set | `generator-input-set-v1` | `protocolVersion`, `manifestVersion`, and the complete ordered `objects` array |
| Input manifest | `generator-input-manifest-v1` | Every manifest field except `manifestIdentity` |
| Invocation | `generator-invocation-v1` | Every request field except `invocationIdentity` |

Array order is significant. The service chooses and records object order; a
consumer must not derive it from filenames, display names, archive order, or an
unordered source. Capability identities use their required lexicographic order
to produce one deterministic request representation.

`contentIdentity` identifies retained content according to service policy,
while `sha256` identifies the exact declared bytes. Display names and source
filenames are optional metadata and never substitute for an immutable identity.

## File Layout And Atomicity

One invocation uses a fresh private root whose declared relative paths cannot
refer outside that root. Manifest, retained geometry, and settings paths are
beneath `inputs/`; the candidate path is beneath `outputs/`. The request and
result file paths are supplied by the later trusted-CLI invocation interface,
not selected from request content.

The invocation owner must begin with no candidate or final result at their
declared paths. Producers write each file to a private sibling temporary file,
finish and close it, and atomically rename it to the declared path. A generator
must finalize the candidate before atomically installing a success result. The
final result is the invocation commit marker:

- A success result is valid only when the declared candidate exists and its
  independently measured bytes, length, and SHA-256 match the result.
- A failure result contains no output. Any candidate or temporary file is
  uncommitted and must not be used.
- Missing or malformed results, unexpected process exits, leftover temporary
  files, undeclared files, and mismatched output are failures, never partial
  success.

The consumer reads only declared final paths after process completion and does
not publish a generator-created path directly. Exact CLI arguments and runner
behavior remain follow-up implementation work.

## Bounds And Failures

Requests and manifests are each limited to 1 MiB; results are limited to 256
KiB. A manifest has at most 256 objects. Results have at most 64 diagnostics and
64 errors. Each entry has bounded codes, messages, JSON Pointer instance paths,
and at most 16 unique bounded context entries. Diagnostics are informational or
warnings; failures use one of the neutral structured-error categories declared
by the schema.

A request that cannot be parsed may produce a failure result without invocation
or reported identities. Once those values are known, any reported value must
match the request. Unsupported identities or combinations produce structured
failure rather than fallback behavior.

## Role Boundary

Input objects use `rawGeometry` or `auxiliaryGeometry`. The sole output role is
`generatedProject`. These roles and their identities remain distinct even if
container formats or bytes happen to overlap. The protocol never relabels raw
geometry as a generated project and does not define a target dialect, feature,
schema, fixture, or validation fact.
