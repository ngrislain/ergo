# Construction MCP server

## Purpose and scope

`ergo-mcp` is a thin stdio/loopback-REST bridge to a minimal local WASM client. An agent can
inspect, place, move, remove, connect, simulate, validate, and render bricks.
Structured feedback explains unsupported/falling parts and missing physical
calibration; the visual tool returns actual PNG image content for vision models.

Editing operates on one configured local JSON file. The optional
`publish_construction` tool sends a snapshot of that working copy to the
authenticated shared-world REST service described in
[concurrent editing](concurrent-global-editing.md). The bridge never writes S3
heads directly or receives S3 credentials. Remote publication is distinct from
local edits, exports, and simulation feedback.

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

## Remote publication setup

The operator fixes the destination and credential when starting `ergo-mcp`:

```sh
ERGO_PUBLISH_URL=https://ergo.example \
ERGO_PUBLISH_TOKEN_FILE=/absolute/path/mcp-token \
./target/release/ergo-mcp --workspace /absolute/path/construction.ergo.json
```

The equivalent flags are `--publish-url` and `--publish-token-file`. Flags
override their environment variables; a token file takes precedence over
`ERGO_PUBLISH_TOKEN`, which can supply the token directly. The MCP process does
not automatically load `.env`. These settings belong in the MCP client's process
environment/configuration, not in tool arguments. The destination must be an
HTTPS origin without credentials, a path, query, or fragment. HTTP and an omitted
token are allowed only on loopback; a Google-enabled loopback server still
requires authentication. `ERGO_API_URL` remains a separate part-asset setting.

Obtain a delegation from the target server while signed in: the landing page
provides **Generate access token**, **Copy token**, and **Download MCP token**.
`ergo-mcp --login --publish-url https://ergosandbox.com` opens that setup/SSO page
and exits; start the bridge normally after saving/configuring the downloaded
token. This is browser-assisted setup, not an automatic cookie/device-code exchange.
The same endpoint is also available from the browser developer console:

```js
const response = await fetch('/api/auth/mcp-token', {
  method: 'POST',
  headers: {'Content-Type': 'application/json', 'X-Ergo-Request': '1'},
  body: JSON.stringify({expires_in_seconds: 3600})
});
if (!response.ok) throw new Error(await response.text());
const grant = await response.json();
copy(grant.token); // Browser developer-console helper
```

Save the copied token in a file readable only by your account. It delegates
shared-world reads, world creation, assembly imports, and geometry commits as the
signed-in user; current world/region grants still apply. It cannot change grants,
retention, private snapshots, or account plans, and cannot mint another delegation.
Older publication-only tokens cannot commit existing geometry; issue a new edit
token for direct-API geometry changes.
Its lifetime defaults to one hour, is bounded to 1–86400 seconds, and cannot
outlive the issuing Google session. It is tied to `ERGO_PUBLIC_URL` and the
server signing secret. Logout does not individually revoke stateless delegations;
expiry or signing-secret rotation invalidates them. A development server uses
its process-local random signing secret, so its tokens expire on restart.

### `publish_construction`

Create a new shared world and import the working copy:

```json
{"name": "Statue of Liberty"}
```

Import a **new assembly** into an existing world:

```json
{
  "world_id": "12345678-1234-4234-8234-123456789abc",
  "base_revision": "<64-character lowercase commit CID>",
  "expected_local_revision": 12
}
```

All fields are optional. `name` overrides only the published name (1–120
characters); it does not rename the local file. `world_id` selects an existing
world with the same geographic origin. Without `base_revision`, the bridge reads
that world's current head; an explicit base uses the service's optimistic
dependency validation/rebase rules. `base_revision` requires `world_id`.
`expected_local_revision` rejects a stale local snapshot before network writes;
local revisions are per bridge process. Publishing again adds another assembly,
not an in-place update or an idempotent save. Overlapping an existing assembly is
subject to the ordinary server validation gate.

New MCP-created worlds default to `authenticated_editing: true`: other signed-in
users can read/edit them under their own pacing allowance. Pass
`authenticated_editing: false` when creating an owner/grant-only world. This
argument is only for new worlds; existing access changes require the owner.

The native bridge captures the authoritative local scene, calls the REST service,
then reads back the accepted world revision and assembly. Success returns
`published: true`, `publication_state: "confirmed"`, `world_id`, `assembly_id`,
`accepted_revision`, local-to-global `pieces`, the server `audit`, and URLs.
`view_url` opens the browser's bounded shared-world navigation view;
`assembly_url` and `world_url` are authenticated, revision-qualified metadata
API URLs for the confirmed revision. The live browser view follows the current
head, not a frozen historical scene. URLs contain no bearer tokens. Local poses
and the working-copy revision are unchanged.

Server authorization, working-set budgets, overlap/ground/static checks, and the
world's frozen connector-measurement model remain authoritative. Local
`publication_ready` does not override a rejection. The bridge augments local
feedback's `global_commit_available` according to publication configuration.

Errors are MCP tool errors with structured details: `http_status` and
`server_response` when available, including conflict/validation reasons and any
candidate CID. `publication_state` distinguishes `not_attempted`, `rejected`,
`unknown`, and `accepted_unconfirmed`. Unknown/unconfirmed outcomes return
`published: null`; do not repeat the non-idempotent import automatically. Inspect
authenticated accepted history using the world/candidate/revision before deciding
whether to retry. No HTTP redirect is followed and no failed request is
automatically retried. Each HTTP request has a 90-second timeout, a 128 MiB
request-body cap, and an 8 MiB response-body cap.
Schema or missing-configuration errors may contain only the standard MCP text error.

New-world creation and assembly import are separate transactions. If creation
succeeds but import is rejected, an empty world can remain; the error includes
its `world_id` and `world_created`. Reuse that world after correcting the model.
Publication does not use the local WASM job queue or its 120-second job timeout.

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

- `get_catalog`: standard-first part/color recommendations, units, and limits.
  Use `include_extended: true` for the embedded full color palette and
  `part: "3039"` to inspect another resolved Studio/LDraw part by ID.
- `get_edit_allowance`: read the remote actor's remaining whole brick changes,
  capacity/refill period, or unlimited paid entitlement using configured credentials.
- `list_bricks`: paginated IDs, parts, poses, bounds, and masses.
- `inspect_brick`: geometry, metric box dimensions, mass source, contact
  candidates, inferred stud relationships, and explicit constraints.
- `place_brick`: place a part by its minimum corner, with optional quarter-turn
  yaw. An overlapping placement is rejected without changing the working copy.
  Other resolvable Studio/LDraw part IDs are accepted; missing physical data is
  reported without inventing mass, body dimensions, or connector capacities.
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
- `publish_construction`: publish through the configured shared-world service and
  confirm the accepted revision by remote read-back; see setup and arguments above.

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
6. Use `publish_construction` for a shared-world assembly, or the web application's
   validated save path for a private snapshot. Both paths retain server-side
   authorization and validation; neither is implied by a local export.

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
annotated failure witnesses, server-managed workspace revisions, idempotent
publication/recovery, and native MCP updates of previously published assemblies.
Direct shared-world geometry commits already support scoped edit tokens. Follow-ups are tracked in
[`TODO.md`](../TODO.md).
