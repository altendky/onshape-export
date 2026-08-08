# Slicer Project Generators

> **Status: Proposed integration, not implemented.** The neutral protocol is a
> settled v1 contract; invocation flags, compatibility windows, and installation
> details remain unsettled.

## Terminology

- **Onshape geometry 3MF** is geometry exported by Onshape in the 3MF format.
  It is a raw Onshape export and is not assumed to contain a slicer's complete project state.
- **Slicer project 3MF** is a slicer-specific project archive containing geometry plus the metadata, configuration, and archive conventions expected by a slicer family.
  Bambu Studio, OrcaSlicer, and PrusaSlicer project files are separate dialect artifacts even where their formats overlap.
- **Geometry input** is the source-neutral input passed to a generator.
   It might eventually be STEP, STL, geometry-only 3MF, or a manifest plus several files.
   Onshape geometry 3MF is one candidate, not the required architecture. The
  [Onshape Geometry Input Characterization](onshape-geometry-input-characterization.md)
  supports an opaque grouped-payload boundary and bounded official-part/root-
  occurrence exports only. Its controlled selected-object follow-ups chose no
  direct-selector profile: comma-separated root and tail IDs did not causally select one
  ordered nested leaf, a root-leaf payload omitted Assembly placement, and the
  immutable-leaf profile therefore uses separate neutral placements. Expected
  matrix derivation and orchestration remain fail-closed pending their owning
  integrations.

The product and cache model must not label an Onshape geometry 3MF as a slicer project 3MF.
User-visible output kinds and media metadata should retain this distinction.

## Proposed Architecture

The Rust service would continue to own Onshape access, immutable source and configuration identity, raw export retention, queueing, publication, and cache index state.
Slicer-aware transformation would run in external CLI generators:

```text
Onshape API -> retained raw geometry -> neutral generator input
                                         |
                       trusted external CLI process
                                         |
                  candidate project 3MF + result metadata
                                         |
                          service-side validation
                                         |
                            artifact publication
```

The approved generator CLI is trusted to the same degree as the service's own
code. The process boundary preserves repository ownership, source-ingress
restrictions, provenance, release, distribution, and license responsibilities;
it also defines a source-neutral interface and is not a runtime security
boundary. A generator result does not authorize publication until the service
validates source-neutral archive and identity requirements and completes any
required target-aware check through separately approved target-side validation
inputs or tools.

The proposed generator responsibilities are:

- Accept the protocol's ordered source-neutral geometry input set and explicit
  project settings.
- Produce exactly one candidate project 3MF for its declared slicer dialect.
- Report generator, protocol, dialect, capability, and provenance identities.
- Reject unsupported requests rather than silently dropping project features.
- Emit machine-readable diagnostics.
- Produce candidates only; service publication remains a separate decision.

The service would be responsible for:

- Preparing the geometry input and canonical request.
- Selecting a compatible generator exclusively through a reviewed service-owned
  approved-generator manifest; declared capabilities are verification evidence,
  not authorization.
- Verifying generator package/build identity, protocol, dialect, provenance, and capability metadata against a service-owned approved-generator manifest rather than trusting self-reported identity alone.
- Enforcing output-size and generic archive limits before publication.
- Independently hashing the candidate output and comparing it with the generator's validation report before publication.
- Recording the complete recipe in cache and artifact metadata.

The service owns the source-neutral protocol and its request, result, error, and
schema definitions. Those contracts may describe transport, identities, input
roles, settings, diagnostics, output constraints, and output roles, but must not embed
target-derived slicer dialect facts. The exact v1 contract is the normative
[Neutral Generator Protocol](neutral-generator-protocol.md).

## Generator Repository And Binaries

The canonical target-side repository is
[`slicer-project-generators`](https://github.com/altendky/slicer-project-generators).
Generator-local source-informed derivative development, target-derived slicer
schemas and fixtures, package builds, release evidence, and provenance sets
belong in that repository under its pinned normative
[Slicer Project Generator Provenance Policy](https://github.com/altendky/slicer-project-generators/blob/ced6585d5a8e1a47690e7eabdf92beaa7fea7fc4/docs/src/project/slicer-project-generator-provenance.md).
Package layout, binary names, and target-derived implementation facts remain
owned there. The service records only reviewed immutable package and binary
identities in its approved-generator manifest. Shared implementation must not
blur capability, dialect, or provenance records.

Each dialect produces a separate immutable artifact.
A Bambu project, Orca project, and Prusa project must not share an artifact identity merely because their bytes or archive members happen to match.
No generator should claim another slicer's compatibility unless that combination has explicit validation evidence.

## Conceptual CLI Boundary

Protocol v1 uses a file-backed JSON exchange rather than a streaming or
long-running service protocol. The later runner will invoke a generator with
paths for:

- A request JSON file.
- A result JSON file.

The request identifies the protocol version, opaque expected identities, the
ordered geometry input set and roles, settings, and one output declaration. The
manifest, retained objects, and settings use declared safe paths under `inputs/`;
the candidate uses its declared path under `outputs/`, all within one private
invocation root. Request/result path arguments belong to the later trusted CLI
interface.
The
result reports success or structured failure, exact reported identities,
candidate output hash, and bounded diagnostics.
The service would independently recompute the candidate output hash rather than trust the report.
Field names, JSON Schema, atomic-write rules, and diagnostic format are defined
by protocol v1. Exact process flags and runner implementation remain open.

## Capabilities And Versioning

Generators may expose machine-readable capabilities for comparison before work
is scheduled, but only an exact reviewed approved-generator manifest entry
authorizes selection.
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
A multi-blob invocation must not infer source-object identity from archive order,
filenames, display names, or result-array position. Only mappings proven under
the characterization rules may populate that ordered input set. As of the
controlled selected-object follow-ups, no profile satisfies the required Part
Studio and complete Assembly occurrence-path contract, so production multi-object
dispatch remains unavailable.
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

Before publication, service validation follows the normative
[integration policy](slicer-project-generator-integration.md), including neutral
protocol consistency and independent candidate hashing. Generator raw-input
bounds and final target-aware self-validation are owned by
[`slicer-project-generators#8`](https://github.com/altendky/slicer-project-generators/issues/8)
and
[`slicer-project-generators#9`](https://github.com/altendky/slicer-project-generators/issues/9).
Target schemas and fixtures remain in that repository.
A process exit code or successful ZIP parse alone is insufficient.

## Trusted CLI Execution

The service directly invokes only exact approved generator CLI package bytes at
a fixed configured path and does not use a shell. The CLI exchanges declared
request, input, result, and output files through the neutral protocol. Ordinary
runner behavior handles success, structured failure, process crash, unexpected
exit, and missing or malformed results.

No runtime sandbox or containment mechanism is required. The approved CLI may
run with the same ambient runtime access as the service because it is trusted to
the same degree as service code. Independent result validation remains a
publication-integrity gate, not a hostile-code boundary. The service stages and
publishes independently accepted bytes rather than forwarding a
generator-created path.

## Upgrade Overview

Generator release and service publication are separate gates:

1. The generator repository completes its source-access, provenance, build, and
   release review and releases exact immutable package bytes.
2. The service acquires and hashes those exact bytes without rebuilding them.
3. The service reviews the interface, distribution, trusted CLI, validation,
   deployment, cache, and publication behavior for the exact package identity.
4. The service adds an approved immutable binding to its approved-generator
   manifest and may make that package selectable.
5. A trusted external CLI invocation produces a private candidate project 3MF.
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
