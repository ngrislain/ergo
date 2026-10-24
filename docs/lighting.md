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
construction's `GeoReference`, the view target converted with `to_gps`, and the
browser clock's UTC Unix timestamp. The solar calculation refreshes at minute
granularity or when the target enters another approximately 0.0001-degree cell.
Browser time-zone offsets are not used.

The resulting unit vector uses the renderer's internal east/up/south axes,
consistent with the public east/north/up adapter. The navigator passes the
loaded world's reference and MCP captures pass the scene's reference. The public
landing illustration instead uses the fixed studio setup below, and never requests
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

## Homepage studio lighting and orbit

The public preview uses a transparent background and fixed studio lighting,
independent of UTC. Its world-fixed key points along normalized `(-0.6,1,0.8)`
in renderer axes and uses the shared shadow pass. Broad cool fill/rim directions
and a diffuse ambient lift keep rear/side faces readable while orbiting; they
do not have separate shadow maps. The key, fill and rim stay fixed as the camera
moves, while specular highlights change with the view. This is a presentation
setup, not a claim about the construction's geographic sun.

Dragging moves the actual perspective camera around the fixed model target in
azimuth and elevation, from -1.4 to +1.4 radians; brick transforms do not rotate.
Wheel/pinch zoom changes camera distance between 0.55× and 2.5× the fitted view.
Focused arrow keys orbit, plus/minus zoom, and Home or **Reset view** restores
the initial view. Pointer capture permits drags outside the canvas; cancel/lost
capture end the gesture. Reduced motion disables incidental animation but retains
direct controls. Page scrolling remains available outside the interactive canvas.
The studio shadow projection is scene-fixed too. A cast shadow changes screen
position/perspective during camera orbit, but its light-space projection does not
rotate with the camera; this is separate from a moving view-dependent highlight.

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
The prebuilt v0.3.1 MCP package predates these visual upgrades; its captures use
that bundled runtime. Current-source MCP builds share the updated renderer.

## Cast shadows and limits

`renderer/illumination.rs` renders an instanced depth pass, sampled with a 3×3
percentage-closer filter and depth bias. The shadow map is 1024² when the canvas
is initially at most 600 CSS pixels wide, otherwise 2048². Sunlight uses a bounded
orthographic volume near the target; the flashlight uses a bounded perspective
volume. The map is reused while its matrix and uploaded geometry stay unchanged.

Shadow casters are bounded by the loaded/rendered geometry and LOD. This is not
a whole-world occlusion query: distant, clipped, or unloaded casters can be absent.
Bias/filtering trade contact detail against self-shadowing artifacts. Existing
geometry/instance counters are not a complete GPU-memory account; the depth map
adds roughly four bytes per texel plus driver overhead. GPU resources are
released when the renderer is dropped.

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
python3 scripts/test-site-orbit.py --url http://127.0.0.1:4173
```

Browser checks use fixed UTC daytime/nighttime clocks, actual WebGL main/depth
draw passes, GL-error inspection, studio-lit front/rear views and navigator flashlight.
They also cover orbit/zoom/keyboard controls, reduced motion, fallback, the navigator's sky path, and root
login/viewer routing. Canvas diagnostics include `data-renderer`, `data-lighting`,
`data-sun-elevation`, `data-sun-azimuth`, and `data-shadow-map`.
