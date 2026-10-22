# Orbit, fly, and walk navigation

Navigation changes the local camera, not piece poses or committed model data.
The full browser app provides a selector in the viewport panel and on mobile.
The minimal MCP runtime renders requested camera poses without running a
continuous navigation loop.

## Orbit

Drag to orbit around the focus; shift/right-drag pans; the wheel zooms. **F** or
the fit button focuses the construction. Selecting a piece before returning to
orbit can use it as the focus. The top-view control returns to orbit mode.

## Fly

Focus the canvas. **Left/right arrows turn**, **up/down arrows look up/down**;
dragging also changes the viewing direction. Hold **A to accelerate** and **Z to
slow down**, including to a complete stop. Fly starts stopped each time it is
entered. Once accelerated, the camera continues forward in the viewing direction
at its selected speed until slowed; arrow keys steer that motion.

Speed changes by 64 stud-scaled units/s per second of held A/Z, bounded to
0–2048 units/s. The wheel and on-screen plus/minus controls also adjust speed,
including from zero. Motion is normalized in metric-proportioned directions so
vertical movement respects the 9.6/8 unit ratio. Fly is a free camera and does not
collide with pieces or the ground.

## Walk

Focus the canvas. **Up/down arrows move forward/backward**, **left/right arrows
turn**, **A/Z increase/decrease walking speed**, and **Space jumps**. Dragging
also changes the viewing direction. Walking stops when up/down is released;
turning or changing speed alone does not translate the camera horizontally.

Walking starts at 18 stud-scaled units/s. A/Z changes the selected speed by
18 units/s per second held, bounded to 0–128 units/s; the wheel and on-screen
plus/minus controls also adjust it. The selected walking speed survives mode
changes. Both free modes turn at 1.8 radians/s while an arrow is held and show
the current speed in the viewport panel. On-screen arrows, A/Z buttons, and the
walk-mode jump button provide the same controls for touch use.

The default eye height is **4 brick-height units** (38.4 mm), a minifigure-scale
view, not an assumed 1.7 m human. The UI permits 1–32 units and displays the
selected height again when returning to Walk mode.

### Steps, jumps, and collisions

The camera follows the global Z=0 ground and supported brick tops. While grounded,
walking automatically steps up a ledge of at most **3 vertical units** (28.8 mm),
provided the whole body has clearance. Taller obstacles require a jump. Auto-step
is kinematic; it checks the upward head sweep and the raised horizontal motion,
so a low overhang blocks the step. Airborne motion cannot auto-step up walls.

Press **Space** to launch a ballistic jump whose unobstructed **feet-height peak
is 12 vertical units above takeoff** (115.2 mm). A ceiling can shorten the jump.
The controller uses standard gravity, converted to construction units:

- `g = 9.80665 / 0.0096 ≈ 1021.526 units/s²`.
- `v₀ = √(2 g × 12) ≈ 156.58 units/s` (about 1.503 m/s).
- `h(t) = v₀ t − ½ g t²`, reaching the peak after about 0.153 s and returning
  to the takeoff height after about 0.307 s in unobstructed flight.

Constant-gravity integration is analytic, split at the apex, with substeps of at
most 1/240 s. Horizontal increments are at most 0.1 unit per axis. The body is
0.56 horizontal units wide and extends 0.25 vertical units above the selected eye
height. Body-box wall checks allow sliding; swept ceiling checks stop upward
velocity at contact, and crossed brick tops catch descending feet even on thin
platforms. Landing cancels downward velocity. Walking off an edge falls under
the same gravity. A fresh press is required for each jump; holding Space or
pressing it in midair does not add another impulse.

Known occupied but unloaded cells block the entire body and its vertical sweep
until their data arrives, rather than allowing passage through unseen pieces.
The shared implementation is in `crates/client/src/walking.rs`; the renderer
supplies its resident octree, camera motion, and streaming boundary checks.

This is a navigation controller, not an additional rigid body in the construction
solver. It does not push bricks, add mass, or participate in commit validation.
Conservative AABBs remain less capable than a full rigid-body character
controller, particularly around rapidly moving simulated objects.

## Shared navigation model

`crates/client/src/navigation.rs` defines the modes, typed controls, keyboard
bindings, pad labels, and movement settings. A single up/down input axis means
pitch in Fly and forward/backward movement in Walk. Keyboard and on-screen
buttons update the same set of controls and use the same input conversion.

`Navigation` owns the current mode, each free mode's persistent speed setting,
and eye height. Throttle and wheel adjustments use the same speed limits. The
UI displays a read-only `NavigationStatus` snapshot rather than maintaining
independent mode, speed, or eye-height settings. Desktop/mobile mode selectors
share one component, as do the movement-pad buttons and their press/release path.

The renderer owns camera pose and drawing, reusing one eye-position conversion
and one display scale derived from the physical brick-height/stud-pitch ratio.
The walking scene owns collision queries; the walker owns vertical velocity and
jump-press state. Both navigation policy and walking logic are ordinary Rust
modules shared by native regression tests and the browser runtime.

## Streaming and ground

All modes drive the same frustum/fog-bounded streaming. Newly visible empty
horizontal cells generate their single-color baseplate lazily. Camera movement
does not preload an infinite world, and changing view mode does not remove
supports from an active complete-construction physics working set.

Public coordinates remain X east, Y north, Z height. See
[ground and coordinates](ground-and-coordinates.md) and
[display indexing](3d-display-indexing.md).
