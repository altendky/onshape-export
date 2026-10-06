# Slicer Project Generators

> **Status: Partially implemented.** The neutral protocol, settings v2, static
> deployed-generator identity, pure processing recipe, ordered-occurrence
> persistence, exact cache-lookup contracts, trusted geometry acquisition,
> manifest/settings construction, source-neutral CLI runner, verified artifact
> publication, and reconciliation are implemented. Production worker/UI
> dispatch and real deployment remain unavailable.

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

The configured generator CLI is trusted to the same degree as the service's own
code. The process boundary preserves repository ownership, source-ingress
restrictions, provenance, release, distribution, and license responsibilities;
it also defines a source-neutral interface and is not a runtime security
boundary. A generator result does not authorize publication until the service
verifies source-neutral protocol identities and independently measures the
declared candidate bytes. Final target-aware self-validation belongs to the
generator.

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
- Loading exactly one reviewed static
  [deployed-generator configuration](deployed-generator.md); declared
  capabilities are verification evidence, not authorization.
- Verifying generator package/build/binary identity, protocol, dialect,
  provenance, and capability metadata against that static binding rather than
  trusting self-reported identity alone.
- Enforcing source-neutral protocol and declared output limits before
  publication.
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
identities and digests in its static deployed-generator document. Shared implementation must not
blur capability, dialect, or provenance records.

Each dialect produces a separate immutable artifact.
A Bambu project, Orca project, and Prusa project must not share an artifact identity merely because their bytes or archive members happen to match.
No generator should claim another slicer's compatibility unless that combination has explicit validation evidence.

## CLI Boundary

Protocol v1 uses a file-backed JSON exchange rather than a streaming or
long-running service protocol. The runner invokes the configured executable
directly with:

```text
--request request.json --result result.json
```

The working directory is one fresh private invocation root. Standard streams
use the null device; bounded result diagnostics are the generator's reporting
channel. The ordinary process execution timeout defaults to 600 seconds.

The request identifies the protocol version, opaque expected identities, the
ordered geometry input set and roles, settings, and one output declaration. The
manifest, retained objects, and settings use declared safe paths under `inputs/`;
the candidate uses its declared path under `outputs/`, all within one private
invocation root. The result reports success or structured failure, exact reported identities,
candidate output hash, and bounded diagnostics.
The service independently recomputes the candidate output hash before upload and
verifies stored bytes before readiness.
Field names, JSON Schema, atomic-write rules, and diagnostic format are defined
by protocol v1. Exact runner behavior is defined by the
[integration policy](slicer-project-generator-integration.md#trusted-runner-contract).

## Capabilities And Versioning

Generators may expose machine-readable capabilities as release evidence, but
runtime compatibility uses only the exact reviewed static deployed-generator
binding.
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

`generator-processing-recipe-v1` canonically binds the static deployed-generator
identity, requested compatibility and decision, complete validated ordered
protocol manifest, normalized settings, settings identities, and validated
invocation/output declaration. Retained bytes are represented through ordered
logical occurrences and their explicit input-set identity; content hashes are
not duplicated as separate policy fields.

Generator `optionsHash` identifies logical project-export intent from the output
format, requested dialect, ordered capability revisions, canonical settings
identity, and settings-schema identity. Package/build/binary, provenance,
normalization, and validation identities remain processing identity rather than
logical options.

The recipe contract can represent multiple retained inputs. The
[trusted immutable-leaf input constructor](onshape-generator-inputs.md) now
produces complete ordered manifests and neutral placement settings from the
supported Part Studio and rigid Assembly planning/acquisition handoff. It never
infers source-object identity from archive order, filenames, display names,
result-array position, or content equality. The historical aggregate-export
mapping limitations remain in force. Staging and production generator dispatch
remain follow-up work in #168.
The service-owned static deployed-generator identity binds package and binary
digests plus approved protocol, dialect, provenance, capabilities, input/schema,
normalization, and validation identities. Invocation settings and candidate
output hashes remain separate.

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

Before any future generator-artifact publication, service validation must follow
the normative [integration policy](slicer-project-generator-integration.md),
including neutral protocol consistency and independent candidate hashing.
Runtime candidate staging, validation, upload/readiness verification, and
publication are not implemented here. Generator raw-input
bounds and final target-aware self-validation are owned by
[`slicer-project-generators#8`](https://github.com/altendky/slicer-project-generators/issues/8)
and
[`slicer-project-generators#9`](https://github.com/altendky/slicer-project-generators/issues/9).
Target schemas and fixtures remain in that repository.
A process exit code or successful ZIP parse alone is insufficient.

## Trusted CLI Execution

The trusted runner invokes only exact statically configured generator CLI bytes
at the fixed configured path, without a shell, after an immediate executable
recheck. It consumes the constructed input bundle and prepared recipe, stages
all declared inputs separately, and exchanges request, result, and candidate
files through the neutral protocol. Exit code `0` requires success, `1` requires
structured failure, other codes are unexpected exits, and signal termination is
a crash. Missing, malformed, incompatible, or mismatched results fail closed.
Only independently measured successful bytes can leave the invocation, after
explicit root cleanup; timeout and cancellation kill and reap before cleanup.

No runtime sandbox or containment mechanism is required. The approved CLI may
run with the same ambient runtime access as the service because it is trusted to
the same degree as service code. Independent result validation remains a
publication-integrity gate, not a hostile-code boundary. The runner retains
independently accepted bytes rather than forwarding a generator-created path.
The separate publication API owns verified upload, readiness, reconciliation,
and supersession through the existing artifact lifecycle. Worker/UI dispatch
and real generator installation remain separate integrations. Runner tests use only
synthetic source-neutral executables and protocol fixtures.

## Upgrade Overview

Generator release and service publication are separate gates:

1. The generator repository completes its source-access, provenance, build, and
   release review and releases exact immutable package bytes.
2. The service acquires and hashes those exact bytes without rebuilding them.
3. The service reviews the interface, distribution, trusted CLI, validation,
   deployment, cache, and publication behavior for the exact package identity.
4. Deployment installs those exact bytes and writes the one closed static
   deployed-generator document.
5. A trusted external CLI invocation produces a private candidate project 3MF.
6. The service independently validates and hashes the candidate and publishes
   only those exact validated artifact bytes.

Replacing or removing the static deployment affects future work and does not
mutate already published bytes. The v1 configuration defines no rollback or
revocation lifecycle.

## Related Policy And Questions

Service integration, approval, publication, and revocation are governed by the
normative [Slicer Project Generator Integration Policy](slicer-project-generator-integration.md).
Target-side source access, implementation, schemas, fixtures, builds, evidence,
and release review are governed by the pinned generator policy above. Process
or repository separation does not by itself determine license compatibility.
Unsettled protocol and implementation choices are tracked in [Open Questions](open-questions.md).
