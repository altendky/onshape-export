# Slicer Project Generator Integration Policy

> **Status: Normative.** This is the service-side policy for integrating,
> approving, running, and publishing output from slicer project generators.
> The neutral protocol, static deployment identity, pure processing recipe,
> immutable recipe/occurrence persistence, and exact ready-cache lookup are
> implemented, alongside trusted input construction, CLI execution, verified
> artifact publication, and reconciliation. Worker/UI orchestration and real
> generator deployment remain separate work.

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

### Trusted Runner Contract

The source-neutral runner consumes the complete
[constructed input bundle](onshape-generator-inputs.md) and its prepared
processing recipe. Before staging, it requires a supported compatibility
decision, the exact configured static deployed-generator identity, and matching
manifest, canonical settings, contextual placements, and invocation bindings.
It derives expected generator identities from the configured document and the
invocation's settings identity, rather than accepting caller-replaced bindings.

The service chooses a staging parent and an ordinary execution timeout through
runner options. The default process execution timeout is 600 seconds. Each call
creates a fresh private root beneath that parent; the root is also the process
working directory. The fixed argument vector is:

```text
--request request.json --result result.json
```

The executable is always the configured absolute path. There is no shell,
discovery, fallback, alternate executable, work-directory flag, or job-time
package installation. Standard input, output, and error are connected to the
null device. The bounded protocol result is the sole generator diagnostic
channel; printed text is neither retained nor interpreted as a result.

The runner stages the manifest, canonical settings, and every ordered retained
object separately at their declared paths. Equal-byte objects retain distinct
paths. Each file is written to a private sibling temporary file, finished,
closed, and atomically renamed. Staged settings and geometry are independently
measured against declared lengths and SHA-256 values. The request is installed
last, after complete input staging and validation, with no candidate or result
present. Immediately before spawning, the runner repeats the configured
executable's regular-file, readability, executable-mode, and digest checks;
their existing deployed-generator failure classifications remain distinct.

Exit code `0` requires a valid success result; exit code `1` requires a valid
structured failure result. Other numeric exit codes are unexpected exits, and
signal termination is a process crash. Missing, oversized, or malformed results
are runner failures. Reported protocol version, invocation identity, and opaque
identity mismatches are distinct from malformed-result failures. Result parsing
and cross-document validation retain the neutral protocol's bounds and exact
output role, identity, path, media type, and maximum-length bindings. Structured
failure preserves the generator's bounded neutral errors and diagnostics and
returns no candidate.

After process completion, the final inventory may contain only regular declared
files and their required ancestor directories. Symlinks, other nonregular
entries, undeclared files or directories, and leftover temporary files fail the
invocation. A structured failure may leave a regular candidate at its declared
path, but those bytes are uncommitted and must never be read or returned.
For success, the runner independently streams the candidate into an anonymous
private service-owned file, measures its length and SHA-256, and compares both
with the result and request limit. It returns only this accepted byte retention
and source-neutral metadata, never the generator's path.

The candidate-before-result atomic rename order is the trusted producer's
obligation. The runner reads final files only after process completion; it does
not watch writes or claim that a final inventory proves their temporal order.

The supervisor owns the process and staging root through completion. Timeout
and caller cancellation kill and reap the child before root cleanup; a canceled
caller leaves the supervisor responsible for finishing that cleanup. If the OS
cannot confirm child termination/reaping, the runner instead returns a typed
process-cleanup failure with the retained invocation root for service diagnosis;
it does not delete files beneath a possibly live process. Explicit
root cleanup must succeed before successful bytes are returned. Cleanup failure
suppresses success and preserves any preceding failure. Private accepted bytes
remain independently owned after the invocation root has been removed.

This runner does not wire generator work into workers or the public UI, create
cache or readiness rows, upload or publish candidates, install a real generator,
or interpret a target archive. Its executable and protocol tests use synthetic
source-neutral fixtures only.

### Verified Artifact Publication

Publication accepts only the runner's sealed successful output and its matching
prepared processing recipe, under the currently configured supported deployed
entry. It rechecks processing and invocation identity and copies the accepted
bytes into private service-owned retention while independently remeasuring
length and SHA-256. No failed runner outcome provides this handoff.

The output kind is `slicer_project:<dialectIdentity>` and the artifact format is
`project_3mf`. A safe service-owned download filename supplies only the cosmetic
path segment; the public object key is derived as
`artifacts/v2/<artifactSetHash>/<filename>`. The CLI's candidate path is never a
public location. The bounded neutral result metadata is retained with the file
record; the service does not inspect its target archive.

The service persists the exact recipe, ordered occurrences, artifact set, and
primary-file declarations before upload. Upload uses the independently retained
bytes with attachment and immutable-cache headers. Generator readiness always
requires exact content type and length from both storage HEAD and GET, followed
by bounded streaming read-back length and SHA-256 verification. This stronger
generator gate is independent of the ordinary preview/download verification
mode. Private publication retention is explicitly removed before completion.
One short database transaction marks the verified set ready and supersedes the
previous ready sets for the same source, configuration, logical options, and
output kind. Upload and storage calls never hold that database transaction.

