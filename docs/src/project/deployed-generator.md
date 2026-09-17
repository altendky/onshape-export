# Deployed Generator Configuration

> **Status: Normative deployed-generator v1.** This contract configures exactly
> one statically deployed trusted generator. It is not a registry, discovery
> mechanism, approval record, or authenticity claim.

The schema is
[`config/deployed-generator/v1/deployed-generator.schema.json`](../../../config/deployed-generator/v1/deployed-generator.schema.json).
The service loads only the file named by `TRUSTED_GENERATOR_CONFIG_PATH`. It
does not use a default path, search directories, discover executables, replace
values, or acquire packages.

## Closed Document

The configuration is one closed JSON object. Arrays, multiple JSON values,
duplicate members, unknown fields, and incomplete documents are invalid. It has
one operational field:

- `executablePath`: an absolute path to the deployed executable.

The remaining fields are immutable reviewed metadata:

- `packageIdentity` and lowercase 64-hex `packageSha256`.
- `buildIdentity`, `binaryIdentity`, and lowercase 64-hex `binarySha256`.
- `protocolVersion`, fixed to the implemented neutral protocol version `1`.
- `dialectIdentity` and lexicographically sorted unique
  `capabilityIdentities`.
- `inputKindIdentity`, `inputSchemaIdentity`, and `settingsSchemaIdentity`.
- `provenanceSetIdentity`, `normalizationIdentity`, and
  `validationIdentity`.

JSON Schema validation is necessary but not sufficient. Draft 2020-12 can
express capability uniqueness but not the required arbitrary lexicographic
array ordering. Configured processes always run the loader's semantic checks,
including exact protocol version and sorted capability order, after strict JSON
parsing.

Opaque identities contain 1 to 256 visible ASCII characters. The document has
no static `settingsIdentity`; settings identity belongs to one invocation. The
document also has no package path, build digest, second validator, approval
record, signature, or mutable lifecycle state.

## Loading And Executable Checks

An absent environment variable or absent named configuration file is typed
`NotConfigured`. `serve` and `worker` continue to start, but trusted-generator
output is unavailable. Maintenance commands do not require this configuration.

A specified configuration that cannot be read, decoded, or validated is a
startup failure for `serve` and `worker`. Duplicate members have their own
failure category because JSON Schema operates on an already parsed unique-key
object model and cannot detect them reliably.

The configured executable path may be a symbolic link whose resolved target is
a regular file. Startup requires that target to exist, be readable, have at
least one Linux executable mode bit, and hash to `binarySha256`. The immediate
pre-invocation digest check remains owned by the trusted runner. These checks
bind configuration to measured bytes for correctness and configuration
integrity; they do not establish authenticity or a security boundary.

Typed outcomes keep `NotConfigured`, configuration read/decode/duplicate/
validation failures, executable missing/type/readability/mode failures, binary
digest mismatch, and request-level `UnsupportedCombination` distinct.

## Static Identity

The static deployed-generator identity is lowercase SHA-256 over these exact
bytes:

```text
onshape-export-deployed-generator-v1\0 || JCS(immutable fields)
```

The domain separator contains the final NUL byte. The JCS object contains every
document field except `executablePath`. Thus package and binary digest changes
change the static identity, while relocating identical bytes with unchanged
immutable metadata does not.

The identity is a deterministic configuration, processing, cache, artifact,
and provenance binding. It is not an authenticity or security assertion.

At invocation time, the service combines the configured static protocol
bindings with that invocation's `settingsIdentity`. The static deployed-
generator identity and invocation settings identity remain separate inputs to
the later processing recipe.

## Compatibility

There is one configured entry. Compatibility is exact equality for
`protocolVersion`, `dialectIdentity`, the complete canonical capability set,
`inputKindIdentity`, `inputSchemaIdentity`, and `settingsSchemaIdentity`.
Mismatch is `UnsupportedCombination`; there is no ranking, fallback, alternate
entry, or capability substitution.

Real package installation and the real deployed document belong to the product
deployment integration. This repository contract and its tests use only
synthetic source-neutral executable bytes and identities.
