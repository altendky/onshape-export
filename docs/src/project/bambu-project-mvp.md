# Bambu Project MVP Integration

> **Status: real deployment blocked on the released generator's complete neutral
> service identity handoff.** The published package is available and its exact
> bytes have been measured. This work does not invent the missing deployment
> fields, approve the release for service use, or enable Bambu output.

The service's neutral foundation is complete. Product orchestration connects
controlled catalog selections to trusted planning, opaque acquisition, manifest
and settings construction, the fixed trusted CLI, and verified publication.
Real advertisement and activation require the exact-package review described in
the [service integration policy](slicer-project-generator-integration.md#service-approval-gates).

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

## Missing Neutral Handoff

The release-only prerequisite is tracked in
[slicer-project-generators#16](https://github.com/altendky/slicer-project-generators/issues/16),
which blocks completion and real activation of
[onshape-export#152](https://github.com/altendky/onshape-export/issues/152).

The adjacent package descriptor contains only binary, build, and provenance-set
identities. The reviewed release identities record adds package identity and
archive/descriptor hashes. Its CLI report records hashes of results and
invocations rather than the complete protocol identity bindings. Neither record
supplies the full dialect, capability, input-kind/input-schema, settings-schema,
normalization, and validation identities required by the existing closed
[deployment document](deployed-generator.md).

A neutral identity-mismatch probe returns structured failure without reporting
those missing identities. The service must obtain a named-review-approved,
hash-bound complete neutral handoff for the existing exact release; it must not
infer values, inspect target implementation source, or change its own schemas to
match guesses. A supplemental reviewed metadata record can bind unchanged
release bytes. The immutable existing package need not change merely to publish
that handoff.

`bambu_deployment::configured_generator` consequently rejects every production
deployment until this prerequisite is resolved. Synthetic test entries exercise
the neutral orchestration only. No real deployment configuration is generated,
and `TRUSTED_GENERATOR_CONFIG_PATH` remains unset in the image and Fly
configuration.

## Installation And Distribution

The Docker generator stage runs `scripts/install-bambu-generator.sh`. It checks
Linux x86_64, downloads the exact package and corresponding-source assets,
verifies both archive digests, and independently verifies the extracted
executable. No generator build or source extraction occurs.

The complete package, including `LICENSE`, `NOTICE`, `LICENSE-neutral-MIT`, and
all third-party notices, remains under
`/opt/onshape-export/generators/bambu-studio-v0.1.0`. The exact distribution
archives are retained in that external component's `distribution/` directory.
The corresponding-source archive remains opaque external distribution bytes;
it is never extracted, inspected, or added to this repository. The published
release provides the paired source acquisition link and its fixed digest.

The installer runs as root, refuses replacement of an existing installation,
and normalizes installed directories to mode 0755, ordinary files to 0644, and
the executable to 0755, all owned by root. The runtime's `onshape-export` user
can read notices and execute the component but cannot replace its bytes. The
future reviewed configuration must likewise reside in a root-owned directory
with no group/world-writable ancestor. The deployment helper checks regular
files, ownership, modes, and ancestors; the existing loader and runner retain
their startup and immediate pre-invocation executable digest checks.

The measured executable requires glibc symbols through GLIBC_2.34 plus
`libgcc_s.so.1`. It was launched as a non-root user with a neutral failing
request in Debian Bookworm, demonstrating loader/runtime compatibility without
claiming target output compatibility. No runtime base change is required.

Before activation, service review must cover the exact neutral handoff,
distribution and notice retention, paired source availability, static
configuration, interface, cache, deployment, and publication behavior. Generator
release approval alone does not satisfy this service review.

## Remaining Service Work

Catalog selection and presentation are service-owned product policy. The full
runtime request, approved deployment, compatibility, and protocol declarations
must remain immutable through queued execution and status lookup.

An interrupted invocation with a prepared recipe but no staged artifact cannot
silently reacquire geometry and rebind that request: new translation evidence
would change the recipe. It remains safely unavailable pending a defined durable
retained-input recovery or explicit new-attempt policy. Completed-upload
reconciliation can verify existing immutable evidence without rerunning Onshape
or the generator. This remaining service policy is separate from the missing
generator metadata handoff; neither gap is hidden by successful synthetic tests.
