# Ergo MCP client

The native stdio bridge hosts a minimal local WASM client. Editing/simulation and
actual WebGL image capture execute there; authenticated I/O commits to the one
shared Sandbox. The bridge never grants itself acceptance or direct agent S3 access.

## Install and configure

```sh
npm install --global --ignore-scripts 'https://github.com/ngrislain/ergo/releases/download/v0.5.7/ngrislain-ergo-mcp-0.5.7.tgz'
ergo-mcp --login --publish-url https://ergosandbox.com
```

Generate/download a delegated token in the signed-in browser, then configure a
private token path and absolute cache path. Consumers need Node 20+ and a local
Chrome/Chromium execution host, not Rust. `--headless` uses isolated Chrome;
`--attach` prints a local capability URL for manual attachment. Normal startup
opens the minimal client. Quit/restart the MCP client after configuration/token changes.

### OpenCode

```json
{
  "$schema":"https://opencode.ai/config.json",
  "mcp":{
    "ergo":{
      "type":"local",
      "command":["ergo-mcp","--workspace","/absolute/path/sandbox.cache.json","--headless"],
      "environment":{
        "ERGO_PUBLISH_URL":"https://ergosandbox.com",
        "ERGO_PUBLISH_TOKEN_FILE":"/absolute/private/path/ergo-mcp-token"
      },
      "enabled":true,
      "timeout":2100000
    }
  }
}
```

Google sign-in stays in the human's browser. Tokens delegate the signed-in actor,
expire and are origin/purpose-bound. They are not S3 credentials.

## Position and current state

- `get_user_position` reads the caller's private last S3 camera/target and returns
  snapped integer-horizontal `ground_position_world` plus native `model_transform`
  for new standing models. Saved camera/target coordinates remain continuous.
- `open_shared_construction` loads a complete authoritative assembly by ID or
  bounded nearest-position lookup. Accepted edits update that same assembly.
- `get_edit_allowance` returns pacing capacity/refill and the authoring piece limit;
  god mode reports `unlimited:true`, `god_mode:true`, `authoring_piece_limit:null`.
- MCP initialization, `get_catalog` discovery and its returned `building_guidance`
  share one part-choice policy: use minifigure scale by default and inspect available model-library examples first,
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
  gravity balance with zero joint wrench (`joint_strength_required:false`);
  `connector_calibration_issue` still discloses unavailable clutch measurements,
  which remain required for dynamic probes or certificates relying on bond strength.
  The catalog
  resolves native Ergo part descriptions by ID.
- `list_bricks` and `inspect_brick` expose actual IDs, poses, bounds, sourced
  masses and contact/constraint metadata.

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
cannot bypass a server certificate that requires measured joint strength.

For the MCP small-shake tool, `connection_model:"estimated"` returns the estimate
profile, engagement-based thresholds and unsupported pairs. Results expose
estimated connections separately; `calibrated_stability_verified` stays false.

Positions are public east/north/up construction units, not GPS degrees or cell
indices. New models can pass the saved `model_transform` to `place_submodel`.
Existing construction edits preserve their absolute accepted positions.

## Native construction and editing

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
- `create_group`, `set_group_members`, `transform_group`, `list_groups`, `ungroup`:
  nested authoring membership and rigid transforms; grouping is not welding.
- `undo`: restore the preceding local working copy; in shared mode mutations still
  require server acceptance.
- `export_construction`: flush/identify the current native cache file.

See [construction format](construction-format.md) and [groups](groups.md).

Incoming submodels are snapped with one translation of the new pieces' nominal
union corner. Existing pieces and relative internal poses are preserved. The
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
- `simulate_gravity`: retain the resulting local working pose; not a global commit.
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
base revision and pending state, not a credential. Bound caches reload server
authority instead of interpreting stale local file formats.

Each publication/read-back HTTP request has a 300-second deadline; a complete
WASM/MCP job allows 1,800 seconds for refined dynamics, local work and both remote phases.
Configure the host MCP execution timeout to at least 2,100,000 ms. A timeout is not
proof of rejection: reconcile the authoritative assembly/receipt before any new
attempt. No publication request is automatically retried.

Native models are limited to 768KiB inline, imported text to128KiB. Local editing
has no 5,000-piece cap; dynamics simulations remain limited to5,000 bodies.
Shared operations normally budget5,000 identities/128MiB; god-mode authoring
removes the identity-count cap. External dependencies prevent pretending a clipped local
simulation is complete. Explicit ballpark diagnostics never acquire measured
calibration status or override the server's measurement requirements.

Source: `crates/world/src/mcp.rs`, `crates/mcp/src/`, `crates/client/src/runtime.rs`
and `web/mcp.html`. See [agent workflow](agent-shared-editing.md).
