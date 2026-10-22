# Local construction MCP server

## Purpose and scope

`ergo-mcp` is a thin stdio/loopback-REST bridge to a minimal local WASM client. An agent can
inspect, place, move, remove, connect, simulate, validate, and render bricks.
Structured feedback explains unsupported/falling parts and missing physical
calibration; the visual tool returns actual PNG image content for vision models.

The server operates on one configured local JSON file. It does not receive S3
credentials and exposes no global publication tool. A future permission-checked
global commit tool belongs on top of the collaboration protocol described in
[concurrent editing](concurrent-global-editing.md).

## Start and configure

Build the native binary from the repository root:

```sh
cargo build --release -p ergo-mcp
cargo xtask build-web
./target/release/ergo-mcp --workspace /absolute/path/construction.ergo.json
```

The parent directory must exist. A missing file starts an empty construction;
successful edits atomically replace the configured working-copy JSON. Existing
Ergo JSON exports can be opened directly. A local advisory lock prevents two MCP
processes from concurrently opening the same file. This is not a distributed
S3 lease.

The default opens the minimal runtime in the existing browser. Use `--attach`
for manual attachment, `--headless` for isolated Chrome automation, and
`ERGO_CHROME_BIN` if Chrome is not in the default location. The runtime does not
mount Leptos or the full app UI, does no continuous idle drawing, and initializes
WebGL only for visual requests. See [architecture](architecture.md) for REST routes.

A generic MCP client's stdio server configuration is:

```json
{
  "mcpServers": {
    "ergo": {
      "command": "/absolute/path/ergo/target/release/ergo-mcp",
      "args": [
        "--workspace", "/absolute/path/construction.ergo.json"
      ]
    }
  }
}
```

The binary writes only newline-delimited JSON-RPC messages to stdout. Diagnostics
belong on stderr. Supported capabilities are tool discovery and tool calls, with
initialization and ping. MCP itself uses stdio; the capability-protected local
HTTP bridge connects the WASM worker. No public remote MCP transport is exposed.

To use operator-supplied connector measurements:

```sh
./target/release/ergo-mcp \
  --workspace /absolute/path/construction.ergo.json \
  --measurements /absolute/path/connector-measurements.json
```

See [physical calibration](physical-calibration.md) for the format and data-quality
requirements. Missing calibration does not prevent experimenting in a working
copy; it does prevent a claim of calibrated publication readiness.

## Tools

The three primary testing tools are:

- `test_stability_by_gravity`: non-destructive short equilibrium probe, default one
  step. Reports displacement, rotation, broken connections, residual motion,
  and whether physical connector capacities are complete.
- `test_visual_appearance`: PNG evidence from an arbitrary supplied camera or
  preset views. A vision model/user judges appearance; the tool does not invent
  an automatic aesthetic pass.
- `test_stability_by_earthquake`: configurable shaking intensity expressed as a
  peak-ground-acceleration **bound in g**, tested independently along X and Y.

For example:

```json
{"steps": 1, "position_tolerance": 0.001, "rotation_tolerance_radians": 0.0001}
```

```json
{
  "camera": {
    "position_world": [50, 60, 30],
    "target_world": [24, 24, 8],
    "vertical_fov_degrees": 45
  },
  "width": 640,
  "height": 480
}
```

```json
{
  "peak_ground_acceleration_g": 0.2,
  "frequency_hz": 4,
  "duration_s": 3,
  "include_images": true
}
```

These are the arguments for the gravity, visual, and earthquake tools,
respectively. Camera coordinates use construction units. Earthquake frequency
and duration use Hz and seconds. Richter/moment magnitude alone does not specify
local shaking: distance, site conditions, and a ground-motion model or measured
accelerogram are needed. The tool does not silently convert a magnitude 6 event
into a made-up acceleration.

The broader editing and inspection tool set is:

- `get_catalog`: allowed part IDs/colors, construction/SI units, and limits.
- `list_bricks`: paginated IDs, parts, poses, bounds, and masses.
- `inspect_brick`: geometry, metric box dimensions, mass source, contact
  candidates, inferred stud relationships, and explicit constraints.
- `place_brick`: place a part by its minimum corner, with optional quarter-turn
  yaw. An overlapping placement is rejected without changing the working copy.
- `move_brick`: translate the selected brick's fixed-connected group; check
  collisions along the path and reject anchored/articulated invalid moves.
- `remove_bricks`: remove bricks and incident joints, then report whether the
  remaining assembly becomes unsupported.
- `connect_bricks`: add or replace a fixed/revolute connection with a world-space
  anchor, optional axle axis, and optional angular limits.
