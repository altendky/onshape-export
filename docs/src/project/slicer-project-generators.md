# Slicer Project Generators

> **Status: Proposed, not implemented.** This page describes a direction for prototyping.
> Protocol fields, invocation flags, compatibility windows, and installation details are not settled commitments.

## Terminology

- **Onshape geometry 3MF** is geometry exported by Onshape in the 3MF format.
  It is a raw Onshape export and is not assumed to contain a slicer's complete project state.
- **Slicer project 3MF** is a slicer-specific project archive containing geometry plus the metadata, configuration, and archive conventions expected by a slicer family.
  Bambu Studio, OrcaSlicer, and PrusaSlicer project files are separate dialect artifacts even where their formats overlap.
- **Geometry input** is the source-neutral input passed to a generator.
  It might eventually be STEP, STL, geometry-only 3MF, or a manifest plus several files.
  Onshape geometry 3MF is one candidate, not the required architecture.

The product and cache model must not label an Onshape geometry 3MF as a slicer project 3MF.
User-visible output kinds and media metadata should retain this distinction.

## Proposed Architecture

The Rust service would continue to own Onshape access, immutable source and configuration identity, raw export retention, queueing, publication, and cache index state.
Slicer-aware transformation would run in external CLI generators:

```text
Onshape API -> retained raw geometry -> neutral generator input
                                         |
                      sandboxed external CLI process
                                         |
                  candidate project 3MF + result metadata
                                         |
                          service-side validation
                                         |
                            artifact publication
```

The process boundary is also a trust boundary.
Generator output is untrusted until the service validates source-neutral archive
and identity requirements and completes any required target-aware check through
separately approved target-side validation inputs or tools.
A generator must not receive Onshape or object storage credentials and must not publish artifacts directly.

The proposed generator responsibilities are:

- Accept one source-neutral geometry input and explicit project settings.
- Produce exactly one candidate project 3MF for its declared slicer dialect.
- Report generator, protocol, dialect, capability, and provenance identities.
- Reject unsupported requests rather than silently dropping project features.
- Avoid network access and undeclared filesystem dependencies during a build.
- Emit machine-readable diagnostics without exposing secrets or host paths.

The service would be responsible for:

- Preparing the geometry input and canonical request.
- Selecting a compatible generator through declared capabilities.
- Verifying generator package/build identity, protocol, dialect, provenance, and capability metadata against a service-owned approved-generator manifest rather than trusting self-reported identity alone.
- Applying time, memory, CPU, disk, process, and output-size limits.
- Independently hashing the candidate output and comparing it with the generator's validation report before publication.
- Recording the complete recipe in cache and artifact metadata.

The service owns the source-neutral protocol and its request, result, error, and
schema definitions. Those contracts may describe transport, identities, input
roles, settings, limits, diagnostics, and output roles, but must not embed
target-derived slicer dialect facts. Exact JSON fields and schemas remain
unsettled until a prototype validates the boundary.

## Generator Repository And Binaries

