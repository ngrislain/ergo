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
- [Navigation](navigation.md): orbit/fly/walk controls, 3-unit steps, 12-unit ballistic
  jumps, eye height, full-screen exploration, nearest construction, and collisions.
- [Sun, shadows, and night lighting](lighting.md): geographic UTC solar position,
  camera flashlight, ambient/specular appearance, and shared-renderer limits.
- [Sign-in and agent access](access-and-onboarding.md): landing/SSO pages, token
  setup, shared-world navigation, edit pacing, and operator-managed paid entitlement.
- [Studio/LDraw catalog](studio-catalog.md): open-ended parts, sourced colors,
  standard-first MCP discovery, and measured-physics publication requirements.
- [Nested groups](groups.md): piece/group membership, rigid transforms, pivots, and physics separation.

- [3D indexing for efficient display](3d-display-indexing.md): resident sets,
  octrees, picking, geometry instancing, floating origins, and client caches.
- [Large-scale 2D/3D persistence indexing](persistence-indexing.md): geographic
  coordinates, signed storage cells, boundary ownership, immutable snapshots,
  Morton/Hilbert indexing, and the path beyond a flat manifest.
- [Collision, static stability, and rigid-body simulation](rigid-body-physics.md):
  bounding-box collision semantics, gravity, sparse constraint equations,
  persistence validation, simulation islands, and future destruction.
- [Server pre-commit validation source map](validation-gates.md): overlap,
  stud/joint modeling, gravity balance, and explicit connection-model gaps.
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
  unbounded worlds, stable creation-record identities, region grants, bounded
  on-demand dependency domains, spatial paging, and atomic publication.
- [Transactional CBOR/S3 library](transactional-storage.md): implemented immutable
  octree transactions, independent edits, read/predicate validation, root CAS,
  packed paths, caching, history, durable pins, streaming collection, and shard limits.
- [Storage operations and paging](storage-operations.md): acceptance indexes,
  retention, dense spatial/rule pages, cross-assembly joints, catalogs, revision
  SSE, group publication, benchmarks, and legacy upgrades.
- [Scaleway deployment](scaleway-deployment.md): OpenTofu, CI/local container packaging,
  the existing `ergo` bucket, scale-to-zero, Google SSO, and `ergosandbox.com` DNS/TLS.
- [Public project website](static-site.md): the shared-engine agent guide, privacy/terms,
  GitHub Pages publication, and `www.ergosandbox.com` DNS/HTTPS.
- [Operations runbook](operations.md): shell commands for checks, releases,
  GitHub Pages, DNS/TLS, Scaleway deployment, and private backup/restore.
- [Prebuilt distributions](distribution.md): versioned GHCR server images,
  image-only CI publication, the npm MCP bundle, platform/runtime requirements,
  and maintainer publication setup.
- [Snapshots, deltas, and history](commit-history.md): immutable accepted changes,
  materialized current roots, checkpoints, replay, retention, and garbage collection.
- [Construction MCP tools](local-construction-mcp.md): agent editing,
  structured physics feedback, rendered images, and authenticated remote publication.
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

`ergo-transactions` also backs the authenticated shared-world REST service.
Shared worlds have no aggregate size/count limit. Each edit loads its spatial
and mechanical working set; oversized closures are rejected explicitly.

The following are implementation limits, not mathematical limits of the design:

- Imports contain at most 200,000 parts; uploads are at most 128 MiB.
- The resident set contains at most 128 blocks and 200,000 brick references.
- Interactive simulation/editing uses a complete construction of at most 5,000
  bricks and 128 blocks. It does not simulate a camera-clipped fragment.
- Shared REST edits default to 5,000 loaded/candidate identities and 128 MiB of
  charged CBOR per operation. These configurable budgets are not world-size limits.
  Shared-world navigation uses a bounded read-only region; full CAD editing of
  shared domains in the browser remains future work. MCP/REST publication is implemented.
- The server uses a constructive static support/wrench certificate, with no
  time-integrated fallback. Uncertified cases are rejected conservatively.
- The recommendation catalog contains 27 common part geometries and 20 opaque
  colors; other Studio/LDraw part IDs resolve on demand, and the embedded color
  catalog comes from 322 official LDraw definitions. This is standard-first
  guidance, not an availability restriction or measured popularity ranking.
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
- `crates/transactions/`: reusable CBOR octree transactions and S3 CAS publication.
- `crates/server/src/shared/`: permissioned paged commits, global birth identities,
  and bounded on-demand mechanical dependency validation.
- `crates/server/src/catalog.rs`: private-world summary pages and legacy backfill.
- `crates/server/src/auth.rs`: Google authentication and signed sessions.
- `crates/client/src/renderer.rs`: shared WebGL2 drawing, picking, LOD, fog, and navigation.
- `crates/viewer/src/app.rs`: Leptos controls and streaming orchestration.
- `web/sw.js`: bounded persistent browser cache.
- `crates/mcp/src/main.rs`: thin stdio/REST bridge; `crates/mcp/src/publish.rs`
  handles authenticated shared-world publication; `crates/client/src/runtime.rs`
  and `web/mcp.html`: minimal local WASM tool execution.
- `crates/assets/`: cache-first missing-part fetches and lazy imagery-derived ground.
- `crates/xtask/src/main.rs`: explicit Rust WASM/binding build orchestration via
  `cargo xtask build-web`; native server builds remain independent.

## References

- [LDraw file-format specification](https://www.ldraw.org/article/218.html)
- [Rapier documentation](https://rapier.rs/docs/)
- [WebGL2 instanced rendering](https://developer.mozilla.org/en-US/docs/Web/API/WebGL2RenderingContext/drawArraysInstanced)
- [Scaleway Object Storage](https://www.scaleway.com/en/docs/object-storage/)
- [Google OpenID Connect](https://developers.google.com/identity/openid-connect/openid-connect)