- `list_groups`: memberships, pivots, descendant counts, and derived bounds.
- `create_group`: create a persistent container from pieces and/or other groups.
- `set_group_members`: replace/reparent members, with cycle/parent validation.
- `transform_group`: rigidly edit all descendant poses once, checking destination
  overlaps/floor and mechanical constraints crossing the container boundary.
- `ungroup`: remove a container while preserving pieces, children, and world poses.
- `validate_construction`: overlap/stability feedback and measured-capacity
  completeness; no publication occurs.
- `simulate_gravity`: advance up to four seconds of box-based gravity simulation
  and retain the resulting local pose and unbroken constraints.
- `test_solidity`: non-destructive earthquake or falling-object test, with
  structured failure/calibration feedback and an optional post-test image.
- `test_moderate_earthquake_stability`: a dedicated, reproducible moderate-shake
  preset: separate X/Y tests, each 3 s at 4 Hz with an acceleration bound of
  0.15g, followed by 2 s settling. Returns calibrated/uncalibrated stability
  feedback and optional images without changing the working copy.
- `view_construction`: one to four PNG camera views plus current diagnostics.
- `undo`: restore the immediately preceding local working copy.
- `export_construction`: flush the working-copy JSON and return its configured
  path; this is not a validated S3 snapshot.

Position arguments are in construction units: X/Y use 8 mm per unit and Z uses
9.6 mm per unit. A plate is 1/3 unit high. Forces are newtons, torque is N·m, mass
is kg, angle is radians, and simulation time is seconds. These are unrelated to
the geographic arc-second cell mapping.

## A feedback-driven build loop

1. Call `get_catalog` and choose supported part/color IDs.
2. Place a grounded brick, then add supported parts with `place_brick`.
3. Read the feedback after each edit; an operation's success does not imply that
   the resulting draft is statically publishable.
4. If a brick is unsupported, inspect contacts or render multiple views. Add a
   support or a justified connection; do not add arbitrary anchors to hide an
   unstable arrangement.
5. Simulate if needed, validate again, and export the working copy.
6. Use the web application's validated save path for a private snapshot. A global
   commit will additionally require the future global permission/lease service.

Typical `place_brick` arguments:

```json
{
  "part": "3001",
  "color": 4,
  "min_corner": [10, 10, 0],
  "quarter_turns": 0
}
```

Placing another brick at the same location returns a tool error with the
conflicting IDs. Placing an isolated brick at Z=8 is allowed in the draft but
returns `static_under_current_joint_model: false`, an unsupported-component explanation,
and a suggested next action.

Feedback includes revision, brick count, overlap status, current-model stability,
calibration issues, and `publication_ready`. The latter is only a local check;
it is not an authorization grant. If connector measurements are missing, the
tool distinguishes an idealized unbreakable-joint result from realistic joint
capacity verification.

## Images for vision models

For example:

```json
{
  "views": ["isometric", "north", "top"],
  "width": 640,
  "height": 480
}
```

Directions are `isometric`, `north`, `south`, `east`, `west`, and `top`. “North”
means the camera is north of the construction, looking toward its center. Each
image is returned as MCP `image` content with `mimeType: image/png`, not merely
as a local filesystem path the vision model cannot access.

The image is captured from the **same Rust/WASM WebGL2 renderer** used by the
application. The native bridge does not rasterize it. A browser is required;
headless mode is optional. A separately deployed API server is optional because
the bridge serves its minimal client and local REST routes itself. Cached ground
metadata can be rendered without new map requests in headless automation.
GPU/browser differences mean this is not a cross-device bit-identical promise.

Visual plausibility is not a substitute for collision or stability validation.
A hidden support, an uncalibrated joint, or a tiny overlap may be difficult to
infer from an image alone. Pair images with structured diagnostics.

See [solidity tests](solidity-testing.md) for `test_solidity` load-case arguments,
waveforms, physical units, acceptance criteria, and limitations.

## Current limits

The local workspace is limited to 5,000 bricks. Image dimensions range from
160×120 to 1024×768, with at most four views per tool call. Incoming JSON-RPC
messages are bounded to 1 MiB. Tool calls are processed sequentially, and an
client operation can occupy the stdio request until it completes. Jobs time out
after 120 seconds; expired jobs cannot write a late working-copy update.

Next steps include cancellation/progress, full undo history, support/force overlays,
annotated failure witnesses, server-managed workspace revisions, and authenticated
delegation to the shared-world commit service. They are tracked in
[`TODO.md`](../TODO.md).
