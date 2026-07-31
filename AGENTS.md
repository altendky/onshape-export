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