Exact retries compare complete immutable set and file declarations and resume
`staged` or `upload_failed` rows without replacing evidence or timestamps.
An existing ready set is reverified and reused without uploading again or
repeating supersession. Different bytes under the same recipe are an integrity
conflict. A changed recipe derives a new artifact set and follows ordinary
supersession only after successful verification. Superseded sets cannot revive.

Upload or verification failure leaves pending work non-ready. A failed pending
attempt cannot downgrade a concurrently completed ready set. Failure while
reverifying an already-ready set instead moves that exact observed revision to
`upload_failed`, making it unavailable through ready lookup while preserving
immutable metadata and file history. An exact retry can restore only the
recorded intended bytes, consistent with the existing artifact repair policy.
Generator transition timestamps advance monotonically to guard this update
against stale verification attempts.

Reconciliation accepts the same configured deployment and prepared recipe,
validates an existing complete immutable record and its stored neutral result,
and verifies storage before completing the same readiness transaction. It
creates no artifact when absent, performs no upload, and never reruns a
generator or Onshape acquisition. Interruption after staging or upload leaves
the set non-ready until verified reconciliation or an exact retry completes.
Missing, corrupt, or unverifiable storage cannot complete pending publication.

These APIs provide the source-neutral publication foundation. Worker/UI
dispatch, automatic scheduling of reconciliation, real deployment, and product
publication remain separate integration work. Tests use the real trusted runner
with synthetic neutral outputs and a loopback object-storage service.

The service-owned [Onshape Selection Plans](onshape-selection-plans.md)
contract in [#173](https://github.com/altendky/onshape-export/issues/173) owns
trusted encoding provenance, ordered selector and exact-leaf resolution,
authoring capture/validation, expected neutral placement derivation, source/path
proof, and complete plan identities. Its planner never encodes or acquires
geometry. [#174](https://github.com/altendky/onshape-export/issues/174) consumes
only successful plans and independently checks the reused immutable version
against every consumed leaf before causal raw geometry acquisition. It does not
resolve selectors, recapture metadata, reinterpret configurations or transforms,
or change order.

Its [configured-leaf acquisition contract](onshape-geometry-acquisition.md)
keeps translation-body request provenance distinct from planned response-derived
configuration identities. #174 owns the trusted sidecar contract and
implementation: the same planning invocation retains the original handoff/context
and complete successful plan, commits their immutable association atomically,
and exposes trusted lookup and complete read-only validation. Caller-attached
pairs and self-computed digests cannot establish that relationship. Sidecar
records remain outside existing source/configuration/leaf/plan identities;
missing, forged, conflicting, or mismatched provenance is operational with zero
creates. A well-formed independently resolved snapshot mismatch is unavailable.
The maintainer approved combined contract/implementation review in #310 instead
of a separate contract merge before implementation. All provenance, bounded
binding, and verification requirements remain in force.

Issue #174 retains opaque bytes and complete ordered
occurrence-to-payload bindings; it allocates no protocol paths and does not
parse internal 3MF grouping, units, geometry, or placement. Unsupported bindings
are unavailable, while authentication, transport, and upstream-contract failures
remain operational failures.

Manifest-order orchestration, deterministic retained paths, manifest/settings
construction, role rewrite, placement summaries, and protocol/contextual
validation are owned by
[#175](https://github.com/altendky/onshape-export/issues/175). That orchestration
preserves planned matrices and configuration identities and does not call
carrier endpoints or derive placement. Generator
raw-input bounds and final target-aware self-validation belong to
[`slicer-project-generators#8`](https://github.com/altendky/slicer-project-generators/issues/8)
and
[`slicer-project-generators#9`](https://github.com/altendky/slicer-project-generators/issues/9),
not to the neutral settings validator.

The [input construction implementation](onshape-generator-inputs.md) consumes
the verified trusted acquisition handoff, declares one deterministic versioned
path per occurrence, and returns a read-only manifest/settings/provenance
bundle. Before later dispatch its request-context check binds the invocation to
the exact manifest and canonical settings. It performs no staging, generator
invocation, or cache operation.

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
inputs. Generator logical `optionsHash` separately binds output format,
requested dialect, ordered capability identities, settings identity, and
settings-schema identity; static deployment and processing identities do not
masquerade as logical options.

Persist the exact canonical recipe JSON and its ordered logical occurrence
records before reuse. Each occurrence retains object/content identity,
SHA-256/length, staged path, transport role, display name, mapping/provenance,
and placement. Equal bytes may share retained content, but occurrence identity,
order, path, and semantic evidence never collapse.

Exact cache reuse requires a known supported recipe, the exact derived linked
artifact-set identity with equal generator and post-process identities, absent
singular acquisition identities, no supersession markers, and an exact complete
primary-file record. Generator staging and lookup accept a prepared recipe and
derive artifact-set, source, configuration, options, post-process, and generator
identities internally; the general free-form artifact staging path rejects
generator-linked rows. Generator-linked artifact sets cannot be restaged under
an existing identity; publication can resume only exactly matching immutable
evidence. Supersession changes selection but preserves recipe, occurrence,
artifact-set, and file history. These persistence and lookup rules remain
separate from mutable approval policy, worker/UI orchestration, and target-aware
validation. Published artifact bytes are immutable in normal operation.

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
