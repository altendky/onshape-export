# Contributing

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this project by you, as defined in the Apache-2.0 license, shall
be dual licensed as MIT OR Apache-2.0, without any additional terms or
conditions.

See [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE) for details.

## Covered-Source Isolation

Do not add implementation primarily informed by GPL- or AGPL-covered source to
this repository, including translated, summarized, or adapted implementation.
Follow [AGENTS.md](AGENTS.md) even when source-informed work is performed with
an automated tool. Put work that requires such source in a separate project
under compatible terms, and do not assume that a process, package, interface,
or repository boundary resolves licensing obligations.

Do not access relevant GPL- or AGPL-covered slicer implementation source for work
in this repository. Target-derived or source-informed slicer implementation,
constants, schemas, fixtures, templates, and summaries belong only in the
canonical
[`slicer-project-generators`](https://github.com/altendky/slicer-project-generators)
project. Before source access or generator capability work, follow its pinned
normative
[Slicer Project Generator Provenance Policy](https://github.com/altendky/slicer-project-generators/blob/7650510c72ef5af05b0d62388020f525cface0d9/docs/src/project/slicer-project-generator-provenance.md)
for source access, provenance, evidence, target implementation, builds, and
generator releases.

This repository retains only source-neutral protocol and service integration
work. Follow the normative
[Slicer Project Generator Integration Policy](docs/src/project/slicer-project-generator-integration.md)
for service approval, sandboxing, independent validation and hashing, cache,
publication, revocation, interface, distribution, and deployment review. A
generator release is not service approval or permission to publish generated
artifacts. Qualified legal review is required for licensing conclusions.
