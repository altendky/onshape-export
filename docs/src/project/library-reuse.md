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
