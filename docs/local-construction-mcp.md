# Ergo MCP client

The native stdio bridge hosts a minimal local WASM client. Editing/simulation and
actual WebGL image capture execute there; authenticated I/O commits to the one
shared Sandbox. The bridge never grants itself acceptance or direct agent S3 access.

## Install and configure

```sh
npm install --global --ignore-scripts 'https://github.com/ngrislain/ergo/releases/download/v0.5.1/ngrislain-ergo-mcp-0.5.1.tgz'
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
      "timeout":180000
    }
  }
}
```

Google sign-in stays in the human's browser. Tokens delegate the signed-in actor,
expire and are origin/purpose-bound. They are not S3 credentials.

## Position and current state

- `get_user_position` reads the caller's private last S3 camera/target and returns
  `ground_position_world` plus native `model_transform` for new standing models.
- `open_shared_construction` loads a complete authoritative assembly by ID or
  bounded nearest-position lookup. Accepted edits update that same assembly.
- `get_edit_allowance` returns pacing capacity/refill and the authoring piece limit;
  god mode reports `unlimited:true`, `god_mode:true`, `authoring_piece_limit:null`.
- MCP initialization, `get_catalog` discovery and its returned `building_guidance`
  share one part-choice policy: inspect available model-library examples first,
  follow their demonstrated part choices/construction patterns and retain applicable
  source attribution; prefer common standard studded bricks over plates;
  use plates sparingly for thinner layers, and smooth stud-less tiles for deliberate
  finishing surfaces. Within those preferences use the largest suitable parts,
  preserving shape, colors, support and physically valid connections. Stagger
  adjacent wall courses and interlock corners where geometry permits, using
  half/end bricks to preserve the footprint instead of shifting whole walls.
  Support/connection checks still establish solidity. The catalog
  resolves native Ergo part descriptions by ID.
- `list_bricks` and `inspect_brick` expose actual IDs, poses, bounds, sourced
  masses and contact/constraint metadata.

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
overlap, floor crossing and metric scale/shear for physical bricks.

- `place_brick`: add a supported part by minimum corner.
- `move_brick`: move its fixed-connected component with collision checks.
- `remove_bricks`: remove parts and incident constraints, reporting lost support.
- `connect_bricks`: author a fixed or explicit revolute constraint.
- `create_group`, `set_group_members`, `transform_group`, `list_groups`, `ungroup`:
  nested authoring membership and rigid transforms; grouping is not welding.
- `undo`: restore the preceding local working copy; in shared mode mutations still
  require server acceptance.
- `export_construction`: flush/identify the current native cache file.

See [construction format](construction-format.md) and [groups](groups.md).

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

Native models are limited to 768KiB inline, imported text to128KiB. Local editing
has no 5,000-piece cap; dynamics simulations remain limited to5,000 bodies.
Shared operations normally budget5,000 identities/128MiB; god-mode authoring
removes the identity-count cap. External dependencies prevent pretending a clipped local
simulation is complete. Missing joint measurements never become guessed strength.

Source: `crates/world/src/mcp.rs`, `crates/mcp/src/`, `crates/client/src/runtime.rs`
and `web/mcp.html`. See [agent workflow](agent-shared-editing.md).
