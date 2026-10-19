# Technical design

Ergo is a browser-first Rust workbench for very large, geographically addressed
brick constructions. The UI is Leptos; WebAssembly owns rendering and the local
physics sandbox; an Axum service publishes immutable objects to a local directory
or Scaleway's S3-compatible object store. User profiles also live in object storage.

## Reading order

- [Architecture and execution boundaries](architecture.md): browser, thin MCP
  bridge, minimal WASM runtime, REST API, and object storage, with an SVG overview.
- [Ground and coordinates](ground-and-coordinates.md): public Z-up schema, lazy
  single-color baseplates, imagery averaging, palette projection, and source policy.
- [Navigation](navigation.md): orbit, fly, walk, controls, eye height, and collision behavior.
- [Nested groups](groups.md): piece/group membership, rigid transforms, pivots, and physics separation.

- [3D indexing for efficient display](3d-display-indexing.md): resident sets,
  octrees, picking, geometry instancing, floating origins, and client caches.
- [Large-scale 2D/3D persistence indexing](persistence-indexing.md): geographic
  coordinates, signed storage cells, boundary ownership, immutable snapshots,
  Morton/Hilbert indexing, and the path beyond a flat manifest.
- [Collision, static stability, and rigid-body simulation](rigid-body-physics.md):
  bounding-box collision semantics, gravity, sparse constraint equations,
  persistence validation, simulation islands, and future destruction.
- [Constraint solving for interactive positioning](interactive-constraints.md):
  moving connected parts, fixed/revolute joints, constrained dragging,
  collision-aware edits, and saving the result.
- [Physical dimensions, masses, and connection calibration](physical-calibration.md):
  SI conversion, sourced catalog masses, measured force/torque limits, and missing-data behavior.
- [Solidity tests](solidity-testing.md): controlled earthquakes/baseplate shaking,
  dropped-object impacts, SI parameters, and interpretation of results.
- [Identity, ownership, and object-store-only metadata](identity-and-ownership.md):
  Google OpenID Connect, stateless sessions, authorization, and object layout.
- [Concurrent editing of the global model](concurrent-global-editing.md):
  octree regions, hierarchical leases, permissioned commits, and atomic publication.
- [Snapshots, deltas, and history](commit-history.md): immutable accepted changes,
  materialized current roots, checkpoints, replay, retention, and garbage collection.
- [Local construction MCP tools](local-construction-mcp.md): agent editing,
  structured physics feedback, and rendered images for vision models.
- [Annotated references and related systems](references.md): academic background,
  comparable systems, relevance to Ergo, and explicit implementation distinctions.

## The three spaces must stay separate

**Address space** maps a storage cell to GPS. **Construction space** describes
brick geometry. **Physics space** is a local, complete working set with explicit
boundary conditions. A GPS-addressed storage boundary is not automatically a
mechanical joint or a rendering boundary.

The chosen convention is **one 1024³ storage cell per arc-second**, not one
arc-second per individual brick. Public axes are X east, Y north, Z up. Paris defaults to
48.8566° N, 2.3522° E; the origin and angular resolution are configurable.

## Implementation status and limits

The documents mark proposed scale-out work explicitly. The starter implementation
already contains CBOR objects, SHA-256 addressing, browser download caching,
frustum/fog streaming, distance LOD, instanced WebGL2 drawing, an in-memory octree,
LDraw/Studio assembly import, box-based rigid-body physics, fixed/revolute joints,
and a server-side save-validation gate.

The following are implementation limits, not mathematical limits of the design:

- Imports contain at most 200,000 parts; uploads are at most 128 MiB.
- The resident set contains at most 128 blocks and 200,000 brick references.
- Interactive simulation/editing uses a complete construction of at most 5,000
  bricks and 128 blocks. It does not simulate a camera-clipped fragment.
- The server uses a constructive static support/wrench certificate, with no
  time-integrated fallback. Uncertified cases are rejected conservatively.
- The starter catalog contains 27 common part geometries and 20 opaque colors.
  This is a curated selection, not a measured BrickLink popularity ranking.
- Physics uses nominal metric dimensions and sourced catalog masses. Realistic
  connection capacities must come from operator-supplied measurements; no universal
  clutch-force default is invented. A finite gravity test is not a structural proof.

## Source map

- `crates/world/src/lib.rs`: world coordinates, manifests, partitioning, snapshots.
- `crates/world/src/spatial.rs`: octree, contact candidates, stud inference.
- `crates/world/src/coordinates.rs`, `terrain.rs`, and `groups.rs`: interchange,
  single-color ground records, and nested authoring metadata/transforms.
- `crates/world/src/geometry.rs`: shared meshes and catalog approximations.
- `crates/world/src/ldraw.rs`: submodel flattening and geometry baking.
- `crates/world/src/physics.rs`: rigid bodies, joints, manipulation, box checks.
- `crates/world/src/validation.rs`: the persistence validation gate.
- `crates/world/src/physical.rs`: metric dimensions, sourced masses, and connector measurements.
- `crates/world/src/solidity.rs`: gravity, earthquake, and dropped-object test cases.
- `crates/server/src/store.rs`: immutable object publication and metadata access.
- `crates/server/src/auth.rs`: Google authentication and signed sessions.
- `crates/client/src/renderer.rs`: shared WebGL2 drawing, picking, LOD, fog, and navigation.
- `crates/viewer/src/app.rs`: Leptos controls and streaming orchestration.
- `web/sw.js`: bounded persistent browser cache.
- `crates/mcp/src/main.rs`: thin stdio/REST bridge; `crates/client/src/runtime.rs`
  and `web/mcp.html`: minimal local WASM tool execution.
- `crates/assets/`: cache-first missing-part fetches and lazy imagery-derived ground.

## References

- [LDraw file-format specification](https://www.ldraw.org/article/218.html)
- [Rapier documentation](https://rapier.rs/docs/)
- [WebGL2 instanced rendering](https://developer.mozilla.org/en-US/docs/Web/API/WebGL2RenderingContext/drawArraysInstanced)
- [Scaleway Object Storage](https://www.scaleway.com/en/docs/object-storage/)
- [Google OpenID Connect](https://developers.google.com/identity/openid-connect/openid-connect)
