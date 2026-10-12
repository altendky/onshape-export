# Bambu Project MVP Integration

> **Status: the released package and its complete neutral handoff are pinned.**
> The image installs the unchanged external package and selects its generated
> closed deployment document. Catalog entries explicitly opt in with ordered
> selections; models without that configuration expose no project request.

The service's neutral foundation is complete. Product orchestration connects
controlled catalog selections to trusted planning, opaque acquisition, manifest
and settings construction, the fixed trusted CLI, and verified publication.
Exact-package review and black-box verification follow the
[service integration policy](slicer-project-generator-integration.md#service-approval-gates).

## Exact External Component

The candidate release is
[bambu-studio-v0.1.0](https://github.com/altendky/slicer-project-generators/releases/tag/bambu-studio-v0.1.0).
The service acquires the published Linux x86_64 package without rebuilding it.
The frozen distribution bindings are:

| Artifact | SHA-256 |
| --- | --- |
| Linux x86_64 package | `a9c859962d23de9812a28871fa6a80334b7543ab695b52db73ca2d91387c3d9c` |
| Generator executable | `734aa2340c954b7d06f1cea0d6c307b33279ffdd74b10cfe1639cdefc7e5e44c` |
| Complete corresponding-source archive | `bcc3c15220f739b9df9c6b5a6b2422c05ac4c126a37e2e8c5a8640b3184535ae` |
| Adjacent package descriptor | `a93a499aa55a5096b2c0c264dd7db537e305148f664a9ed2253062cd7358d8c1` |

The package identity is
`0a65ae55eb5042b9f3b3987d2b4b81afdbcf05b9c94c2d44c30392b30651e2cd`;
the build identity is
`79ba9c5c4abfc8e20a61e266a9df6ef44116f10d34c625c2e805b1b72af8e34f`;
the aggregate provenance-set identity is `BBL-MVP-PROVENANCE-SET-v1`.
These are opaque release bindings, not implementation facts or a grant of
service deployment approval.

## Reviewed Neutral Handoff

The release-only prerequisite
[slicer-project-generators#16](https://github.com/altendky/slicer-project-generators/issues/16)
was completed by its merged supplemental metadata publication. The service pins
commit `7acb58a4d631c1afdd3d7ffc450d431f184c0160` and the external
[complete neutral handoff](https://github.com/altendky/slicer-project-generators/blob/7acb58a4d631c1afdd3d7ffc450d431f184c0160/handoffs/bambu-studio/v0.1.0-v1/handoff.json)
SHA-256 `8547eae6e1d9378040922972ab9c9bc4561d54afb9c0ba9e3e7305e320eeb40f`.
Its detached named review binds unchanged package, source, descriptor and binary
digests. The complete static deployed-generator identity is
`fd088d4e2066db7de9a26436ad081f045f1fac472429f249d58f623b3c6f8beb`.

The adjacent package descriptor contains only binary, build, and provenance-set
identities. The supplement now supplies all fourteen immutable fields required
by the existing closed [deployment document](deployed-generator.md), without
changing those original bytes or the service's identity scopes. The installer
adds only the service's absolute executable path. The startup loader and product
admission check every binding through the complete static identity.

Local protocol, deployment, and settings-v2 schemas match the handoff's exact
neutral contract pins. The service treats identities as opaque values, retains
the AGPL-authored packet externally, and imports no upstream implementation,
target schemas, or target fixtures. Generator-local release review remains
separate from the service's installation, interface, cache and publication checks.

The reviewed transport declarations require `model/3mf` geometry, at most
16 MiB per staged file and 64 MiB across all staged occurrences, including
repeated equal bytes. Generated project output is bounded by 32 MiB. These
limits enter the frozen product policy and apply to fresh and recovered recipes.
The service does not inspect geometry or generated target archives.

## Installation And Distribution

The Docker generator stage runs `scripts/install-bambu-generator.sh`. It checks
Linux x86_64, downloads the exact package and corresponding-source assets,
verifies both archive digests, and independently verifies the extracted
executable and adjacent descriptor. It separately verifies the pinned handoff,
named review, and release metadata. No generator build or source extraction occurs.

The complete package, including `LICENSE`, `NOTICE`, `LICENSE-neutral-MIT`, and
all third-party notices, remains under
`/opt/onshape-export/generators/bambu-studio-v0.1.0`. The exact distribution
archives are retained in that external component's `distribution/` directory.
The corresponding-source archive remains opaque external distribution bytes;
it is never extracted, inspected, or added to this repository. The published
release provides the paired source acquisition link and its fixed digest. The
supplemental metadata, its licensing information, and original corresponding
source remain external distribution material.

The installer runs as root, refuses replacement of an existing installation,
and normalizes installed directories to mode 0755, ordinary files to 0644, and
the executable to 0755, all owned by root. The runtime's `onshape-export` user
can read notices and execute the component but cannot replace its bytes. The
generated configuration must likewise reside in a root-owned directory
with no group/world-writable ancestor. The deployment helper checks regular
files, ownership, modes, and ancestors; the existing loader and runner retain
their startup and immediate pre-invocation executable digest checks.
`TRUSTED_GENERATOR_CONFIG_PATH` in Docker and Fly names
`/opt/onshape-export/generators/bambu-studio-v0.1.0/deployed-generator.json`.

The measured executable requires glibc symbols through GLIBC_2.34 plus
`libgcc_s.so.1`. It was launched as a non-root user with a neutral failing
request in Debian Bookworm, demonstrating loader/runtime compatibility without
claiming target output compatibility. No runtime base change is required.

Before activation, service review must cover the exact neutral handoff,
distribution and notice retention, paired source availability, static
configuration, interface, cache, deployment, and publication behavior. Generator
release approval alone does not satisfy this service review.

## Durable Product Recovery

Catalog selection and presentation are service-owned product policy. The full
runtime request, approved deployment, compatibility, and protocol declarations
must remain immutable through queued execution and status lookup.

A fresh request remains unbound until the trusted runner succeeds. The service
independently copies and remeasures its accepted output, writes a closed neutral
receipt, syncs both files and the directory ancestry, and atomically installs a
private immutable checkpoint. Only then does SQLite bind the exact processing
recipe and checkpoint identity together. A generator failure before acceptance
leaves the request unbound, allowing a retry with fresh causal acquisition.

Restart recovery first attempts verified storage reconciliation. If publication
was interrupted before staging or upload, or storage no longer contains the
intended bytes, the service reopens the bound checkpoint. It checks the receipt's
canonical hash, successful invocation, current prepared recipe, result bindings,
and exact output length and SHA-256 before restoring the private accepted-byte
handoff. Existing immutable publication and supersession guards still apply.
Recovery never reacquires geometry or silently changes the associated recipe.
Corrupt checkpoint evidence and inconsistent associations remain unavailable.

Production requires persistent SQLite storage. By default, checkpoints live in
`.bambu-generator-outputs/<database-path-hash>` beside the canonical database
file. `GENERATOR_OUTPUT_RETENTION_DIR` can choose an absolute external base
directory; it still receives the database-specific namespace. Retention paths
must remain outside Git checkouts, including through symbolic-link ancestry.
Checkpoint directories are mode 0700 and files are mode 0600. Paths are
operational configuration and do not enter processing identity or public status.
The database path and retention storage must survive application restarts.

Ready checkpoints remain available for exact repair. Resetting generated SQL
state removes associations without deleting checkpoint bytes. Automatic
checkpoint pruning is not implemented; operator removal must preserve snapshots
referenced by retained requests. Missing retention cannot authorize replacement
bytes under an existing recipe. These service rules are also recorded in the
[canonical integration policy](slicer-project-generator-integration.md#durable-accepted-output-checkpoints).

## Real Package Verification

The `generator-verification` Docker target installs exactly the published
component and executes the service's product tests as the non-root runtime user.
It exercises configured Part Studio and rigid Assembly paths, named objects,
support blockers, repeated occurrences with equal standard Geometry 3MF bytes,
verified publication, ready-cache reuse, and an unsupported nonrigid Assembly.
Synthetic loopback APIs provide source-neutral source and storage responses;
the installed generator is real and owns its final target self-validation.

Input geometry is independently authored from the 3MF Consortium Core standard.
Generated archives remain opaque; only the neutral result, measured byte length,
SHA-256, storage read-back and readiness are checked. No live Onshape fixture,
source-derived slicer fixture, or service-side target validator is involved.
The CI aggregate requires this real-package check as well as ordinary Rust,
documentation, hook and security checks.

## Service Technical Review

On 2026-10-12, independent agent `Codex /root/readiness_contract` reviewed the
exact package, full handoff and static identity bindings above, external notice
and corresponding-source retention, protected installation paths, neutral
interface limits, frozen request policy, and recovery/publication guards. It
found no remaining technical blocker after the transport, request-field, and
supplemental-notice integrity findings were corrected.

The final non-root installed-package verification passed all three product
cases: configured Part Studio, rigid Assembly, and unsupported nonrigid Assembly.
Successful cases reached generator-owned final validation, verified storage
read-back and ready-cache reuse. The ordinary Rust suite passed 417 tests, with
the three real-package tests run separately by the mandatory Docker check.

These measurements and the named technical review accept the exact static
binding for the maintainer-authorized service integration workflow, contingent
on current-head CI. They do not claim a human review, a legal compatibility
opinion, live Onshape characterization, or independent target validation. Any
package, handoff, static binding or interface change requires renewed review.
