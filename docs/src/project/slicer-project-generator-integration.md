# Slicer Project Generator Integration Policy

> **Status: Normative.** This is the service-side policy for integrating,
> approving, running, and publishing output from slicer project generators.
> The generator architecture remains proposed and is not implemented here.

This policy does not provide legal advice. Repository or process separation does
not itself decide whether licenses are compatible; qualified review is required
for licensing conclusions.

## Authority And Ownership

This MIT OR Apache-2.0 repository owns the source-neutral generator protocol and
schemas, approved-generator manifest, runtime sandbox, independent output
validation and hashing, cache, generated-artifact publication and revocation,
and interface, distribution, and deployment review.

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
error, identity, and resource-limit concepts. It must not absorb target-derived
slicer facts merely to avoid the target repository boundary.

## Approved-Generator Manifest

The service must select generators only through a reviewed, service-owned
approved-generator manifest. Each entry must bind at least:

- Exact immutable generator package bytes and cryptographic hash.
- Generator repository and immutable release identity.
- Protocol version and generator binary identity.
- Slicer dialect and dialect revision.
- Immutable provenance-set identity.
- Approved capability identifiers and revisions.
- Approved geometry-input kinds and schema versions.
- Validation, normalization, and compatibility policy identities, including
  exact approved target-side validation inputs or tools when required.
- Service approval record, date, and review status.

Invocation-specific candidate output hashes do not belong in this manifest.
Self-reported generator metadata is evidence to compare, not authorization.
Mutable tags, channels, package names, or filesystem paths are insufficient
identities.

## Service Approval Gates

Approval applies to exact released package bytes. Rebuilding, repackaging, or
changing any byte requires a new package identity and service review. Before an
entry becomes selectable, the service review must verify:

- The exact package and release identities and their cryptographic hashes.
- The immutable provenance-set identity and generator release record.
- Protocol, dialect, input-kind, and capability metadata consistency.
- Distribution rights, notices, acquisition path, and package retention.
- The source-neutral interface and absence of target-derived facts in service
  code and schemas.
- Sandbox and deployment configuration for the exact package.
- Independent validation, hashing, compatibility, and resource-limit results.
- Cache, publication, rollback, and revocation behavior.

Unknown, disputed, provisional, incomplete, or inconsistent records block
service approval, selectability, deployment, capability advertisement, and
publication. Do not invent an identity, release, hash, record, result, or review
to satisfy a gate.

## Runtime And Validation

Run an approved generator with no service credentials or network access, a
fresh restricted work directory, declared read-only inputs, bounded diagnostics,
and explicit elapsed-time, CPU, memory, disk, file/member-count, output-size,
and subprocess limits. The containment mechanism remains an implementation
decision and must be reviewed before deployment.

Treat every generator result as untrusted. The service must independently:

- Recompute the exact candidate artifact hash.
- Match reported package, protocol, dialect, provenance, and capability
  identities to the approved-generator manifest.
- Enforce safe archive paths, member and expanded-size limits, and publication
  policy.
- Reject missing, extra, malformed, incompatible, or unsupported output rather
  than silently substituting another dialect or raw Onshape geometry.

The service owns the validation policy, orchestration, and publication decision,
not target-derived validation facts. Local code may perform source-neutral
protocol, identity, archive-safety, resource-limit, and hash validation. Any
target-aware structure, dialect, or compatibility check must consume an exact,
separately approved target-side validation input or tool released from
`slicer-project-generators`; its identity belongs in the approved-generator
manifest and processing recipe. Do not copy its target schemas or fixtures into
this repository. The packaging and execution boundary for such validation are
unsettled. Until an independent target-aware check required by publication
policy exists, publication remains blocked.

A successful process exit, matching self-reported hash, or parseable ZIP is not
sufficient for publication.

## Cache, Publication, And Revocation

Keep exact generator package identity, protocol, dialect, provenance set,
exercised capabilities, input identity, normalization, and validation policy in
the processing and artifact recipe described by the
[Forward-Looking Cache Model](cache-model.md). Published artifact bytes are
immutable in normal operation.

Approval is mutable service policy, not artifact identity. Recheck manifest
authorization before cached reuse or publication. Revoking an entry must stop
new selection and publication, identify affected generated artifacts, and
supersede or withdraw them according to the recorded reason and applicable
legal, safety, or operational requirements. An unchanged exact binding may keep
its existing artifact identity when unrelated manifest entries change.

Rollback selects previously retained, still-approved exact generator and
artifact bytes. It must not rebuild a release or mutate published objects in
place.

## Approval And Publication Sequence

1. `slicer-project-generators` completes provenance, build, and release review
   under its canonical policy and releases exact immutable package bytes.
2. The service acquires and hashes those exact bytes without rebuilding them.
3. The service completes interface, distribution, sandbox, validation, cache,
   deployment, and publication review for that exact package identity.
4. The service records the approved immutable identities in the
   approved-generator manifest and may make the generator selectable.
5. A sandboxed invocation produces a private candidate project artifact.
6. The service independently validates and hashes the candidate, records its
   complete recipe, and only then publishes the exact validated artifact bytes.
7. Revocation or changed approval prevents further selection or publication and
   triggers cache and artifact re-evaluation.

Generator package release, service approval/selectability, and generated
artifact publication are three separate decisions.
