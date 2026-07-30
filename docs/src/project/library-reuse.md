# Library Reuse

## Reference Projects

Useful references:

- `~/repos/onshape-mcp/workers/oauth-proxy`
- `~/repos/onshape-mcp/crates/onshape-client-*`
- `~/repos/onshape3mf`

## Reuse Candidates

From `onshape-mcp`:

- Onshape authorization header construction.
- Basic and OAuth auth data types if they are cleanly separable.
- Request/response sans-IO modeling.
- Configuration layering patterns with `figment`.
- `reqwest` client wrapping and error classification.

From `onshape3mf`:

- Endpoint sequence for configuration discovery and async translation.
- Practical parsing expectations for Onshape configuration responses.

## What Not To Reuse Directly

- MCP protocol logic.
- MCP HTTP OAuth server state and routes.
- CLI-specific OAuth token file behavior.
- Python implementation code from `onshape3mf`.
- Cloudflare OAuth relay routes and Worker-specific deployment patterns.

Target-derived generator reuse and implementation decisions belong in
[`slicer-project-generators`](https://github.com/altendky/slicer-project-generators)
under its pinned
[Slicer Project Generator Provenance Policy](https://github.com/altendky/slicer-project-generators/blob/7650510c72ef5af05b0d62388020f525cface0d9/docs/src/project/slicer-project-generator-provenance.md).
The external process boundary does not itself establish license compatibility.
Service integration follows the local
[Slicer Project Generator Integration Policy](slicer-project-generator-integration.md);
do not duplicate or weaken either policy here.

## Extraction Strategy

Do not force shared libraries before the first vertical slice.

Recommended sequence:

1. Implement the minimum Onshape export client locally.
2. Keep auth, request building, and cache key logic isolated.
3. Compare local interfaces with `onshape-mcp` crates.
4. Extract or depend on shared crates only when the boundary is clear.

Potential shared crate boundaries:

- Onshape API auth and request execution.
- Onshape document URL and id parsing.
- Configuration parameter normalization.
- Translation polling state machine.
- Cache key canonicalization.

## Quantity Canonicalization

Quantity inputs intentionally use a small in-house parser instead of a general unit-expression library.

Maintained Rust unit crates such as `uom` and `measurements` provide useful typed unit conversion, but they do not remove the need for an Onshape-specific input boundary. The application accepts only a plain decimal value plus a selected unit, not arbitrary Onshape/FeatureScript expressions. That narrow contract keeps cache identity deterministic and avoids spending extra Onshape API calls on decode or evaluation.

Canonical numeric values use exact rational arithmetic through `num-rational` and `num-bigint`. Length values are converted exactly to canonical meters for `configHash` identity. The Onshape encoding request projects generated values as parenthesized fractions, such as `(127/5000) m`, so future support for fractional input can preserve the same request projection style.

Angles currently accept `deg` and `rad` but do not canonicalize across units because exact degree/radian conversion would require a symbolic or approximate pi policy. A future angle-normalization policy should bump the configuration canonicalization version.
