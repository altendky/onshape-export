# Slicer Adapter Provenance And Licensing Policy

> **Status: Normative policy.** This policy governs source access, isolated adapter development, and release eligibility, even though the adapter architecture itself is only proposed.

This policy records feature-level evidence needed to review origin, licensing, implementation boundaries, compatibility claims, and release eligibility.
It does not provide legal advice.
Architecture, process, package, or repository separation does not itself settle whether work is derivative or whether licenses are compatible.
Qualified legal review is required for licensing conclusions.

## Scope And Required Granularity

Each planned adapter capability must have a feature-level provenance record, including independently derived or clean-room work.
Before upstream implementation source is accessed or source-informed work begins, establish a source-access record identifying who may access the source, the intended official upstream material, the isolated adapter development location, and the classification process.
Before independent or clean-room work begins, establish the corresponding evidence and separation process described below.
The initial classification and evidence may be explicitly provisional while restricted development generates implementation evidence.

Provisional work must remain in isolated adapter development and must not enter this repository, be advertised as a supported capability, or produce published, released, or production-deployed artifacts.
Package-level statements such as "based on upstream" are not sufficient.
A capability spanning distinct algorithms, constants, and schema facts needs records granular enough to classify and review each source relationship.

Records are append-only review evidence.
Corrections create a new immutable record or provenance-set version and retain the superseded record.

## Universal Release Gates

Every releasable implementation and advertised capability must have:

- A resolved feature-level classification for every source or evidence relationship.
- The classification-specific evidence required by this policy.
- Immutable local implementation paths and commit, modification records when applicable, and authorship and review records.
- Fixture identifiers and hashes, test identifiers and results, and exact slicer versions used for compatibility validation.
- Completed review of licenses and notices applicable to the evidence inputs, implementation, dependencies, fixtures, package, and planned distribution.
- An immutable provenance-set version linked to every advertised capability identifier and revision.
- The exact immutable candidate package identity, metadata, and validation results approved for release.

Unknown, disputed, provisional, or incomplete classification or evidence is non-releasable.
It blocks packaging for distribution, capability advertisement, fixture or artifact publication, release, and production deployment.
Do not invent a source, commit, release, specification, copyright holder, license, permalink, experiment, fixture, attestation, or review result to complete a gate.

## Classification-Specific Evidence

Each relationship must use one of these classifications:

- `direct_source_reuse`: copied source or source retained with only mechanical changes.
- `adapted_algorithm`: implementation derived from an upstream algorithm, control flow, or source-level design.
- `adapted_constant`: constants, tables, identifiers, templates, or serialized values taken or transformed from upstream source.
- `schema_fact`: format or schema facts, with `source_informed_schema_fact` evidence when learned from inspected upstream implementation or `independently_established_schema_fact` evidence when established from official public specifications, official documentation, or documented black-box observations and fixtures.
- `independently_derived_behavior`: behavior derived without using covered implementation source, supported by independent specifications, experiments, or interoperability observations.
- `clean_room_implementation`: optional classification requiring documented separation of observers and implementers, a source-neutral behavioral specification, access records, and reviewable evidence that the implementer did not use covered source.

Classification is evidence, not a legal conclusion.
Calling work clean-room, independent, or schema-only does not make it so without reviewable records.
The classification-to-evidence-kind mapping is exact: `direct_source_reuse`, `adapted_algorithm`, and `adapted_constant` use `source_informed`; `schema_fact` uses either `source_informed_schema_fact` or `independently_established_schema_fact`; `independently_derived_behavior` uses `independently_derived_behavior`; and `clean_room_implementation` uses `clean_room`.

### Source-Informed Evidence

`direct_source_reuse`, `adapted_algorithm`, `adapted_constant`, and a `schema_fact` with `source_informed_schema_fact` evidence require immutable official upstream implementation-source references.
Each reference must record:

- Official repository owner, repository name, and canonical repository URL.
- Upstream release or tag, or an explicit statement that none exists.
- Full commit hash, never a branch name or abbreviated hash.
- Repository-relative path.
- Symbol when one exists and an exact line range identifying the material.
- Full GitHub blob permalink pinned to the full commit and line range when the official repository is on GitHub; use the equivalent immutable official URL for another forge.
- Cryptographic content hash of the exact referenced file or extracted material, with the hash algorithm stated.
- The exact license text and notices applying at that commit and path, including repository-, directory-, and file-specific terms.
- Reviewer identity, review date, outcome, and required attribution, modification, source-offer, or redistribution actions.

