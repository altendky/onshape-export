# Slicer Project Generator Integration Policy

> **Status: Normative.** This is the service-side policy for integrating,
> approving, running, and publishing output from slicer project generators.
> The neutral protocol, static deployment identity, pure processing recipe,
> immutable recipe/occurrence persistence, and exact ready-cache lookup are
> implemented. Runtime orchestration, CLI execution, and upload/readiness
> verification remain separate work.

This policy does not provide legal advice. Repository or process separation does
not itself decide whether licenses are compatible; qualified review is required
for licensing conclusions.

## Authority And Ownership

This MIT OR Apache-2.0 repository owns the source-neutral generator protocol and
schemas, one static deployed-generator configuration, trusted external CLI
invocation, source-neutral result and candidate-byte verification, cache,
generated-artifact publication and revocation, and interface, distribution, and
deployment review.

The canonical target-side project is
[`slicer-project-generators`](https://github.com/altendky/slicer-project-generators).
It owns target-derived generator implementation, slicer dialect schemas and
fixtures, provenance evidence, package builds, and generator package release
decisions. Its pinned normative
[Slicer Project Generator Provenance Policy](https://github.com/altendky/slicer-project-generators/blob/ced6585d5a8e1a47690e7eabdf92beaa7fea7fc4/docs/src/project/slicer-project-generator-provenance.md)
governs source access, classification, evidence, target development, builds, and
releases. Those procedures are not duplicated here.

A generator package release is only a candidate for service integration. It
does not approve service deployment or generated-artifact publication. Service
approval likewise does not replace generator provenance or release review.

## Repository Ingress Boundary

Generator-local source-informed derivative development may proceed in
`slicer-project-generators` under its pinned policy. That does not relax the
stricter repository-root `AGENTS.md` boundary, which applies to every
contribution and tool here. Do not inspect relevant GPL- or AGPL-covered slicer
implementation source for work in this repository. Do not add target-derived
implementation, constants, schemas, fixtures, templates, or source-informed
summaries here.

The neutral protocol may express service-owned transport, request, result,
error, identity, diagnostic, and output-limit concepts. It must not absorb target-derived
slicer facts merely to avoid the target repository boundary.

The normative [Neutral Generator Protocol](neutral-generator-protocol.md)
defines the versioned document set, identity rules, file roles, bounds, and
atomic commit behavior. This policy remains authoritative for approval,
execution, validation, cache, publication, and revocation outside that exchange.
The normative
[Neutral Generator Settings V2](neutral-generator-settings-v2.md) defines only
its closed document, normalization, identity, and pure validation contracts.

## Static Deployed Generator

The service uses exactly one reviewed, service-owned
[deployed-generator configuration](deployed-generator.md). The closed document
binds:

- Exact immutable generator package bytes and cryptographic hash.
- Protocol version and generator binary identity.
- Opaque slicer dialect identity.
- Immutable provenance-set identity.
- Approved capability identifiers and revisions.
- Approved geometry-input kinds and schema versions.
- Generator-owned final validation and normalization identities.

Invocation-specific settings identity and candidate output hashes do not belong
in this document.
Self-reported generator metadata is evidence to compare, not authorization.
The executable path is operational and excluded from immutable identity. Mutable
tags, channels, package names, or filesystem paths are insufficient identities.

An absent configuration makes generator output unavailable. A specified invalid
document or executable is a configured-process startup failure. There is no
registry, ranking, fallback, discovery, acquisition, approval history,
revocation state, or rollback state in this document.

## Service Approval Gates

Approval applies to exact released package bytes. Rebuilding, repackaging, or
changing any byte requires a new package identity and service review. Before the
static binding is deployed, the service review must verify:

- The exact package and release identities and their cryptographic hashes.
- The immutable provenance-set identity and generator release record.
- Protocol, dialect, input-kind, and capability metadata consistency.
- Distribution rights, notices, acquisition path, and package retention.
- The source-neutral interface and absence of target-derived facts in service
  code and schemas.
- Trusted CLI and deployment configuration for the exact package.
- Generator-owned final self-validation, source-neutral candidate hashing,
  compatibility, and output-limit behavior.
- Cache and publication behavior.

Unknown, disputed, provisional, incomplete, or inconsistent records block
service approval, deployment, capability advertisement, and
publication. Do not invent an identity, release, hash, record, result, or review
to satisfy a gate.

## Runtime And Validation

Invoke the exact configured generator CLI directly at its fixed path,
without a shell, using the declared file-backed request, input, result, and
output protocol. Handle success, structured failure, process crash, unexpected
exit, and missing or malformed results as ordinary runner outcomes.

The configured generator CLI is trusted to the same degree as the service's own
code. The external-process boundary preserves repository ownership,
source-ingress restrictions, provenance, release, distribution, and license
responsibilities and defines a source-neutral interface; it is not a runtime
security boundary. The service does not require sandboxing, containment,
hostile-code defenses, credential stripping, network or filesystem isolation,
or process resource limits for a configured trusted generator CLI.

CLI trust does not make a result sufficient for publication. The service must:

- Recompute the exact candidate artifact hash.
- Match reported package, protocol, dialect, provenance, capability,
  normalization, and validation identities to the configured expected bindings.
- Verify declared candidate existence, measured length and SHA-256, upload,
  storage, and publication policy.
- Reject missing, extra, malformed, incompatible, or unsupported output rather
  than silently substituting another dialect or raw Onshape geometry.

The CLI produces candidate files only. The service, not the CLI, owns private
staging and publication and publishes only independently accepted bytes rather
than forwarding a generator-created path.

The service owns source-neutral protocol, identity, candidate-byte,
orchestration, and publication checks, not target-derived validation facts. The
generator owns final target-aware self-validation and reports its exact immutable
`validationIdentity`. Do not copy target schemas, validators, fixtures, or
evidence into this repository, and do not add a second service-side target
validator.

Expected neutral placement derivation and source/path proof are owned by
[#173](https://github.com/altendky/onshape-export/issues/173). Manifest-order
orchestration, settings construction, and contextual-validator invocation are
owned by [#175](https://github.com/altendky/onshape-export/issues/175). Generator
raw-input bounds and final target-aware self-validation belong to
[`slicer-project-generators#8`](https://github.com/altendky/slicer-project-generators/issues/8)
and
[`slicer-project-generators#9`](https://github.com/altendky/slicer-project-generators/issues/9),
not to the neutral settings validator.

A successful process exit, matching self-reported hash, or parseable ZIP is not
sufficient for publication.

## Cache, Publication, And Revocation

The service computes the source-neutral `generator-processing-recipe-v1` from
the exact static deployed-generator identity, requested compatibility and
unsupported-case decision, complete validated ordered protocol-v1 manifest,
normalized settings-v2 document, settings identity, and settings-schema
identity. The recipe also contains the validated protocol invocation, including
manifest/settings staging declarations, canonical settings content metadata,
invocation identity, and complete output declaration. The canonical recipe hash
is both the generator processing identity and the post-process component of the
generated artifact-set identity. Singular request/raw-payload artifact identity
fields are omitted because one generator recipe may contain multiple retained
inputs.

Persist the exact canonical recipe JSON and its ordered logical occurrence
records before reuse. Each occurrence retains object/content identity,
SHA-256/length, staged path, transport role, display name, mapping/provenance,
and placement. Equal bytes may share retained content, but occurrence identity,
order, path, and semantic evidence never collapse.

Exact cache reuse requires a known supported recipe, the exact derived linked
artifact-set identity with equal generator and post-process identities, absent
singular acquisition identities, no supersession markers, and an exact complete
primary-file record. Generator-linked artifact sets cannot be restaged under an
existing identity. Supersession changes selection but preserves immutable
recipe, occurrence, artifact-set, and file history. These
implemented persistence and lookup rules do not perform mutable approval checks,
runtime orchestration, runner behavior, upload verification, or target-aware
validation; those remain their separately owned gates. Published artifact bytes
are immutable in normal operation.

The static deployed-generator identity is immutable processing input, while the
decision to deploy or remove its configuration is operational policy. Changing
any immutable configured field creates a different static identity. Removing
configuration stops new generator work. The v1 document itself defines no
lifecycle, revocation, or rollback state.

Service approval and publication policy remain mutable outside that document.
Before cached reuse or publication, confirm that the exact static binding remains
approved. Revoking approval must stop new work and publication, identify affected
artifacts, and explicitly supersede or withdraw them according to the recorded
reason and applicable legal, safety, or operational requirements. Revocation
does not mutate an artifact's immutable identity or bytes.

## Approval And Publication Sequence

1. `slicer-project-generators` completes provenance, build, and release review
   under its canonical policy and releases exact immutable package bytes.
2. The service acquires and hashes those exact bytes without rebuilding them.
3. The service completes interface, distribution, trusted CLI, validation,
   cache, deployment, and publication review for that exact package identity.
4. Deployment installs those exact bytes and writes the one closed static
   deployed-generator document.
5. A trusted external CLI invocation produces a private candidate project
   artifact.
6. The service independently validates and hashes the candidate, records its
   complete recipe, and only then publishes the exact validated artifact bytes.
7. Removing or replacing deployment configuration affects future work without
   mutating existing immutable artifact bytes.

Generator package release, service deployment approval, and generated
artifact publication are three separate decisions.
