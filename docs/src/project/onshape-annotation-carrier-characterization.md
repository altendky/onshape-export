# Onshape Annotation Carrier And Selector Characterization

> **Status: source-neutral characterization, not an annotation convention.**
> This report records sanitized authenticated observations made on 2026-08-02.
> It does not define carrier precedence, name markup, semantic roles, a settings
> schema, or a production selector implementation.

## Scope And Boundary

This study characterizes built-in metadata and names available to an Onshape
Free account and the source-scoped selectors needed to attach later annotations
to Part Studio parts and Assembly occurrences. It covers configured Part Studio
and Assembly contexts, duplicate names, repeated references, nested occurrence
paths, and suppressed instances.

The work is source-neutral. No slicer implementation source, target-derived
schema, target fixture, generated slicer project, or target compatibility fact
was inspected. The repository-root `AGENTS.md` boundary and the
[Slicer Project Generator Integration Policy](slicer-project-generator-integration.md#repository-ingress-boundary)
govern this work.

This report uses three local aliases:

- `DOC-CARRIER`: one controlled synthetic document.
- `PS-CARRIER`: one Part Studio with two solid parts and two configuration
  options.
- `ASM-CARRIER`: one root Assembly with a nested Assembly and two configuration
  options.

No document, workspace, version, microversion, element, part, instance,
occurrence, property, or user identifier is included. No raw request, response,
header, credential, name test value, or private account data is committed.

## Authentication And Access Evidence

The observations used an authenticated OAuth session with read/write scopes.
The authenticated session reported the `Free` plan group and no company plan.
The synthetic document was user-owned, not enterprise-owned, and had no
explicit ACL entry or support sharing. Its access class was public, as required
for Free-plan documents by Onshape's published plan terms. The designated
sandbox folder was verified before creation. The create response and a
post-creation document read both reported that exact folder as the parent.

Authentication validation succeeded before evidence collection. No HTTP 401
response was used as evidence about endpoint, selector, or carrier availability.

## Method And Evidence Boundary

The controlled workspace was mutated only to create source-neutral geometry,
configuration inputs, Assembly relationships, and built-in metadata values.
Every successful metadata mutation used a read-after-write check. After the
fixture was complete, a document version was created and resolved to its
immutable document microversion. Final selector and part-carrier reads used
that microversion. The Assembly metadata-debug probes used the corresponding
workspace state because those calls returned no metadata body.

Private evidence retained exact request and response data outside the
repository. Checked-in evidence is limited to endpoint and request shapes,
counts, booleans, classifications, and bounded conclusions.

| Purpose | Sanitized API shape | Context |
| --- | --- | --- |
| Verify authenticated plan | `GET /api/users/sessioninfo` | OAuth session |
| Verify sandbox ACL | `GET /api/folders/{fid}/acl` | Before fixture creation |
| Create controlled document | `POST /api/documents` with `name`, `description`, `parentId`, and `isPublic` | Workspace creation |
| Verify parent, ownership, and access | `GET /api/documents/{did}` and `GET /api/documents/{did}/acl` | After fixture creation |
| Resolve immutable context | `GET /api/documents/d/{did}/{wv}/{wvid}/currentmicroversion` | Workspace and version |
| Create immutable evidence version | `POST /api/documents/d/{did}/versions` with workspace, microversion, name, and description | Workspace |
| Read Part Studio configuration | `GET /api/elements/d/{did}/{wvm}/{wvmid}/e/{eid}/configuration` | Workspace and microversion |
| Read configured parts | `GET /api/parts/d/{did}/{wvm}/{wvmid}/e/{eid}?configuration=...` | Workspace and microversion |
| Read one part's metadata | `GET /api/metadata/d/{did}/{wvm}/{wvmid}/e/{eid}/p/{pid}?configuration=...` | Workspace and microversion |
| Update built-in part metadata | `POST /api/metadata/d/{did}/w/{wid}/e/{eid}/p/{pid}?configuration=...` with `properties[]` containing `propertyId` and `value` | Workspace only |
| Read Assembly configuration | `GET /api/elements/d/{did}/{wvm}/{wvmid}/e/{eid}/configuration` | Workspace and microversion |
| Read instances and occurrences | `GET /api/assemblies/d/{did}/{wvm}/{wvmid}/e/{eid}?configuration=...&excludeSuppressed=false` | Workspace and microversion |
| Configure instance suppression | `POST /api/assemblies/d/{did}/w/{wid}/e/{eid}/modify` with `suppressionStates` keyed by instance ID | Workspace only |
| Probe full Assembly metadata | `GET /api/metadata/d/{did}/{wvm}/{wvmid}/e/{eid}/assembly-debug?configuration=...` | Workspace |
| Read Assembly-element metadata | `GET /api/metadata/d/{did}/{wvm}/{wvmid}/e/{eid}` | Workspace |

Both `assembly-debug` invocations completed without an API-call error and the
client returned an empty response body. The client did not expose an HTTP status
for those empty results, so they are classified as empty client results, not as
proof of a successful metadata representation or endpoint unavailability. They
contribute no positive evidence of an occurrence metadata bag.

## Controlled Source Shape

`PS-CARRIER` contained exactly two solid parts. The parts had distinct part IDs
and deliberately equal display names. Its enum configuration had `Default` and
`Alternate` options. One built-in Description value differed by configuration.

`ASM-CARRIER` contained:

- Four root instances: two repeated references to the same part and
  configuration, one statically suppressed part, and one nested Assembly.
- Two instances inside the nested Assembly.
- Six occurrence records: four one-segment paths and two two-segment paths.
- Duplicate generated instance display strings across root and nested scopes.
- One root instance whose suppression changed from `false` to `true` between
  the Assembly's `Default` and `Alternate` configurations.
- One separate root instance suppressed in both configurations.

With `excludeSuppressed=false`, all six occurrence paths were returned in both
observed configurations, including paths for suppressed instances. This proves
only the observed inclusion behavior; it does not claim that the same path has
cross-configuration identity.

## Selector Characterization

### Immutable Source And Configuration Context

Resolving the created version returned the same document microversion used for
the final reads. Both configuration definitions and their option identifiers
were read at that immutable context. A selector must therefore include at
least the document ID, document microversion, element ID, element kind, and
explicit configuration identity before adding a source-object selector.

### Part Studio Parts

`GET /parts/.../e/{eid}` returned exactly two distinct `partId` values for the
two solids in both observed configurations. It also returned a distinct opaque
configuration identity for each configuration. The observed cosmetic
configuration did not change either `partId`; that observation is not a claim
of equivalence across configurations, geometry changes, or microversions.

A `PS-CARRIER` source-object selector is therefore the observed `partId` scoped
to the exact document, Part Studio element, microversion, and configuration.
Names, ordinals, body queries, metadata microversions, and configuration
encodings are not substitutes for that selector.

### Assembly Occurrences

The Assembly definition returned each occurrence as an ordered `path` array of
instance IDs. The two nested leaf occurrences had complete two-segment paths;
the root instances had one-segment paths. Repeated references had different
complete paths even when they referenced the same part and configuration.

An `ASM-CARRIER` source-object selector is therefore the complete ordered path
scoped to the exact document, root Assembly element, microversion, and
configuration. A tail instance ID, display name, referenced part ID, array
position, or flattened path is insufficient.

## Built-In Metadata Inventory

### Part Scope

The part metadata endpoint returned 21 built-in property descriptors. All were
single-valued according to their descriptor. They covered object, string,
category, enum, Boolean, and computed value types.

| Built-in property | Value type | Workspace advertised editable | Empirical write result |
| --- | --- | --- | --- |
| Appearance | Object | Yes | Not exercised |
| Name | String | Yes | Succeeded; bounds and value behavior tested |
| Description | String | Yes | Succeeded in unconfigured and configured forms |
| Category | Category | No | Not exercised |
| Part number | String | Yes | Not exercised |
| Revision | String | Yes | Not exercised |
| State | Enum | No | Not exercised |
| Vendor | String | Yes | Not exercised |
| Project | String | Yes | Not exercised |
| Product line | String | Yes | Not exercised |
| Material | Object | Yes | Not exercised |
| Title 1 | String | Yes | Not exercised |
| Title 2 | String | Yes | Not exercised |
| Title 3 | String | Yes | Not exercised |
| Not revision managed | Boolean | Yes | Not exercised |
| Exclude from all BOMs | Boolean | Yes | Succeeded |
| Unit of measure | Enum | Yes | Succeeded |
| Mass | Computed | Yes | Not exercised |
| Center of mass | Computed | Yes | Not exercised |
| Inertia | Computed | Yes | Not exercised |
| Tessellation quality | Enum | Yes | Not exercised |

In the workspace, 19 descriptors advertised `editable=true`; Category and
State advertised `editable=false`. Advertised editability is not treated as
proof that every value type can be written. The study successfully wrote and
read back four representative built-ins without defining custom properties:

- Name: required string.
- Description: optional multiline string.
- Exclude from all BOMs: Boolean.
- Unit of measure: enum.

The successful writes changed the property source classification from an
automatic/default source to an unconfigured or configured source as applicable.
A Description written only for `Alternate` read differently from `Default`,
proving configuration-scoped built-in metadata for that property and source.

At the immutable microversion, all 21 descriptors were readable and advertised
`editable=false`. Mutations must target a workspace; a version or microversion
is evidence context, not a writable authoring context.

### Assembly Element Scope

The root Assembly element metadata endpoint returned 19 built-in descriptors,
all marked single-valued: Name, Description, Category, Part number, Revision,
State, Vendor, Project, Product line, Title 1, Title 2, Title 3, Not revision
managed, Exclude from all BOMs, Unit of measure, Mass, Center of mass, Inertia,
and Subassembly BOM behavior. Category and State advertised non-editable in the
workspace; the other 17 advertised editable. None was empirically written in
this study. This collection is scoped to the Assembly element. It is not
instance metadata and cannot distinguish two occurrences of the same referenced
object.

### Instance Scope

Observed Assembly instance records exposed readable fields for instance ID,
type, generated name, suppression state, referenced document ID, element ID,
document microversion, short and full configuration, and, for part instances,
part ID and standard-content status. The Assembly modification API could write
plain or configuration-controlled suppression state by instance ID.

The public instance response schema also allows document version, feature ID,
part number, revision, and status fields. None was present with a value in the
observed controlled responses. Their readability and usefulness as carriers
remain unproven.

No public metadata route addressed an Assembly instance. `createInstance` did
not accept a name, and `modify` exposed deletion, suppression, and transforms
but no name or arbitrary metadata update. Instance state is useful source
structure, but no general instance annotation bag was proven.

### Occurrence Scope And Bounded Negative Result

Occurrence records exposed only a complete ordered path plus transform, hidden,
fixed, and a null mate-status value in the observed response. They did not
contain a name, properties collection, or metadata reference.

The official Metadata API surface exposed document, element, part, and standard
content metadata routes, but no route addressed an occurrence path. Authenticated
`assembly-debug` probes for both Assembly configurations returned no metadata
body and therefore supplied no occurrence-addressable properties. The broader
Assembly-element metadata response was element-scoped, while referenced-part
metadata was part/configuration-scoped.

No readable and writable occurrence-level metadata bag was proven. This is a
bounded negative result from immutable occurrence reads, the public metadata
route inventory, and empty workspace metadata-debug client results for the
observed Free account, API surface, source state, and configurations. It is not
a claim about internal Onshape storage, the debug endpoint's HTTP availability,
or future APIs. Distinct annotations on repeated occurrences must not be claimed
from the observed carriers unless later work proves an occurrence-scoped
carrier.

## Name Characterization

### Part Names

Part Name was readable and writable through built-in part metadata. The
descriptor required at least one character and advertised a maximum length of
256. A 256-character value was accepted and read back; a 257-character value
and an empty value were rejected with property-validation status. Failed writes
did not replace the previous value.

A synthetic name containing spaces, delimiter-like punctuation, quotes, a
backslash, and a decomposed Unicode accent was accepted and returned unchanged
in the observed JSON value. No escaping or Unicode normalization was observed
for that case. This does not establish behavior for control characters, every
Unicode sequence, or future clients. Two different parts accepted exactly equal
names, proving that part names are not unique selectors.

### Instance Names

Instance names were readable in Assembly definitions. Onshape generated
numbered suffixes from the duplicate source part names, disambiguating siblings
in the root Assembly. Equal generated strings still appeared in different root
and nested scopes, so a name remained non-unique across complete occurrence
paths.

No public instance-name write operation was found or proven. Instance-name
length, escaping, and normalization limits are therefore unproven. A later
convention must not require instance-name authoring unless a supported write
path is characterized.

## Carrier And Selector Matrix

| Candidate | Observed scope | Read | Write | Cardinality | Configuration behavior | Bounded conclusion |
| --- | --- | --- | --- | --- | --- | --- |
| Part `partId` | Document + Part Studio element/kind + microversion + configuration | Yes | Not applicable | One per returned part | Two explicit configurations observed; values happened to match | Usable source selector only with full source/configuration scope. |
| Complete occurrence path | Document + root Assembly element/kind + microversion + configuration | Yes | Not applicable | One ordered path per occurrence | Suppressed and nested paths returned in both observed configurations | Usable source selector only as the complete ordered path with full source/configuration scope. |
| Built-in part properties | Referenced part + configuration | Yes | Four workspace fields proven; others unproven | 21 single-valued descriptors observed | One Description differed by configuration | Proven fields are candidate carriers; untested fields are not implicitly allowed. |
| Assembly element properties | Whole Assembly element | Yes | Endpoint available; not exercised here | 19 descriptors observed | Configuration query available | Too broad to distinguish instances or occurrences. |
| Instance definition fields | Assembly instance | Yes | Suppression only among relevant observed fields | One record per instance definition | Configured suppression proven | Structural source data, not a general annotation bag. |
| Occurrence properties | Complete occurrence | Path/state only | No metadata write path proven | Six occurrence records, no properties collection | Both configurations probed | No occurrence-level annotation bag proven. |
| Part Name | Referenced part + configuration | Yes | Yes in workspace | One; duplicates allowed | Configuration-capable metadata route | Potential fallback carrier only; never identity. |
| Instance Name | Assembly instance definition | Yes | No public write path proven | One generated value; duplicates across scopes | Read in both configurations | Not a proven authoring carrier. |

## Fail-Closed Requirements

1. Resolve the document ID, immutable document microversion, element ID and
   kind, and explicit configuration before interpreting a part ID or occurrence
   path.
2. Keep Part Studio `partId` and Assembly complete ordered occurrence path as
   distinct selector types.
3. Never identify, order, deduplicate, or associate source objects by metadata,
   name, ordinal, array position, referenced part, or content equality.
4. Do not attach distinct annotations to repeated occurrences using part- or
   element-scoped metadata.
5. Treat metadata editability as context-dependent. Workspace write results do
   not make immutable version/microversion responses writable.
6. Record missing, ambiguous, duplicate, unsupported, and unproven carriers
   explicitly. Do not silently fall back to a broader scope.
7. Treat authentication failure only as authentication failure.

## Decision And Limitations

The observed Free account provides two sufficient source-scoped selector forms:
Part Studio `partId` and complete ordered Assembly occurrence path, each bound
to an immutable document and element plus explicit configuration. Four specific
built-in part properties are proven candidate carriers because representative
string, Boolean, and enum values were read and written without custom property
definitions, including one configuration-specific Description. The other
advertised editable properties remain unproven for writing and are not
implicitly approved.

No occurrence-level metadata bag and no writable instance-name carrier were
proven. Consequently, this study establishes no carrier for distinct metadata
on repeated occurrences. A later annotation convention may define a bounded
allowlist and name fallback only from these results; this report does not choose
fields, precedence, grammar, roles, or schemas.

The conclusions are limited to one synthetic public Free-plan document, one
immutable microversion, the stated endpoint shapes, and two configurations per
source element on the observation date. They do not claim cross-microversion,
cross-configuration, cross-account-tier, or future-API stability.
