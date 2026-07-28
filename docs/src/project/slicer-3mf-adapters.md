# Slicer Project 3MF Adapters

> **Status: Proposed, not implemented.** This page describes a direction for prototyping.
> Command names, JSON fields, package layouts, compatibility windows, and installation details are not settled protocol commitments.

## Terminology

- **Onshape geometry 3MF** is geometry exported by Onshape in the 3MF format.
  It is a raw Onshape export and is not assumed to contain a slicer's complete project state.
- **Slicer project 3MF** is a slicer-specific project archive containing geometry plus the metadata, configuration, and archive conventions expected by a slicer family.
  Bambu Studio, OrcaSlicer, and PrusaSlicer project files are separate dialect artifacts even where their formats overlap.
- **Geometry input** is the source-neutral input passed to an adapter.
  It might eventually be STEP, STL, geometry-only 3MF, or a manifest plus several files.
  Onshape geometry 3MF is one candidate, not the required architecture.

The product and cache model must not label an Onshape geometry 3MF as a slicer project 3MF.
User-visible output kinds and media metadata should retain this distinction.

## Proposed Architecture

The Rust service would continue to own Onshape access, immutable source and configuration identity, raw export retention, queueing, publication, and cache index state.
Slicer-aware transformation would run in external CLI adapters:

