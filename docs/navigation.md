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
a snapped ground-level `model_transform` for new standing models. Raw saved
camera/target remain continuous. Existing edits retain
their accepted absolute coordinates. See [agent placement](agent-shared-editing.md).

## Scrolling construction list

Compact construction rows occupy a fixed-height scrolling zone on the left,
immediately after World coordinates. The zone keeps its height as items/pages
arrive; long names are truncated visually with the full name on hover. It is
shorter on mobile. The list pages accepted assemblies in bounded batches. Scrolling near the end
loads another page; Load more and Refresh list remain explicit controls. Requests
are coalesced and repeated identities are deduplicated.

Rows show the construction's creator, recorded UTC creation date/time and piece
count. New assembly births capture server time and author attribution; renames
and replacements retain them. Historical births identify their creator but did
not record dates, so those rows say **Date unavailable**, not the last edit or
migration date. Old headers can recover counts from at most 5,000 member rows;
larger uncounted historical assemblies show an unavailable size until metadata is
available. New headers store their exact count.

The sort control orders loaded rows by name, author, creation time or size, with
a reverse-order button. Name/author comparisons are case-insensitive; missing
dates/sizes stay last. When further pages remain, **Sorts loaded constructions**
makes the scope explicit. This does not claim a globally indexed whole-world sort.

Clicking a construction asks for one current member's bounded location hint,
opens that shared region and focuses its resident assembly bounds. The world
geometry retains neighboring constructions at their actual positions. Browsing
does not fetch an isolated authoring workspace or publish an edit. The selected
assembly is included in the agent handoff even without a separately selected brick.
When the accepted region confirms that all members of the selected assembly are
already resident, selection reuses that geometry immediately. Partial/unknown
membership still requests a server location and region. Region refreshes keep
unchanged GPU prototypes instead of clearing every mesh buffer. See
[loading and caching](loading-and-cache.md).

After selecting a construction, **Download construction** exports its complete
accepted assembly as self-contained LDraw/MPD or native Ergo JSON. It uses a
revision-consistent authoring read, rather than downloading only the visible
region. Workspace JSON/MPD actions separately export the current local workspace.
See [interchange export](construction-format.md#standard-ldrawmpd-export).

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

## Checking a walking route

The compass rotates geographic north relative to the current camera heading in
orbit, fly and walk modes. Fullscreen/mobile navigation buttons flow below the
location/title; view tools occupy a separate right-hand column. Compass, nearest,
attribution and render-status pills use one lower-right layout stack with an
allocated compass area; short landscape viewports place the compass beside the
pills. Browser regression checks cover eight compass headings in normal,
fullscreen and mobile layouts. Keyboard focus
uses a neutral thin indicator and an explicit focus label, with scroll-preserving
canvas focus.
The Orbit/Fly/Walk group sizes to its buttons, including in fullscreen, so its
border does not leave an empty column on the right.

`cargo run --locked -p ergo-client --example check_walk -- CONSTRUCTION ROUTE.json`
loads a complete native construction and runs `walking::Walker` at 120 Hz, with
gravity/stepping and no jumping or teleporting between checkpoints. It fails on
blocked paths, wrong floor heights or insufficient specified headroom. This is
geometry/navigation verification, not a physical acceptance or earthquake test.

The `ergo-walk-route-1`/schema2 JSON contains `origin_world`, relative `start` and
`waypoints` (`name`, `feet`, optional arrival `min_clearance`), `eye_height`,
continuous-route `min_clearance` and `speed`. Arrival overrides can verify full
room height separately from stairhead clearance. Coordinates
use the usual public east/north/up units. Use a complete cache, not a clipped
browser region or an assembly whose external collision dependencies are omitted.
The tool checks the supplied scene; it does not fetch or publish world state.