The canonical target-side repository is
[`slicer-project-generators`](https://github.com/altendky/slicer-project-generators).
It maps three independent packages and binaries:

| Slicer dialect | Package | Binary |
| --- | --- | --- |
| Bambu Studio | `crates/bambu-studio` | `slicer-project-generator-bambu-studio` |
| OrcaSlicer | `crates/orca-slicer` | `slicer-project-generator-orca-slicer` |
| PrusaSlicer | `crates/prusa-slicer` | `slicer-project-generator-prusa-slicer` |

These boundaries do not claim implemented capabilities or compatibility.
Generator-local source-informed derivative development, target-derived slicer
schemas and fixtures, package builds, release evidence, and provenance sets
belong in that repository under its pinned normative
[Slicer Project Generator Provenance Policy](https://github.com/altendky/slicer-project-generators/blob/ced6585d5a8e1a47690e7eabdf92beaa7fea7fc4/docs/src/project/slicer-project-generator-provenance.md).
Shared implementation must not blur capability, dialect, or provenance records.

Each dialect produces a separate immutable artifact.
A Bambu project, Orca project, and Prusa project must not share an artifact identity merely because their bytes or archive members happen to match.
No generator should claim another slicer's compatibility unless that combination has explicit validation evidence.

## Conceptual CLI Boundary

The initial prototype should prefer a file-backed JSON exchange over a streaming or long-running service protocol.
Conceptually, the service would invoke a generator with paths for:

- A request JSON file.
- A geometry file or input-directory manifest.
- A result JSON file.
- A candidate output 3MF file.
- A private working directory.

The request would identify the protocol version, requested dialect and features, the geometry input's existing retained-content identity and role, and canonical project settings.
The result would report success or structured failure, actual capabilities used, generator build identity, dialect revision, candidate output hash, warnings, and provenance-set version.
The service would independently recompute the candidate output hash rather than trust the report.
Exact flags, field names, JSON Schema, atomic-write rules, and diagnostic format remain open until a prototype tests crash behavior and portability.

## Capabilities And Versioning

Generators should expose machine-readable capabilities before work is scheduled.
Capabilities should use granular, revisioned identities rather than one broad
format-support flag.
The target repository owns capability identifiers, revisions, and evidence. The
neutral service protocol should transport those opaque identities without
inventing target features locally.

Compatibility should account for at least:

- CLI protocol version.
- Generator package and build identity.
- Slicer dialect and dialect revision.
- Supported capability identifiers and capability revisions.
- Supported geometry input kinds and schema versions.
- Validation and normalization policy versions.
- Tested slicer versions or compatibility window.
- Provenance-set version covering the requested capabilities.

Protocol compatibility and artifact compatibility are different.
A service may be able to invoke a generator while refusing a requested capability or declining to publish output for an unvalidated slicer version.

## Cache Identity

Project-3MF post-processing identity must include the generator build or immutable package identity, protocol version, dialect revision, provenance-set version, exercised capability revisions, geometry-input media type and kind, neutral-IR or input-schema version, relevant parser identity/version, parser-normalization identity/version, and validation policy/tool versions.
Requested dialect, requested capabilities, and canonical project settings are logical export-option identity.
The current single retained geometry input remains bound through the existing raw-payload/content identity, not a duplicate content hash inside processing-recipe or policy identity.
If a future invocation accepts multiple input blobs, it must use a separate explicit input-set or invocation identity rather than placing their hashes in processing policy.
A service-owned approved-generator manifest must bind the allowed package/build identity to its approved protocol, dialect, provenance set, and capabilities.
Candidate output hashes are independently computed per invocation and do not belong in the approved-generator manifest.

Changing any output-affecting identity creates a new candidate artifact and may supersede the active artifact.
It must not overwrite a published object.
See the [Forward-Looking Cache Model](cache-model.md) for cache layering.

## Determinism And Validation

The desired determinism level is unresolved. The prototype must distinguish:

- Byte-for-byte reproducibility.
- Canonical archive reproducibility after approved normalization.
- Semantic equivalence accepted by pinned slicer versions.

Until the projects choose a level, generators should remove controllable nondeterminism, report unavoidable sources, and preserve enough inputs and versions to reproduce or diagnose a build.
Normalization must not conceal a semantic change.

Before publication, service-local validation should include safe archive paths,
member count and size limits, neutral protocol and identity consistency, and no
unexpected external references. Target-aware project structure, dialect, and
compatibility checks must use separately approved immutable validation inputs or
tools from the generator repository; target schemas and fixtures remain there.
Their packaging and execution boundary are unresolved. The service must
independently hash the candidate output and compare it with the generator's
validation report.
A process exit code or successful ZIP parse alone is insufficient.

## Sandboxing

Generators process potentially complex archives and run outside the service's trusted implementation.
The intended runtime denies network access and service credentials, uses a fresh restricted working directory, passes only declared input files, limits resources and subprocesses, captures bounded diagnostics, and publishes no generator-created path directly.
Platform-specific containment and whether generators run as local processes, containers, or another mechanism remain open.

## Upgrade Overview

Generator release and service publication are separate gates:

1. The generator repository completes its source-access, provenance, build, and
   release review and releases exact immutable package bytes.
2. The service acquires and hashes those exact bytes without rebuilding them.
3. The service reviews the interface, distribution, sandbox, validation,
   deployment, cache, and publication behavior for the exact package identity.
4. The service adds an approved immutable binding to its approved-generator
   manifest and may make that package selectable.
5. A sandboxed invocation produces a private candidate project 3MF.
6. The service independently validates and hashes the candidate and publishes
   only those exact validated artifact bytes.

Rollback should select a previously retained, still-approved generator and artifact set, not mutate already published bytes.

## Related Policy And Questions

Service integration, approval, publication, and revocation are governed by the
normative [Slicer Project Generator Integration Policy](slicer-project-generator-integration.md).
Target-side source access, implementation, schemas, fixtures, builds, evidence,
and release review are governed by the pinned generator policy above. Process
or repository separation does not by itself determine license compatibility.
Unsettled protocol and implementation choices are tracked in [Open Questions](open-questions.md).