```text
Onshape API -> retained raw geometry -> neutral adapter input
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
Adapter output is untrusted until the service validates archive paths, sizes, structure, declared dialect, and other publication requirements.
An adapter must not receive Onshape or object storage credentials and must not publish artifacts directly.

The proposed adapter responsibilities are:

- Accept one source-neutral geometry input and explicit project settings.
- Produce exactly one candidate project 3MF for its declared slicer dialect.
- Report adapter, protocol, dialect, capability, and provenance identities.
- Reject unsupported requests rather than silently dropping project features.
- Avoid network access and undeclared filesystem dependencies during a build.
- Emit machine-readable diagnostics without exposing secrets or host paths.

The service would be responsible for:

- Preparing the geometry input and canonical request.
- Selecting a compatible adapter through declared capabilities.
- Verifying adapter package/build identity, protocol, dialect, provenance, and capability metadata against a service-owned approved-adapter manifest rather than trusting self-reported identity alone.
- Applying time, memory, CPU, disk, process, and output-size limits.
- Independently hashing the candidate output and comparing it with the adapter's validation report before publication.
- Recording the complete recipe in cache and artifact metadata.

## Separate Adapters And Artifacts

Bambu Studio, OrcaSlicer, and PrusaSlicer should have separately versioned CLI adapters.
They may share code in their own implementation project when licensing and evidence permit, but shared implementation must not blur capability or provenance records.

Each dialect produces a separate immutable artifact.
A Bambu project, Orca project, and Prusa project must not share an artifact identity merely because their bytes or archive members happen to match.
No adapter should claim another slicer's compatibility unless that combination has explicit validation evidence.

## Conceptual CLI Boundary

The initial prototype should prefer a file-backed JSON exchange over a streaming or long-running service protocol.
Conceptually, the service would invoke an adapter with paths for:

- A request JSON file.
- A geometry file or input-directory manifest.
- A result JSON file.
- A candidate output 3MF file.
- A private working directory.

The request would identify the protocol version, requested dialect and features, the geometry input's existing retained-content identity and role, and canonical project settings.
The result would report success or structured failure, actual capabilities used, adapter build identity, dialect revision, candidate output hash, warnings, and provenance-set version.
The service would independently recompute the candidate output hash rather than trust the report.
Exact flags, field names, JSON Schema, atomic-write rules, and diagnostic format remain open until a prototype tests crash behavior and portability.

## Capabilities And Versioning

Adapters should expose machine-readable capabilities before work is scheduled.
Capabilities should be granular project features, not a single `supports_3mf` boolean.
Examples include plate layout, per-object settings, filament mapping, printer/process presets, thumbnails, and modifier volumes.

Compatibility should account for at least:

- CLI protocol version.
- Adapter package and build identity.
- Slicer dialect and dialect revision.
- Supported capability identifiers and capability revisions.
- Supported geometry input kinds and schema versions.
- Validation and normalization policy versions.
- Tested slicer versions or compatibility window.
- Provenance-set version covering the requested capabilities.

Protocol compatibility and artifact compatibility are different.
A service may be able to invoke an adapter while refusing a requested capability or declining to publish output for an unvalidated slicer version.

## Cache Identity

Project-3MF post-processing identity must include the adapter build or immutable package identity, protocol version, dialect revision, provenance-set version, exercised capability revisions, geometry-input media type and kind, neutral-IR or input-schema version, relevant parser identity/version, parser-normalization identity/version, and validation policy/tool versions.
Requested dialect, requested capabilities, and canonical project settings are logical export-option identity.
The current single retained geometry input remains bound through the existing raw-payload/content identity, not a duplicate content hash inside processing-recipe or policy identity.
If a future invocation accepts multiple input blobs, it must use a separate explicit input-set or invocation identity rather than placing their hashes in processing policy.
A service-owned approved-adapter manifest must bind the allowed package/build identity to its approved protocol, dialect, provenance set, and capabilities.
Candidate output hashes are independently computed per invocation and do not belong in the approved-adapter manifest.

Changing any output-affecting identity creates a new candidate artifact and may supersede the active artifact.
It must not overwrite a published object.
See the [Forward-Looking Cache Model](cache-model.md) for cache layering.

## Determinism And Validation

The desired determinism level is unresolved. The prototype must distinguish:

- Byte-for-byte reproducibility.
- Canonical archive reproducibility after approved normalization.
- Semantic equivalence accepted by pinned slicer versions.

Until the project chooses a level, adapters should remove controllable nondeterminism, report unavoidable sources, and preserve enough inputs and versions to reproduce or diagnose a build.
Normalization must not conceal a semantic change.

Before publication, validation should include safe archive paths, member count and size limits, required project members, parseable metadata, no unexpected external references, declared dialect consistency, and compatibility fixtures against pinned slicer versions.
The service must independently hash the candidate output and compare it with the adapter's validation report.
A process exit code or successful ZIP parse alone is insufficient.

## Sandboxing

Adapters process potentially complex archives and run outside the service's trusted implementation.
The intended runtime denies network access and service credentials, uses a fresh restricted working directory, passes only declared input files, limits resources and subprocesses, captures bounded diagnostics, and publishes no adapter-created path directly.
Platform-specific containment and whether adapters run as local processes, containers, or another mechanism remain open.

## Upgrade Overview

An adapter upgrade should be treated as a recipe change:

1. Before source access or source-informed work, create provisional feature-level provenance, source-access, classification, and applicable license-review records for changed capabilities; establish the corresponding evidence process before independent or clean-room work.
2. In the isolated adapter project, run restricted, explicitly non-distributable development builds and tests to develop the implementation and collect evidence.
3. Complete the classification-specific pre-candidate source and provenance evidence, including applicable immutable source or requirements references, source-access or separation records, experiments, fixture hashes, authorship/review records, and the license/notice materials needed for final review.
4. Build one immutable, explicitly non-distributable release candidate and record its exact package hash and build identity without treating it as approved or selectable.
5. Validate those exact candidate bytes and record their protocol, dialect, provenance-set, capability, fixture, determinism, sandbox, and pinned-slicer metadata and results.
6. Conduct final provenance, qualified license/notice, and release review against the complete record for that exact candidate.
7. After approval, promote the exact validated candidate bytes without rebuilding, add their identity and approved metadata to the service-owned adapter manifest, and make them selectable without replacing retained packages in place.
8. Generate and validate new artifact sets with the approved release package.
9. Promote the new artifact sets and supersede old ones only after validation.

Rollback should select a previously retained adapter and artifact set, not mutate already published bytes.

## Related Policy And Questions

Every adapter capability, whether source-informed, independently derived, or clean-room, is subject to the normative [Slicer Adapter Provenance And Licensing Policy](slicer-adapter-provenance.md).
Process or repository separation does not by itself determine license compatibility.
Unsettled protocol and implementation choices are tracked in [Open Questions](open-questions.md).