Restricted development may carry an explicit unresolved source reference or license field while evidence is collected, but the capability is not releasable.
If an official immutable reference or exact applicable license review cannot ultimately be completed, publication, release, production deployment, and capability advertisement are blocked.

### Independently Derived Evidence

`independently_derived_behavior` and a `schema_fact` with `independently_established_schema_fact` evidence do not require a fictitious implementation-source reference.
An independently established schema fact must be based on official public specifications, official documentation, or documented black-box observations and fixtures, not inspected upstream implementation source.
It requires:

- Immutable authoritative specification or documentation references where available, including version, stable URL or identifier, relevant section, content hash when obtainable, and applicable license or terms.
- A documented statement when no authoritative immutable reference is available, with review of the alternative evidence basis.
- Immutable reproducible experiment records containing plans, inputs, observed outputs, results, dates, and environment or tool versions.
- Fixture identifiers and hashes linked to the experiments and capability tests.
- Authorship and independent review evidence for the behavior, experiments, and implementation.
- A source-access declaration identifying whether each author or reviewer accessed relevant upstream implementation source and the controls used to preserve the classification.
- Review confirming the evidence basis and the applicable `independently_derived_behavior` or `independently_established_schema_fact` evidence kind.

If relevant implementation source actually informed a relationship, classify and review that relationship as source-informed rather than omitting its source.

### Clean-Room Evidence

`clean_room_implementation` does not require implementers to consult covered implementation source.
It requires:

- Immutable requirements, behavioral specifications, and other evidence inputs supplied to the implementation team, with versions, hashes, authors, and review history.
- A documented separation procedure identifying specification-team and implementation-team roles, communication boundaries, access controls, and retained records.
- Source-access declarations and attestations from participants, including implementer attestations that they did not access prohibited covered implementation source.
- Reproducible experiment records, fixture identifiers and hashes, test results, and exact slicer versions.
- Independent review of the requirements, separation procedure, attestations, experiments, fixtures, and implementation.
- Complete source-informed implementation evidence and applicable license/notice review for any covered or other upstream implementation source consulted by the specification team, retained without exposing prohibited source to implementers.
- A complete provenance chain in which official specifications or documentation, standards, black-box experiments, generated fixtures, and other inputs are each recorded under their applicable evidence classification rather than all being classified as implementation source.

The specification team's source-side provenance does not imply that clean-room implementers consulted that source.

## License And Notice Review

The review must address the actual classification and planned distribution.
A license name from repository metadata is not enough.
During restricted development, unresolved review fields must be marked provisional and the work must remain isolated and non-distributable.
Ambiguous scope, exceptions, dual licensing, generated files, specifications, dependencies, fixtures, and conflicting notices block releasable implementation, capability advertisement, publication, release, and production deployment until resolved by qualified review.
Preserve required notices in the adapter project and distribution as applicable.

## Local Implementation Evidence

Every record must identify:

- Adapter repository and immutable local commit once available.
- Local paths, symbols, and line ranges implementing the capability.
- Summary of modifications relative to any reused or adapted material.
- Author and reviewer identities and review dates.
- Fixtures and tests that exercise the capability.
- Slicer products and exact versions used for compatibility validation.
- Related notices and where they are distributed.

Before a local commit exists, the initial record may reserve planned paths and planned verification.
Restricted development builds and tests may generate the implementation evidence needed to complete the record.
The record remains incomplete until the immutable local references, modifications, fixtures, tests, and slicer-version results are filled in.
It cannot satisfy capability advertisement, publication, release, or production-deployment gates.

## Capability Linkage

Every advertised capability identifier and revision must link to one immutable provenance-set version.
That set must cover all code, constants, schemas, fixtures, and generated templates needed by the capability.
Adapter capability output and artifact metadata must report the same provenance-set version so a published project 3MF can be traced to the reviewed evidence.

Shared Bambu/Orca/Prusa behavior still requires per-capability linkage.
Sharing code or format ancestry is not evidence that licenses, notices, or compatibility claims are interchangeable.

## CI And Release Gates

CI may compile and test explicitly isolated, non-distributable development work with provisional records to generate implementation evidence.
CI and release automation must prohibit packaging for distribution, capability advertising, fixture or artifact publication, release, and production deployment when any exercised capability has:

