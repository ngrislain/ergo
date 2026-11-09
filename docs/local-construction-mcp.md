# Ergo MCP client

The native stdio bridge hosts a minimal local WASM client. Editing/simulation and
actual WebGL image capture execute there; authenticated I/O commits to the one
shared Sandbox. The bridge never grants itself acceptance or direct agent S3 access.

## Install and configure

```sh
npm install --global --ignore-scripts @ngrislain/ergo-mcp@0.5.34
ergo-mcp --login --publish-url https://ergosandbox.com
```

Version 0.5.26 opens human browser approval and privately saves/picks up a
delegated token automatically; see [browser delegation](access-and-onboarding.md).
This requires the matching client and browser deployment; v0.5.25 does not
gain the behavior retroactively. Manual generation/download remains supported.
Configure an absolute cache path and optionally an explicit private token path.
Consumers need Node 20+ and a local
Chrome/Chromium execution host, not Rust. `--headless` uses isolated Chrome;
`--attach` prints a local capability URL for manual attachment. Normal startup
opens the minimal client. The npm launcher defaults tokenless runs to the hosted
Sandbox; `--local` explicitly disables publication/sign-in for isolated proposals.
Bare native runs with no configured URL retain local-only behavior. `--headless`
affects the WASM execution host, not the human's sign-in browser. Complete
`ergo-mcp --login` first if the MCP host cannot wait for interactive setup.

### OpenCode

```json
{
  "$schema":"https://opencode.ai/config.json",
  "mcp":{
    "servers":{"ergo":{
      "type":"local",
      "command":["ergo-mcp","--workspace","/absolute/path/sandbox.cache.json","--headless"],
      "environment":{
        "ERGO_PUBLISH_URL":"https://ergosandbox.com"
      },
      "timeout":{"startup":660000,"catalog":30000,"execution":3900000}
    }}
  }
}
```

Google sign-in stays in the human's browser. Tokens delegate the signed-in actor,
expire and are origin/purpose-bound. They are not S3 credentials.
Installable source wrappers for Codex and Claude Code are documented in
[MCP plugins](mcp-plugins.md); the underlying transport remains standard stdio MCP.
The dedicated [official Inspector page](mcp-inspector.md) remains unlinked.

## Position and current state

- `get_lego_models` searches/retrieves similar official LDraw or configured custom
  models before new design work, returning sources, licences, thumbnails, part
  inventories and reusable documents. Rebrickable API metadata and BrickLink
  catalogue references are explicitly distinguished from spatial model data.
  See [model-library workflow](model-library.md).
- `get_user_position` reads the caller's private last S3 camera/target and returns
  snapped integer-horizontal `ground_position_world` plus native `model_transform`
  for new standing models. Saved camera/target coordinates remain continuous.
- `open_shared_construction` loads a complete authoritative assembly by ID or
  bounded nearest-position lookup. Accepted edits update that same assembly.
- `get_edit_allowance` returns pacing capacity/refill and the authoring piece limit;
  god mode reports `unlimited:true`, `god_mode:true`, `authoring_piece_limit:null`.
- MCP initialization, `get_catalog` discovery and its returned `building_guidance`
  share one part-choice policy: use minifigure scale by default and call
  `get_lego_models` to retrieve/inspect similar official or custom references first,
  follow their demonstrated part choices/construction patterns and retain applicable
  source attribution; prefer common standard studded bricks over plates, normally
  upright with their studs facing up (+Z). Reserve sideways or inverted orientations
  for rare, clearly justified cases supported by established library building techniques.
  House/palace storeys use nine brick courses (27 plate-height units, or 9 current
  native Z units of room height), with floor slabs added separately;
  prefer purpose-made catalogue roof/sloped parts, windows, doors/frames, fences,
  trees and flowers for architectural/landscape details, resolving actual part IDs,
  geometry and required physical data;
  use plates sparingly for thinner layers, and smooth stud-less tiles for deliberate
  finishing surfaces. Within those preferences use the largest suitable parts,
  preserving shape, colors, support and physically valid connections. Stagger
  adjacent wall courses and interlock corners where geometry permits, using
  half/end bricks to preserve the footprint instead of shifting whole walls.
  Support/connection checks still establish solidity. Static reports may prove
  gravity balance with zero joint wrench (`joint_strength_required:false`).
  Otherwise joints use measured or reviewed connector capacities; an unmeasured
  stud joint carries no load. `max_joint_utilisation` reports the most loaded
  joint's fraction of its capacity.
  The catalog
  resolves native Ergo part descriptions by ID.
- `list_bricks` and `inspect_brick` expose actual IDs, poses, bounds, sourced
  masses and contact/constraint metadata. `inspect_brick` also lists the piece's
  `pivots`: each revolute connection with its `pivot_id`, connection, bearing
  (`free` or `friction`), anchor and the pieces that would turn with it.
