# Edit Ergo Sandbox with an agent at a position

There is one **public Ergo Sandbox**. Anyone can view it; signed-in users can
edit it, subject to pacing and physical validation. Current MCP clients commit
accepted edits to that same S3-backed world automatically. A working file is a
cache, not a private world. Use MCP **v0.5.13** for sourced model references and building techniques,
coherent grid snapping, proper-isometry checks, per-piece stability diagnostics,
explicit estimated connector tests and long shared operations.

Call `get_lego_models` to search and retrieve similar official/custom examples
first; inspect their poses, parts and source attribution, and follow their demonstrated
part choices and construction patterns, adapting them to the requested shape and
retaining source attribution where applicable. Prefer common standard bricks with studs over plates. Use plates sparingly when
thin layers are needed, and smooth stud-less tiles for deliberate finishing
surfaces. Minimize piece count within these preferences while preserving shape,
colors, support and measured connection requirements. The same guidance is exposed
by MCP initialization and `get_catalog`.

Call `get_building_techniques` and propose relevant sourced patterns before
generating geometry. Its curated starter library includes running bond,
interlocked L/T junctions, lintels, overlapping slabs and roof techniques, with
explicit additional-asset requirements for slopes/dormers. It is guidance rather
than a physical certificate or semantic model-library search.

Use minifigure scale by default for architectural details, stairs, furniture and
landscaping. Prefer purpose-made catalogue roof/sloped parts, window and door
frames, doors, fences, trees and flowers where appropriate. Resolve actual part
IDs/geometry through `get_catalog` and obtain the physical data validation needs.

Normally place bricks upright with their studs facing up (+Z). Sideways or inverted
orientations are reserved for rare, clearly justified cases supported by established
library building techniques.

House and palace storeys use nine standard upright brick courses: 27 plate-height
units. Current native Z units are full brick heights, so this is 9 native vertical
units of room height. Convert the physical height using the active coordinate
scale; floor slabs are additional to the room height.

Stagger adjacent wall courses, typically by half a brick, and interlock corners
where geometry permits. Use suitable half/end bricks to maintain the footprint,
openings and support rather than shifting whole walls outward. Avoid continuous
vertical seams. This is a construction rule, not a substitute for checking support
and measured connections or a guarantee of solidity by itself.

Place and move complete models coherently on the stud/plate grid. The client
snaps the nominal union corner, preserving relative poses; the server separately
rejects off-grid grounded footprints and scale/shear/reflection. Every hierarchy
instance must be a proper metric isometry, even if nested invalid scales would
cancel in the final part pose. Camera coordinates are not placement coordinates.

## Start from the viewer's current position

