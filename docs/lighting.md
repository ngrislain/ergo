# Shared renderer: sun, night flashlight, and plastic lighting

The navigator, MCP visual captures, and public landing illustration use
`crates/client/src/renderer.rs`. The public site calls the small WASM wrapper in
`crates/client/src/preview.rs`; there is no separate Three.js renderer. Catalog
meshes, metric proportions, material shading, and shadow implementation are shared.

## Geographic sun and UTC

`crates/client/src/lighting.rs` computes apparent solar declination and equation
of time with NOAA/Meeus Julian-century formulas. Its inputs are the loaded
construction's `GeoReference`, the view target converted with `to_gps`, and the
browser clock's UTC Unix timestamp. The solar calculation refreshes at minute
granularity or when the target enters another approximately 0.0001-degree cell.
Browser time-zone offsets are not used.

The resulting unit vector uses the renderer's internal east/up/south axes,
consistent with the public east/north/up adapter. The navigator passes the
loaded world's reference and MCP captures pass the scene's reference. The
landing example uses the default Paris reference (48.8566° N, 2.3522° E) and
never requests a visitor's device location.

Above the geometric horizon, the sun direction drives direct light, highlights,
shadows, and the visible sky disk. These are approximate astronomical visual
calculations, not a weather model: refraction, clouds, terrain horizons, and
calibrated photometry are not modeled. The browser clock must be correct.
See [NOAA's calculation details](https://gml.noaa.gov/grad/solcalc/calcdetails.html).
Tests include the NREL SPA reference observation within 0.1°, east/west direction,
longitude, equinox day/night, and polar day/night.

## Automatic night flashlight

At or below zero geometric solar elevation, direct sunlight is replaced by a
camera-mounted flashlight aimed toward the view target. It follows orbit/fly/walk
movement and uses a soft spotlight cone (16° inner / 32° outer half-angle),
distance attenuation, and a perspective shadow projection. Ambient sky light
varies through twilight. This is a visual navigation aid, not a measured
lumen/exposure model or a physics force.

The public preview uses a transparent background to keep its guide readable.
The navigator and ordinary MCP captures render the time-dependent sky.

## Ambient, diffuse, and specular light

The shared `renderer/shaders.rs` material combines:

- Hemispherical sky/ground ambient diffuse illumination.
- An analytic ambient specular reflection of the sky.
- Direct diffuse light plus GGX/Schlick specular highlights from the active light.
- A visual dielectric reflectance of 0.04 and roughness of 0.36, linear-light
  calculations, and display conversion.
- Existing selection tint, distance fog, and near/distant ground-stud detail.

These plastic-appearance parameters do not supply physical masses or connector
capacities and do not imply a construction validation certificate.

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
```

Browser checks use fixed UTC daytime/nighttime clocks, actual WebGL main/depth
draw passes, GL-error inspection, and the automatic flashlight. They also cover
hover/drag/scroll, reduced motion, fallback, the navigator's sky path, and root
login/viewer routing. Canvas diagnostics include `data-renderer`, `data-lighting`,
`data-sun-elevation`, `data-sun-azimuth`, and `data-shadow-map`.
