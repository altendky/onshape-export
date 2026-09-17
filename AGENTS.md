# GPL And AGPL Source Boundaries

This project is licensed MIT OR Apache-2.0. Treat implementation primarily
informed by GPL- or AGPL-covered source as derivative work. Do not add such
implementation to this repository, whether produced directly or through
translation or summary.

GPL- and AGPL-covered source may still be read for unrelated work. Relevant
upstream slicer implementation source must not be accessed for work in this
repository. Keep target-derived or source-informed slicer implementation,
constants, schemas, fixtures, and summaries out of this repository.

Any interface between this project and such a component must be compatible with
the licenses of both projects.

The canonical target-side project is
`https://github.com/altendky/slicer-project-generators`. Generator-local
source-informed derivative development, target-derived work, builds, and
releases must follow its pinned provenance, build, and release policy:
`https://github.com/altendky/slicer-project-generators/blob/ced6585d5a8e1a47690e7eabdf92beaa7fea7fc4/docs/src/project/slicer-project-generator-provenance.md`.
That generator-local policy does not relax this repository's stricter source
access and ingress prohibitions above.

Keep service-owned neutral protocol, approval, trusted external CLI invocation,
validation, cache, publication, revocation, interface, distribution, and deployment rules in
`docs/src/project/slicer-project-generator-integration.md`.

## Controlled Onshape Fixtures

Before creating an Onshape fixture for this repository, verify the
`onshape-export/Agent Sandbox` folder and create the document directly under
that folder. Do not create or modify fixtures elsewhere, and do not modify an
existing document unless it is explicitly identified as a controlled fixture
for the current work.

Keep fixtures source-neutral and give each document an issue-specific name and
description. Verify the document's parent, ownership, account tier, and access
class after creation. Free-account fixtures may be public when the account tier
requires public documents, but they must contain only synthetic source-neutral
content. Treat fixture documents as persistent reproducibility evidence; do
not delete them without explicit maintainer approval.

Never commit controlled-fixture document, workspace, version, microversion,
element, part, instance, occurrence, property, translation, or external-data
identifiers. Never commit raw Onshape requests or responses, private names or
payloads, credentials, tokens, or private account data. This fixture rule does
not prohibit reviewed public catalog source identifiers required by the catalog
schema. Publish fixture evidence only as sanitized aliases, request shapes,
counts, booleans, classifications, and bounded conclusions. Authentication
failures, including HTTP 401, establish no endpoint or feature availability
result.
