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

Focus the canvas, drag to look, and use WASD or arrow keys to move. Q/E descend
and ascend; Shift increases speed. The wheel adjusts movement speed. Fly is a
free camera and does not collide with pieces or the ground.

Motion is normalized in metric-proportioned directions, so diagonal inputs do not
unintentionally increase speed and vertical movement respects the 9.6/8 unit
ratio. The default speed is 64 stud-scaled units/s, configurable with the wheel.

## Walk

Drag to look, use WASD/arrows to move, Shift for faster motion, and Space to jump.
The default eye height is **4 brick-height units** (38.4 mm), a minifigure-scale
view, not an assumed 1.7 m human. The UI permits a different eye height.

The camera follows the global Z=0 ground and supported brick tops. A lightweight
client-side controller checks body boxes, permits one-plate steps, slides along
blocked axes, and applies gravity while descending. Known occupied but unloaded
cells block walking until their data arrives, rather than allowing a walk
through an unseen construction. On-screen movement controls support touch use.

This is a navigation controller, not an additional rigid body in the construction
solver. It does not push bricks, add mass, or participate in commit validation.
Conservative AABBs and simple stepping are less capable than a full character
controller, particularly around rapidly moving simulated objects.

## Streaming and ground

All modes drive the same frustum/fog-bounded streaming. Newly visible empty
horizontal cells generate their single-color baseplate lazily. Camera movement
does not preload an infinite world, and changing view mode does not remove
supports from an active complete-construction physics working set.

Public coordinates remain X east, Y north, Z height. See
[ground and coordinates](ground-and-coordinates.md) and
[display indexing](3d-display-indexing.md).