- No provenance record or no capability-to-record linkage.
- An unknown, disputed, provisional, or incomplete classification.
- A classification whose evidence kind does not match the exact mapping above, including a `schema_fact` without resolved `source_informed_schema_fact` or `independently_established_schema_fact` evidence.
- Missing or unverifiable evidence required for its classification.
- Missing content hashes where required, local paths, modifications when applicable, fixtures, tests, authorship/review records, or slicer versions.
- An incomplete applicable license or required-notice review.
- An unresolved classification or legal-review status.
- Provenance metadata that disagrees with the adapter build or package metadata.

The restricted workflow must mark outputs non-distributable and must not advertise capabilities or publish fixtures, packages, or project artifacts.
It must run only in the isolated adapter development project established by the source-access record.
Waivers must not bypass legal or notice requirements.
Any procedural exception must be narrow, approved, dated, and retained in the provenance set.

## Example Record Shape

This example is illustrative, not a settled storage schema.
Placeholder text may be used only in explicitly provisional records for restricted, isolated development.
Every placeholder must be replaced by verified immutable evidence before capability advertisement, publication, release, or production deployment.
Each record contains exactly one evidence object with the exact evidence kind required by its classification; independently established schema-fact, independently derived behavior, and clean-room records must not fabricate `officialSources` entries.

```json
{
  "recordVersion": 1,
  "capability": "dialect.feature",
  "capabilityRevision": 1,
  "classification": "one required classification",
  "evidence": "exactly one classification-dependent object",
  "local": {
    "repository": "adapter repository",
    "commit": "full local commit hash",
    "paths": ["local/path and symbol or lines"],
    "modifications": "concise description"
  },
  "verification": {
    "fixtures": ["fixture identifiers and hashes"],
    "tests": ["test identifiers"],
    "slicerVersions": ["exact validated versions"]
  },
  "releaseCandidate": {
    "packageIdentity": "immutable package hash",
    "metadata": "validated protocol, dialect, provenance, and capabilities",
    "validationResults": ["immutable result records"]
  },
  "provenanceSetVersion": "immutable set identifier"
}
```

`direct_source_reuse`, `adapted_algorithm`, and `adapted_constant` use this evidence shape:

```json
{
  "kind": "source_informed",
  "sourceAccessRecord": "immutable access and process record",
  "officialSources": ["immutable official implementation-source references and hashes"],
  "exactLicenseAndNoticeReview": "qualified review record"
}
```

`schema_fact` learned from inspected upstream implementation uses this distinct evidence shape:

```json
{
  "kind": "source_informed_schema_fact",
  "sourceAccessRecord": "immutable access and process record",
  "officialSources": ["immutable official implementation-source references and hashes"],
  "exactLicenseAndNoticeReview": "qualified review record"
}
```

`independently_derived_behavior` uses this evidence shape:

```json
{
  "kind": "independently_derived_behavior",
  "authoritativeReferences": ["immutable references where available"],
  "experimentsAndFixtureHashes": ["immutable evidence records"],
  "authorshipAndReview": "review record",
  "sourceAccessDeclaration": "participant declaration"
}
```

`schema_fact` established independently of upstream implementation source uses this distinct evidence shape:

```json
{
  "kind": "independently_established_schema_fact",
  "authoritativeReferences": ["immutable specification or documentation references where available"],
  "experimentsAndFixtureHashes": ["immutable evidence records"],
  "authorshipAndReview": "review record",
  "sourceAccessDeclaration": "participant declaration"
}
```

`clean_room_implementation` uses this evidence shape:

```json
{
  "kind": "clean_room",
  "requirementsAndEvidenceInputs": ["immutable references and hashes"],
  "separationProcedureAndAttestations": "reviewed records",
  "experimentsAndFixtureHashes": ["immutable evidence records"],
  "specificationTeamInputProvenance": ["applicable classification records for every input"],
  "coveredImplementationSourceEvidence": ["applicable source-informed records"],
  "review": "independent review record"
}
```

## Repository Boundary

The repository-root `AGENTS.md` source-boundary rules apply.
Work primarily informed by GPL- or AGPL-covered implementation source must not be added to this MIT OR Apache-2.0 repository.
Such implementation belongs in a separate appropriately licensed project, subject to this same evidence policy and qualified review of the interface and distribution.
See [Library Reuse](library-reuse.md) for a policy pointer, not an alternative rule.