1. Open [Ergo Sandbox](https://ergosandbox.com/), sign in, and navigate to the place
   you want to work on. Select a brick for an exact construction selection.
2. Click **Edit here with an agent** (the command-symbol button in the header).
3. The setup page carries the current view target in public construction
   coordinates, plus the selected/edited assembly ID when available. Generate
   and download a short-lived token. Copy its MCP configuration into your agent.
4. Use a fresh, absolute cache path for that context. Keep the token in a private
   file and put only its path in the configuration.

Example configuration (replace the file paths):

```json
{
  "mcpServers": {
    "ergo": {
      "command": "ergo-mcp",
      "args": ["--workspace", "/absolute/path/ergo-here.cache.json"],
      "env": {
        "ERGO_PUBLISH_URL": "https://ergosandbox.com",
        "ERGO_PUBLISH_TOKEN_FILE": "/absolute/path/ergo-mcp-token",
        "ERGO_POSITION_WORLD": "[128,64,0]"
      }
    }
  }
}
```

For a selection the handoff also includes `ERGO_SHARED_ASSEMBLY_ID`. It takes
precedence over nearest-position selection. The UUID/assembly hash are identifiers,
not credentials. Sign-in stays in the human's browser; never pass Google or S3
credentials to the agent.

## Specify a position explicitly

Positions are `[east,north,height]`, with 8 mm horizontal units and 9.6 mm height
units. They are not GPS degrees or storage-cell indices.

Set `ERGO_POSITION_WORLD` as the JSON string above before starting a new cache,
or have the agent call this on an already running configured MCP server:

```json
{"name":"open_shared_construction","arguments":{"position_world":[128,64,0]}}
```

To choose a known construction exactly:

```json
{"name":"open_shared_construction","arguments":{"assembly_id":"REPLACE_WITH_64_HEX_ASSEMBLY_ID"}}
```

Position lookup chooses the nearest assembly among candidates in a bounded
±1536-unit search box. It is not a whole-world nearest-neighbor scan. An exact
assembly ID is preferable when several constructions are close together. Inspect
`list_bricks`/bounds after opening. An empty search is reported explicitly; on
startup an empty position can initialize a new construction cache with the
sandbox's geographic reference.

For GPS, first use the viewer's **Go to a location** dialog, then the handoff.
The conversion is documented in [ground and coordinates](ground-and-coordinates.md):
`x = Δlongitude × 3600 × 1024 / arcseconds_per_block`,
`y = Δlatitude × 3600 × 1024 / arcseconds_per_block`, with longitude wrapping.
For example, in the default Paris reference, `[128,64,0]` is approximately
48.8566173611° N, 2.3522347222° E. Viewer `/app?x=...&y=...&z=...` links use
**block indices**; `/login` handoff coordinates and MCP positions use construction
units. Do not interchange them.

## A useful agent prompt

> Open the selected shared construction at the supplied position. Inspect its
> parts, bounds and connections first. Modify the façade as requested, preserving
> other users' work. Minimize the part count for the required shape: prefer the
> largest suitable standard bricks/plates over many small parts, while preserving
> shape, colors, support and connectivity. Inspect actual front/isometric renders.
> Return each confirmed shared revision and author attribution; do not describe
> local previews, failed validation, or uncertain requests as committed edits.

Discover tools with `tools/list`; use `get_catalog`, `list_bricks`, and
`inspect_brick`. `place_brick`, `place_submodel`, removal, groups and transforms
update the **same** shared assembly. `place_submodel` can submit an inline MPD
batch atomically, avoiding a stream of unsupported intermediate states.

Read `shared_commit` in a successful mutation result. It contains the accepted
revision and server audit. `publish_construction` can confirm a receipt, rename a
construction, or submit an existing validated proposal; it cannot create another
world. There is no `authenticated_editing=false` private-world option in the
production tool schema.

## Acceptance, conflicts, and cache recovery

Native publication HTTP requests default to 300 seconds. Source-built bridges
after v0.5.11 support `ERGO_MCP_HTTP_TIMEOUT_SECONDS` from 1–1800 for a diagnosed long
operation; the server/proxy deadline
and the enclosing MCP job deadline still apply. Increasing a timeout never retries
a mutation. Transport errors include the HTTP method and preserve their cause;
an unconfirmed result still requires authoritative reconciliation.

Cold whole-assembly reads warm member and piece records from the accepted snapshot
in bounded 512-member pages with at most 64 reads in flight. The ordinary working
read still charges every record and verifies identities/dependencies; this is not
a trusted client cache or an alternative authoring path.

For compact whole-assembly reads, the external-dependency guard uses one
conservative bounding query plus explicit peer/joint checks, rather than one
spatial query per member. Its own members are excluded using their full IDs in
validated spatial keys, avoiding unused self-reference payloads.
Nearby unrelated candidates can conservatively block a
clipped local simulation. Remote explicit joint endpoints are still checked;
authoritative publication continues to discover/validate the complete affected domain.

Prefer gradual edits to the same assembly in small supported batches. Server
publication validates the affected domain, so avoid a duplicate standalone
pre-validation/dynamics chain by default. Additional diagnostics are for explicit
requests or specific problems. Successful user feedback should shape subsequent
edits; genuine catalogue architectural parts remain preferred.

The server revalidates the affected mechanical dependency domain, permissions and
quota, then publishes with compare-and-swap. It assigns names from the account
profile, not tool arguments: `audit.actor` is stable; `audit.actor_name` is the
display name at acceptance. Names are authenticated attribution, not unique
usernames or a personal cryptographic signature.

- Invalid MCP edits restore the preceding accepted cache; no successful shared
  mutation is reported. The browser can retain a clearly labeled **Preview only**
  proposal for inspection/correction, but does not call it saved.
- Concurrent changes to the same assembly cause a conflict instead of overwriting
  another editor's work. Reopen it with `open_shared_construction`, inspect the
  current state, and apply the intended change again.
- Unknown outcomes are fenced and never automatically retried. Resolve them from
  accepted history/assembly read-back before another mutation.
- The `.shared.json` sidecar records origin, sandbox ID, assembly, accepted base
  revision and pending state. It contains no token. Do not delete a pending journal
  merely to repeat an unconfirmed import. On restart, a bound cache reloads the
  authoritative shared assembly.
- A bounded authoring assembly is limited to the configured working-set budget
  (normally 5,000 pieces/128 MiB; god-mode authoring has no piece-count cap).
  Assemblies with external mechanical dependencies
  do not claim a complete isolated client simulation; server validation still
  follows the full affected dependency closure.

Interdependent multi-assembly rebuilds can use the authenticated atomic REST
batch-replacement route described in the [shared protocol](concurrent-global-editing.md#rest-workflow).
It preserves assembly identities and validates one final state; the MCP tools
continue to target one opened assembly at a time.

Use `test_visual_appearance`/`view_construction` with `lighting: "studio"` for
time-independent inspection. Gravity and earthquake tests are non-mutating
diagnostics, not permission to bypass the server's certificate or measurements.

See [the native construction format](construction-format.md) and
[MCP tools](local-construction-mcp.md).