- `get_build_feedback` supplies an inspect–act–review–adjust harness: local
  structural/calibration findings, measured part/color counts, actionable next
  tools and heuristic plate/tile review suggestions. Pass `goal` (at most 2000
  bytes) and optionally `include_images:true` for actual WebGL images from up to
  four views. It does not edit, simulate, publish, certify aesthetics or infer
  requested feature correctness from inventories. Review images against the human
  brief, then improve the specific finding. A successful shared receipt remains
  distinct from visual quality; duplicate validation/dynamics is not a default
  gate after every edit. Per-piece examples in heuristic suggestions are capped
  at 20 IDs; counts describe the full local construction. Names in the canonical
  catalogue classify bricks/plates/tiles. Plate review triggers at four or more
  plates when plates outnumber recognized bricks; tile review triggers at four or
  more tiles when tiles exceed half the pieces. These are prompts to inspect an
  intentional design choice, not rejection thresholds.
  Rendered `view_construction`, `test_visual_appearance` and image-inclusive
  feedback responses also expose their local numeric feedback as
  `structuredContent`, alongside real PNG content and the text fallback.
- `get_catalog` lists common starting pieces and versioned common-construct IDs.
  Reference library parts and reusable plate-free `minifigure-v2`/`classic-car-v2`
  models rather than copying definitions.
  - Recorded `set_count` values and hand/wheel/Technic connector endpoints are
    included where reviewed; endpoint coordinates use public part-local brick units.
  - `search` (with `category`, `cursor`, `limit`) pages through every supported
    piece; `list=bricklink` searches the separate BrickLink items without
    official geometry. Each piece states its mass source.
