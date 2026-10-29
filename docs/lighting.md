# Shared renderer: sun, night flashlight, and plastic lighting

The navigator, MCP visual captures, and public landing illustration use
`crates/client/src/renderer.rs`. The public site calls the small WASM wrapper in
`crates/client/src/preview.rs`; there is no separate Three.js renderer. Catalog
meshes, metric proportions, material shading, and shadow implementation are shared.
Static brick outlines use `crates/world/src/illustration.rs`: orthographic
projection and hidden-line removal of the same catalog mesh, scaled by the
physical X/Z 8 mm and Y 9.6 mm units. The empty inspector and website icons share
this projection; they are not hand-drawn differently proportioned bricks.

## Geographic sun and UTC

`crates/client/src/lighting.rs` computes apparent solar declination and equation
of time with NOAA/Meeus Julian-century formulas. Its inputs are the loaded
construction's `GeoReference`, the lighting focus converted with `to_gps`, and the
browser clock's UTC Unix timestamp. The solar calculation refreshes at minute
granularity or when the target enters another approximately 0.0001-degree cell.
Browser time-zone offsets are not used.

The resulting unit vector uses the renderer's internal east/up/south axes,
consistent with the public east/north/up adapter. The navigator passes the
loaded world's reference and MCP captures pass the scene's reference. The public
landing illustration instead uses the cursor light and studio fill below, and never requests
a visitor's device location.

