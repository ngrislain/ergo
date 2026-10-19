# Client/server/MCP architecture

![Ergo execution and persistence architecture](figures/architecture.svg)

## Execution boundaries

The deployed server is an **I/O and commit-validation service**. It does not run
interactive rigid-body integration, earthquakes, impact tests, or image rendering.
Those operations execute in Rust/WebAssembly on the user's device.

There are two client entry points, sharing the renderer and simulation code:

- **Browser application:** Leptos UI plus the Rust WebGL2 renderer and WASM physics.
  It streams visible construction blocks inside a finite fog horizon, uses distance
  LOD, loads single-color ground cells lazily, and caches downloaded assets.
- **Minimal MCP client:** a small HTML/JavaScript REST adapter and the dedicated
  `ergo-client` WASM module. It does not mount the Leptos application. WebGL is
  initialized only for visual tools; transient rendering buffers are released
  after tool execution. There is no continuous rendering loop while idle.

The minimal client normally attaches in an existing local browser session.
`--attach` allows manual attachment, and `--headless` creates an isolated Chrome
session for automation. Headless Chrome is an optional execution host, not part
of the deployed server and not a claim of a zero-footprint browser runtime.

## The MCP server is a thin bridge

`ergo-mcp` translates MCP stdio requests into jobs for a loopback HTTP REST bridge.
The local WASM client polls for jobs, performs the requested work, and posts its
results back. Images are actual WebGL2 canvas PNGs, not native software renders.

The bridge has responsibility for:

- MCP protocol/schema validation and job delivery.
- A loopback-only, capability-protected REST API.
- Atomic local working-copy file replacement and revision checks.
- On-demand part delivery from a persistent local cache, or an optional upstream
  `ERGO_API_URL` REST asset service.

The bridge has **no native rigid-body simulation or rendering implementation**.
Neither the bridge nor its local working-copy endpoint grants global commit
permission. A local draft may be unstable while an agent is building it.

### Local REST routes

- `GET /api/local/workspace`: current scene, working-copy revision, and operator
  measurement data for client tests.
- `PUT /api/local/workspace`: replace the local working copy for a live job and
  expected revision. This is not a global commit.
- `GET /api/local/jobs/next`: retrieve the next MCP job, or 204 while idle.
- `POST /api/local/jobs/result`: return structured feedback and optional PNGs.
- `GET /api/local/parts/{part}`: cache-first supported part geometry.

Each route requires the local client capability. It is passed in the browser URL
fragment and then an HTTP header, not a public query parameter. Expired jobs cannot
write a late working-copy update. No CORS access is enabled for other origins.

## Deployed REST API

The Axum server handles identity, ownership, model import/export, immutable object
delivery, lazy ground generation, and part-asset caching. Its main routes include:

- `/api/auth/*` and `/auth/google/*`: identity and application sessions.
- `/api/worlds`: list accessible snapshots or validate/publish a new private snapshot.
- `/api/worlds/{id}` and `/api/worlds/{id}/objects/{hash}`: authorized metadata/data.
- `/api/import`: parse into an uncommitted working copy.
- `/api/parts/{part}`: retrieve or pull and persist a supported LDraw part.
- `/api/ground/{x}/{y}`: retrieve or lazily generate a horizontal ground cell.

User profiles, manifests, blocks, source/cache objects, and geometry are backed by
the same object-store abstraction: local files in development or Scaleway S3.
There is no application database. Remote credentials never enter the browser or
MCP WASM runtime.

## Server commit gate

A client simulation is useful feedback, but is not trusted as commit authority.
Before publishing a snapshot, the server recomputes/validates:

1. Schema, allowed parts/colors, finite rigid poses, and nominal body bounds.
2. Non-overlap and the global ground boundary at public height Z=0.
3. Valid constraint endpoints and grounded world fixtures.
4. A conservative, constructive static gravity certificate: nonnegative ground
   reactions, force/torque equilibrium, admissible contact support, and connection
   capacity checks on a selected constraint/contact forest.
5. Server-owned measured connector capacities for ordinary published connections.

The gate uses spatial indexing and sparse graph/wrench arithmetic. It does not
advance a physics timestep. A certificate may conservatively reject a configuration
that a more general equilibrium solver could certify. A successful client gravity
test cannot bypass that rejection.

The `ergo-world` crate's `simulation` feature is optional. The browser/minimal WASM
client enables it; the server and native MCP bridge do not. Consequently ordinary
server builds do not depend on Rapier. Asset decoding/baking and palette sampling
are small, cached data-processing tasks, not 3D rendering or dynamic simulation.

## Client rendering and simulation

The shared renderer lives in `crates/client/src/renderer.rs`. Full-detail meshes
remain the source for picking while projected-size LOD chooses detailed meshes,
parametric parts, body boxes, or coarse colored aggregates for display. Frustum
tests and a finite fog distance bound visible residency; the ground is generated
as needed rather than preloading an infinite plane.

The WASM physics engine uses nominal metric geometry, sourced part masses, and
configured measured connector capacities. It works on a complete mechanical
working set rather than removing supports when the camera turns away.
Render LOD never substitutes lower-fidelity bodies into the physics model.

MCP gravity, earthquake, impact, and visual tools execute in the same client-side
code paths. The MCP client stays idle between jobs; it does not run a background
simulation merely because the MCP connection is open.

## Coordinates and versioning

Schema 2 public/persisted coordinates are **X east, Y north, Z up**. The internal
renderer/physics kernel uses X east, Y up, Z south, with an explicit proper-rotation
adapter for vectors, bounds, matrices, and meshes. This preserves handedness and
part geometry while exposing the requested Z-up interface.

Schema-1 Y-up data is preserved but is not silently interpreted as schema 2.
Reimport older constructions or perform an explicit migration. See
[ground and coordinates](ground-and-coordinates.md) for the ground-plane and
geographic mapping details.

## Current versus future collaboration

Private immutable snapshots and shared read-only examples are implemented. The
global shared branch, user/region commit grants, hierarchical octree leases,
fencing, and distributed publication protocol are specified in
[concurrent global editing](concurrent-global-editing.md), but are future work.

WebGPU acceleration is also proposed, not implemented. The baseline remains
WebGL2 rendering plus WASM CPU simulation. If profiling justifies a GPU solver,
keeping simulation and rendering buffers on a common WebGPU backend is preferable
to repeated GPU→CPU→WebGL2 transfers. Server commit checks remain independent.

## Source map

- `crates/viewer/src/app.rs`: browser UI, REST data loading, and streaming orchestration.
- `crates/client/src/renderer.rs`: shared WebGL2 drawing, picking, fog, and LOD.
- `crates/client/src/runtime.rs`: minimal WASM MCP tool implementation.
- `web/mcp.html`: small local REST/job adapter, without the application UI.
- `crates/mcp/src/main.rs`: thin stdio/REST bridge and local file persistence.
- `crates/world/src/mcp.rs`: shared MCP schemas.
- `crates/world/src/validation.rs`: lightweight static commit certificate.
- `crates/world/src/physics.rs` and `solidity.rs`: optional client simulation feature.
- `crates/assets/`: cache-first part delivery and lazy imagery-average ground generation.
- `crates/server/`: deployed REST API, authentication, and local/S3 persistence.
