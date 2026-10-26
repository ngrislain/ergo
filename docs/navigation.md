# Navigation, live position and constructions

The workbench and shared-world viewer use the same camera/controller. Public
coordinates are X east, Y north, Z up; navigation has one physical-scale conversion.

## Controls

- **Orbit:** drag to orbit, Shift/right-drag to pan, wheel to change distance.
- **Fly:** arrows steer; A accelerates, Z slows to a stop; drag looks around.
  Movement follows the rendered view direction and uses continuous body-volume
  collision sweeps/sliding, without walk stepping or gravity.
- **Walk:** up/down move, left/right turn, A/Z adjust speed, Space jumps.
  Focus the canvas for keys. Walking steps up at most 3 vertical units and jumps
  up to 12 units with metric gravity, collision/ceiling checks and landing.
- **F:** fit; **N:** nearest construction; **Shift+F:** full screen; Esc exits.
  Touch controls expose the same typed navigation actions.

Eye height is adjustable from 1 to 32 construction units. Unloaded cells block the
whole moving body rather than allowing passage into unknown geometry.

## Live camera GPS

The viewport displays camera latitude/longitude to seven decimals, public storage
cell and height Z. It updates from the actual eye, including orbit/pan/fly/walk,
at roughly four UI updates per second. Height is a construction coordinate, not
GPS altitude. This needs no device-location permission.

The sidebar's **Coordinate reference** is the fixed geographic origin used for
conversion. It does not move with the camera. Signed-in position context is saved
privately in S3 at most once per three seconds while changing, with a best-effort
keepalive flush on hiding/leaving the page. Per-session sequence numbers prevent
late requests from overwriting a newer observation from that session.

The MCP `get_user_position` tool reads this user's saved camera/target and supplies
a ground-level `model_transform` for new standing models. Existing edits retain
their accepted absolute coordinates. See [agent placement](agent-shared-editing.md).

## Scrolling construction list

The list pages accepted assemblies in bounded batches. Scrolling near the end
loads another page; Load more and Refresh list remain explicit controls. Requests
are coalesced and repeated identities are deduplicated.

Clicking a construction asks for one current member's bounded location hint,
opens that shared region and focuses its resident assembly bounds. The world
geometry retains neighboring constructions at their actual positions. Browsing
does not fetch an isolated authoring workspace or publish an edit. The selected
assembly is included in the agent handoff even without a separately selected brick.

## Shared-region boundaries

The browser loads a bounded 3×3×3 storage-cell neighborhood, at most 5,000 pieces
and 16 MiB of CBOR. It checks the small accepted head every five seconds and
reloads when its revision or region changes. This navigation set is not a complete
mechanical domain; direct clipped-fragment simulation is rejected.

A cell locates data. It does not weld a building to the ground and its vertical
index does not create another floor. Models rest on the Z=0 baseplate or use
explicit valid ground anchors. All accepted positions are server-validated.

Implementation: `viewer/src/location.rs`, `presence.rs`, `common.rs`, `app.rs`;
shared controls/collisions are in `client/src/navigation.rs` and `walking.rs`.