- The MCP installs the verified [part library](part-library.md#distribution) from
  its origin and keeps an offline mirror in `.ergo-assets` (or
  `ERGO_MCP_ASSET_DIR`), including fetched mesh objects. The origin is
  `--publish-url`, else `ERGO_API_URL`, else `https://ergosandbox.com`. After the
  first successful run it starts offline from the mirror.

Gravity reports include `unstable_pieces` with exact IDs/parts, initial/current
positions, displacement/rotation peaks and nearby piece IDs. Use those IDs for
targeted repairs. `test_small_earthquake_stability` compares a one-second gravity
baseline with gravity plus seeded, bounded random horizontal acceleration. Defaults
are 0.01 g, three seconds of shaking, two seconds settling, 0.1-second intervals
and seed42. It reports both cases and calibration separately; optional images
highlight moving pieces red. Neither test changes/publishes the working copy.
`calibrated_stability_verified` requires complete connector calibration as well
as passing the stated model/tolerances; an idealized diagnostic pass is not a
measured clutch-strength guarantee.

The local example generator `scripts/build-world-models.py` records the small-shake
result before `--publish` and withholds example publication unless gravity/shake
survival and connector calibration pass. This additional recipe check does not
replace the server's independent static gate or establish solver convergence.
An explicit `--estimated-connectors` instead runs the disclosed ballpark model
and requires `stable_under_declared_estimates` for that example workflow. It also
runs without `--publish` for local preflight. It is not measured calibration and
cannot bypass the server certificate, which uses only measured or reviewed
joint capacities.

For the MCP small-shake tool, `connection_model:"estimated"` returns the estimate
profile, engagement-based thresholds and unsupported pairs. Results expose
estimated connections separately; `calibrated_stability_verified` stays false.

Positions are public east/north/up construction units, not GPS degrees or cell
indices. New models can pass the saved `model_transform` to `place_submodel`.
Existing construction edits preserve their absolute accepted positions.

## Native construction and editing

Unreleased source after v0.5.34 uses [paged shared authoring](paged-authoring.md).
`open_shared_construction` loads a manifest, not geometry. Explicitly call
`read_construction_page` with spatial `bounds`, piece `ids`, or a continuation
`cursor`; `append:true` keeps previous pages within the 2,048-piece/32 MiB resident
budget. Replies scope local inspection/rendering to the loaded view. Local full
validation/dynamics are refused on partial bound views; server commits still
validate complete affected domains. New and changed pieces use sparse `PATCH`
deltas. `export_construction` returns a separate full accepted export path; do
not read the resident workspace cache as a full model. The old full-workspace
protocol/sidecars require the explicit coordinated migration in that guide.

In shared mode `list_groups(cursor,limit)` pages authoritative group headers,
without geometry. `read_construction_group` (`group_id`) replaces the view with a
complete bounded descendant closure: nested definitions, pieces, internal
explicit joints and anchors. This is not a physical-domain certificate. Unloaded
or globally managed constraints fence transforms, and the manifest's group-ID
floor prevents collisions with unseen groups. Dissolving a loaded child preserves
its authoritative unloaded parent. See [group loading](paged-authoring.md#mcp-workflow)
and [coupled partitioned equilibrium](partitioned-stability.md).

Constructions already use the migrated canonical source library. Only current
common-construct versions are supported; the completed migration's
`retrofit_library` tool is retired. Native saves/exports intern identical local
definitions and reference matching current common constructs without changing
occurrence IDs/poses. Authoritative shared validation and accepted readback still
establish publication.

`place_submodel` optionally accepts `replace_ids`: remove those local piece IDs
and place the replacement in **one WASM update / one accepted shared edit**.
Unchanged pieces retain their IDs; new IDs exceed the opened manifest's highest
live member ID, not merely the loaded page's highest ID.
Incident joints, anchors and group references are removed consistently. Invalid
overlap/geometry leaves the original scene intact. Replacements use the exact
supplied catalogue poses rather than snapping a hair/socket datum to a new
overall bounding-box corner; authoritative ground/support checks still apply.
This supports replacing cars/hair or wall sections without publishing an
unsupported intermediate state. Ordinary additions retain coherent snapping.

Repository authoring recipes use the shared `scripts/mcp_client.py` stdio client;
demo, monument and district drivers share its request/error/close handling.

Native publication requests and complete-assembly read-back responses are bounded
to 128 MiB each. The response cap also applies while streaming when no length is
declared. An accepted write whose read-back fails must be reconciled by reopening
its recorded assembly; never repeat the placement merely because read-back failed.

For a resolved part, `get_catalog` includes render `bounds`, nominal `body_bounds`,
mass/source, source credits and collision-model limitations. Use nominal body
datums for CAD placement rather than inferring them from decorative studs or
source-mesh extrema. Specialty catalogue profiles provide geometry interfaces
and average masses, not calibrated joint strength; see [part profiles](part-catalog.md#physical-and-material-limits).

Grow an opened construction through small self-supporting published edits,
preserving its assembly identity. Publication performs authoritative validation;
a separate `validate_construction` or expensive dynamic probe is not a default
prerequisite for every step. Use atomic batches where intermediate states would
be unsupported, inspect real rejection details, and request additional diagnostics
when useful or explicitly wanted. Existing local collision/integrity feedback
and server validation remain enabled. Keep working user feedback and revise
rejected approaches. These workflow suggestions are user-confirmed guidance,
separate from the attributed building-technique source.

`place_submodel` takes exactly one `model` (native `ergo-model-1` JSON) or `document`
(an inline imported text model). The optional `transform` is a row-major4×4 matrix
in native public units. Color omission inherits; the placement context defaults to
standard gray. Imported source units are converted at the boundary.

Models can reuse definitions. Existing/resolved parts are shared geometry; known
catalog shapes can use labeled nominal geometry. Nonstandard geometry must be
resolved or supplied by the import. Placement is an atomic batch and rejects
overlap, floor crossing and metric scale/shear/reflection for physical bricks.
Every nested model instance must be a proper metric isometry, including unused
definitions; cancelling parent/child scales are rejected.

- `place_brick`: snap a requested minimum corner to studs/plate heights, then add
  the supported part; feedback includes the applied placement and snap delta.
- `move_brick`: snap the destination of its fixed-connected component coherently,
  with collision checks. A snapped zero move leaves cache/revision unchanged.
- `remove_bricks`: remove parts and incident constraints, reporting lost support.
- `connect_bricks`: author a fixed or explicit revolute constraint.
- `rotate_about_pivot`: turn a piece by any angle (`angle_radians`) about one of
  its pivots (`pivot_id` from `inspect_brick`). The pieces on its side of that
  pivot turn exactly about the connection axis, with no grid snap. Loops through
  the pivot, anchored pieces, floor crossings and overlaps are rejected; joints
  are re-inferred. Use it to pose arms, wrists and held equipment, hinges and
  Technic beams on pins or axles.
- `create_group`, `set_group_members`, `transform_group`, `list_groups`, `ungroup`:
  nested authoring membership and rigid transforms; grouping is not welding.
- `undo`: restore the preceding local working copy; in shared mode mutations still
  require server acceptance.
- `export_construction`: flush/identify the current native cache file.

See [construction format](construction-format.md) and [groups](groups.md).

Incoming submodels are snapped with one translation of the lowest receiving-grid
datum, using the nominal union corner as fallback. Existing pieces and relative internal poses are preserved. The
server independently rejects off-grid grounded footprints; an internally
misregistered source model is not repaired by rounding every piece.

## Sourced building techniques

Call `get_building_techniques` before generating architectural geometry and propose
the relevant techniques. Optional `query`, `category` (`walls`, `junctions`,
`openings`, `floors`, `roofs`) and exact `technique_id` filter the curated library.
For example, `{"query":"Swiss chalet"}` returns attributed patterns for bonded
walls, corners/T-junctions, lintels, floors and roofs. Filtering is case-insensitive
keyword matching, not live semantic catalogue/model search.

The initial `ergo-building-techniques-1` library paraphrases
[Brick Builder's Handbook: Basic LEGO Techniques](https://brickbuildershandbook.com/basic-lego-techniques/).
Nine entries cover running bond, interlocked corners/T-junctions, English bond,
supported openings, staggered slabs, stepped roofs, catalogue-slope roofs and
dormers. Slab guidance is an explicit application of the source's overlap
principle. Slope/dormer entries identify their additional catalogue/physics needs;
they do not invent available slope assets or measured connectors. Guidance is not
a stability certificate. Source: `crates/world/src/techniques.rs`.

The generator in `scripts/chalet_model.py` applies six of these patterns to a
24×16-stud, two-storey chalet with nine upright brick courses per room, staggered
floors, bonded wall junctions, an external staircase and balcony. Its shallow
studs-up roof uses overlapping standard plates with supported gables rather than
scaled bricks or freely tilted slabs. Window/door openings are brick-built, not
claimed catalogue inserts. `scripts/build-world-models.py` records the selected
library guidance and emits an actual-walker route for each chalet. Preflight and
publication receipts remain separate.

## Validation and images

- `validate_construction`: overlap/static feedback and calibration gaps.
- `test_stability_by_gravity`: non-mutating short equilibrium probe, default one
  step with explicit tolerances.
- `simulate_gravity`: let gravity act (at most 8 s, damped, stopping once at rest),
  project every connection back onto its exact geometry and keep that pose as a
  local edit, then validate it; not a global commit. A Technic chain on free pins
  settles into its hanging (funicular) shape and becomes publishable. The result's
  `settled` reports the duration, whether it came to rest and the projection
  residual.
- `test_stability_by_earthquake`: bounded X/Y shake by local PGA, not magnitude.
- `test_moderate_earthquake_stability`: named 0.15g,4Hz preset.
- `test_solidity`: declared earthquake or dropped-object load case.
- `view_construction`: 1–4 actual preset WebGL images.
- `test_visual_appearance`: actual images from presets or an explicit camera.

Studio lighting is time-independent for inspection; ordinary world lighting uses
the geographic sun/night headlamp. Images do not establish physical acceptance.

## Publication and recovery

In configured shared mode accepted mutations commit automatically. Read their
`shared_commit` confirmation, accepted revision and server audit. A local file or
successful image is not a remote receipt. `publish_construction` confirms an
existing receipt, renames a shared construction or submits a validated proposal;
it cannot create another hosted world.

Rejected edits restore the preceding accepted cache. Concurrent same-assembly
changes require reopening/reapplying. Unknown outcomes are fenced and never
automatically repeated. The `.shared.json` journal stores origin, world, assembly,
base revision and pending state, not a credential. Paged bindings use
`format:"ergo-workspace-binding-2"`, loaded posed-record versions, a read cursor,
count summary and ID allocation floor. Old ETag/full-file sidecars are explicitly
migrated, not read through a compatibility path. Opening loads the current
manifest; pages are loaded on demand at that revision. HEAD checks allow 30 seconds;
they never grant offline access or authorize a commit. Browser regions use
the separate [durable preparation checkpoints](loading-and-cache.md#durable-browser-loading-checkpoints).

Ordinary HTTP requests default to 300 seconds; complete construction transfers
(GET `workspace`, `view`, `download`, `export`, and the workspace PATCH that commits
an assembly edit) default to 3,600 seconds. Explicit
`ERGO_MCP_HTTP_TIMEOUT_SECONDS` accepts 1–3,600 and overrides both. A queued
WASM/MCP job allows 3,600 seconds for local work and remote phases together.
Configure the host MCP execution timeout to at least 3,900,000 ms. A timeout is not
proof of rejection: reconcile the authoritative assembly/receipt before any new
attempt. No publication request is automatically retried.

Native models are limited to 768KiB inline, imported text to128KiB. Local editing
has no 5,000-piece cap; dynamics simulations remain limited to5,000 bodies.
Shared operations normally budget10,000 identities/128MiB; god-mode authoring
removes the identity-count cap, not the paged MCP residency limit. External dependencies prevent pretending a clipped local
simulation is complete. Explicit ballpark diagnostics never acquire measured
calibration status or override the server's measurement requirements.

Source: `crates/world/src/mcp.rs`, `crates/mcp/src/`, `crates/client/src/runtime.rs`
and `web/mcp.html`. See [agent workflow](agent-shared-editing.md).