Above the geometric horizon, the sun direction drives direct light, highlights,
shadows, and the visible sky disk. These are approximate astronomical visual
calculations, not a weather model: refraction, clouds, terrain horizons, and
calibrated photometry are not modeled. The browser clock must be correct.
See [NOAA's calculation details](https://gml.noaa.gov/grad/solcalc/calcdetails.html).
Tests include the NREL SPA reference observation within 0.1°, east/west direction,
longitude, equinox day/night, and polar day/night.

## Automatic night flashlight

At or below zero geometric solar elevation, direct sunlight is replaced by a
camera-mounted flashlight aimed toward the view target.
The lamp origin coincides with the rendered camera eye (after metric scale and
floating-origin conversion), and its axis is the actual camera-to-target ray.
There is no sideways/upward offset that would shift nearby walking illumination.
It follows orbit/fly/walk
movement and uses a soft spotlight cone (16° inner / 32° outer half-angle),
distance attenuation, and a perspective shadow projection. Ambient sky light
varies through twilight. This is a visual navigation aid, not a measured
lumen/exposure model or a physics force.

The navigator and ordinary MCP captures render the time-dependent sky and use
a camera-mounted night headlamp.

Orbit, fly and walk share the same shadow coverage. Leaving orbit retains its
lighting focus, sun-map span and night-shadow reach instead of using the free
camera's one-unit aiming distance. Entering walk can lower the eye to a surface
without moving that footprint underground. Subsequent camera translation moves
the lighting footprint by the same displacement; looking around does not move
the sun map. Returning to orbit recaptures its current pivot and zoom. The night
lamp still starts at the actual eye and follows the actual view ray; its cone,
attenuation and brightness are unchanged.

## Homepage cursor light and drag orbit

The public preview uses a transparent background and a cursor-positioned point
key, independent of UTC. Free pointer movement maps to a bounded plane in front
of the current camera. The point source uses per-fragment direction/distance
attenuation (no spotlight cone), with a perspective shadow map aimed at the model.
The homepage scales indirect/environment light to **0.30** and broad studio
fill/rim to **0.35**, making cast shadows stronger without changing the point
key's power. Other entry points use independently configured engine parameters.
The tree crown
uses standard green color 2; its trunk uses brown 70. Ordinary MCP studio captures
retain their fixed directional key; world sun/headlamp behavior is unchanged.

Dragging moves the actual perspective camera around the fixed model target in
azimuth and elevation, from -1.4 to +1.4 radians; brick transforms do not rotate.
Pinch or focused plus/minus keys change distance between 0.55× and 2.5× the fitted view.
Focused arrow keys orbit, plus/minus zoom, and Home or **Reset view** restores
the initial view. Pointer capture permits drags outside the canvas; cancel/lost
capture end the gesture. Hover changes only the light; page/wheel scroll changes
only the page. Neither orbits or zooms the camera. Reduced motion retains explicit
pointer, drag and keyboard controls. While dragging, the point light remains at
its last world position; orbit does not carry it or alter its shadow projection.
Free pointer movement intentionally updates both the light and its shadow map.
The homepage uploads its entire small illustration at fixed geometric detail to
both passes, including off-camera casters. Its stud geometry does not morph with
view distance. This avoids camera-dependent caster/LOD changes during orbit;
tests compare the actual geometry/instance-buffer hashes and projection, not just
the light-position diagnostic. Navigator scenes retain bounded progressive LOD.

## Ambient, diffuse, and specular light

The shared `renderer/shaders.rs` material combines:

- Hemispherical sky/ground ambient diffuse illumination.
- An analytic ambient specular reflection of the sky.
- Direct diffuse light plus GGX/Schlick specular highlights from the active light.
- A visual dielectric reflectance of 0.04 and roughness of 0.36, linear-light
  calculations, and display conversion.
- Existing selection tint, distance fog, and near/distant ground-stud detail.

Near geometric stud tops and the distant plate/procedural surface use the same
base color and material response. Procedural rim shading is suppressed under
nearby real stud geometry to avoid double-darkening at the detail transition.
Stud sides and cast shadows can still naturally be darker than their tops.

These plastic-appearance parameters do not supply physical masses or connector
capacities and do not imply a construction validation certificate.
Released MCP packages use their bundled runtime; rebuilding the current source
is required to pick up unshipped engine changes.

## Cast shadows and limits

`renderer/illumination.rs` renders an instanced depth pass, sampled with a 3×3
percentage-closer filter and depth bias. The shadow map is 1024² when the canvas
is initially at most 600 CSS pixels wide, otherwise 2048². Sunlight uses a bounded
orthographic volume near the target; the flashlight uses a bounded perspective
volume. The map is reused while its matrix and uploaded geometry stay unchanged.
The sun half-span remains `clamp(0.7 × orbit distance, 18, 180)` display units;
the night far plane remains `clamp(3 × orbit eye/focus metric distance, 100, 1600)`.
Free navigation preserves those fitted values across mode switches. This retains
orbit's off-camera caster coverage rather than clipping tall/distant casters to
a small volume around the free-camera aim point. It remains bounded resident
coverage, not cascaded or whole-world shadows.

Shadow casters come from an **independent light-frustum query** of the resident
octree. Camera clipping and color-body LOD do not remove a resident caster.
Auto/Performance navigator policies select source/medium/far display geometry
for reviewed parts according to projected stud size in the shadow
buffer (6/1.25 texels before its adaptive detail divisor, 12% hysteresis).
Each budgeted pass targets 2,000,000 brick triangles in Auto or 500,000 in
Performance. The shadow budget can reduce stud detail independently of color;
it remains a soft target. Cached greedy chunk surfaces can replace eligible
standard bodies in the light volume while preserving body-surface occlusion.
Parts without a reviewed display profile
retain source shadow geometry, as do
selected and highlighted pieces. Window-frame far meshes retain their apertures
and the round-brick far mesh retains its annular silhouette. High, MCP captures and the homepage retain
full source shadow parts. This is visual LOD, not a change to collision bodies.
The caster-instance plan is reused while the light volume/origin/resolution,
scene/meshes, selected/highlighted IDs, explosion and quality policy are unchanged;
the existing GPU map reuse then skips the depth draw when appropriate. Unlike a
scene-fixed sun map, a moving night headlamp often needs a new map every frame,
so a triangle budget and merged caster geometry are important in that case.
Ground studs retain their bounded camera-local detail region.
This is not a whole-world occlusion query: unloaded or light-volume-clipped
casters can still be absent. Full-detail shadow prototypes share GPU geometry
with full-detail color batches; adaptive color proxies need separate prototypes.
Bias/filtering trade contact detail against self-shadowing artifacts. Existing
geometry/instance counters are not a complete GPU-memory account; the depth map
adds roughly four bytes per texel plus driver overhead. GPU resources are
released when the renderer is dropped.
The navigator skips the color pass too while camera/scene/viewport/light are
unchanged; captures still request actual draws. Input controls explicitly
invalidate the view rather than relying on continuous redraw.

Lighting never changes the [server commit checks](validation-gates.md) or makes
a rendered model physically valid.

## Build and checks

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked
cargo test --locked -p ergo-client
cargo check --locked --target wasm32-unknown-unknown -p ergo-client \
  --features mcp-entry,site-entry
cargo xtask build-web
npm run build --prefix site
npm test --prefix site
# With Playwright/Chrome installed:
python3 scripts/test-web-isolated.py --script scripts/test-headlamp.py
python3 scripts/test-web-isolated.py --script scripts/test-navigation-lighting.py
python3 scripts/test-site-orbit.py --url http://127.0.0.1:4173
```

Browser checks use fixed UTC daytime/nighttime clocks, actual WebGL main/depth
draw passes, GL-error inspection, studio-lit front/rear views and navigator flashlight.
Navigation-lighting checks inspect the actual WebGL shadow matrices across
orbit→fly→walk→fly and steering, retaining sunlight's world footprint and the
night lamp's depth reach. Native focus tests additionally cover subsequent movement.
They also cover orbit/zoom/keyboard controls, reduced motion, fallback, the navigator's sky path, and root
login/viewer routing. Canvas diagnostics include `data-renderer`, `data-lighting`,
`data-sun-elevation`, `data-sun-azimuth`, and `data-shadow-map`.
`data-color-instances`, `data-shadow-draw-calls` and `data-instance-upload-bytes`
expose current-frame submission/upload diagnostics (a reused shadow map has zero
depth draw calls for that frame). `data-shadow-lod-counts` and
`data-brick-shadow-triangles-estimate` expose planned brick caster detail/work,
including casters whose depth map is reused; they exclude ground.
See [scene-wide LOD and profiling](3d-display-indexing.md#scene-wide-budget-and-quality-controls).
