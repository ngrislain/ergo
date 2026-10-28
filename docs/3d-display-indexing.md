# 3D indexing for efficient display

## Goal: make frame cost depend on what is resident

A construction may contain far more bricks than the browser can draw or hold in
memory. The renderer should never need to enumerate the entire world every frame.
There are two distinct questions:

1. Which persisted blocks should be resident near the camera?
2. Which resident objects participate in a draw, pick, or proximity query?

![Camera neighborhood, resident octree, shared meshes, and drawing](figures/display-pipeline.svg)

The client selects manifest cells intersecting the camera frustum and finite fog
range, then builds a resident octree. The tree prunes individual draw candidates;
geometry is batched by part and detail level. Display residency, render LOD, and
the complete working set required by physics are separate concerns.

## One block-oriented engine, three entry points

The navigator, homepage and MCP captures use one scene-planning and WebGL2
pipeline. Entry points select explicit quality/visibility policies, camera/input
and lighting parameters; background transparency does not select geometry policy.

- `crates/client/src/scene/view.rs` owns physical-scale projection, floating origin,
  frusta and pick-ray unprojection, shared by drawing, streaming and picking.
- `scene.rs` produces deterministic part/LOD batches and **separate camera and
  light-space instance lists** from the resident octree.
- `scene/ground.rs` produces bounded baseplate/stud instances without a browser/GPU
  dependency. The same CPU planner is usable in native regression tests.
- `renderer/batches.rs` owns immutable prototype buffers, per-pass instance buffers,
  draw submission and resource release. Replacing a prototype invalidates its
  derived LOD/GPU entries; unchanged prototypes survive streaming updates.
- `renderer/illumination.rs` prepares the light, then renders depth from the
  independent caster list. `renderer/shaders.rs` supplies shared block deformation
  and material code. Stud thresholds are generated from the Rust detail constants.
- `renderer.rs` orchestrates planning, upload, depth, sky and opaque-color passes.
  Preview, navigator and MCP wrappers do not implement alternative draw backends.

This adopts game-engine ideas of shared prototypes, data-oriented instance batches,
hierarchical visibility, explicit passes, change-driven uploads and LOD hysteresis;
see [instancing references](references.md#s5-engine-style-instancing-and-pass-reuse).
Blocks make prototype reuse unusually effective: palette colors/transforms are
instance attributes, not separate copies of a part's vertices. Render chunks are
derived from residency, not mechanical assembly boundaries or S3 file boundaries.
This is a small ordered pass pipeline. Its hierarchy, quality policies and buffer
reuse are implemented in the modules above.

Interactive views use adaptive detail and camera culling. MCP captures retain
full source geometry for camera-visible pieces. The tiny homepage illustration
keeps every color instance at full detail. All use light-volume culling for
resident shadow casters; camera clipping no longer removes an offscreen caster.

## Frustum- and fog-bounded streaming

For camera position $\mathbf{c}$ and block side $B=1024$:

$$
\mathbf{k}_c=\left\lfloor\frac{\mathbf{c}}{B}\right\rfloor.
$$

Let $F$ be the view frustum and $d_f$ the fog cutoff. The desired persisted set is:

$$
\mathcal{R}=\{\mathbf{k}:B_{\mathbf{k}}\cap F\ne\varnothing,
\quad \operatorname{dist}(\mathbf{c},B_{\mathbf{k}})\le d_f\}.
$$

Only existing construction cells in this set are fetched, with four concurrent
block requests. The ground layer independently creates missing **visible**
horizontal cells lazily, each with one imagery-derived palette color. There is no
infinite-world pre-generation; see [ground and coordinates](ground-and-coordinates.md).

Fog starts at $0.65d_f$ and reaches the background color at $d_f$. The default
cutoff is 2,200 stud-scaled render units; the UI range is 200–4,096. Residency is
bounded to 128 construction blocks and 200,000 references. If the desired set
exceeds those bounds, the client shortens its effective fog distance instead of
silently omitting cells inside that distance. A single dense cell can still need
smaller render pages; an explicit budget failure is reported in that case.

Requests carry a world-generation check, and late results outside the current
view are discarded from memory. The current sparse manifest is scanned to select
candidate storage cells.

## Boundary-crossing bricks

A brick is not cut at cell boundaries. Partitioning inserts a reference into
every cell intersected by its **render bounds**, including studs. The client
deduplicates by brick ID before drawing and indexing. Using only the origin cell
would make a brick disappear while its geometry is still visible from a neighbor.

Replication is limited to 64 cells per brick. For very large spanning structures,
use a separate oversized-object index rather than unbounded replication.

Display bounds include every rendered vertex. Physics body bounds intentionally
exclude decorative studs for catalog parts. Reusing either bound indiscriminately
would introduce a correctness bug in the other subsystem.

## Octree organization

An octree recursively subdivides a cube into eight children. Each brick's AABB is
stored in the smallest node whose volume fully contains it. A brick crossing a
split stays in its parent; it is not duplicated across all eight child nodes.

For a query box $Q$, traversal skips any node $N$ satisfying:

$$
N_{\max,x}<Q_{\min,x}\quad\lor\quad N_{\min,x}>Q_{\max,x}
$$

or the analogous condition on Y or Z. Parent-held objects must still be tested
when visiting a node. Omitting them loses large objects crossing split planes.

The implementation uses a leaf target of 24 objects and maximum depth 16.
Average local query work is often close to $O(\log N+K)$ for well-distributed
objects and $K$ results, but this is **not a worst-case guarantee**. A large number
of objects spanning the same split plane can cause $O(N)$ traversal.

Currently a bounded resident octree is rebuilt after loading/eviction or a pose
update. For larger resident sets, build independent per-block/page trees, reuse
unchanged trees, and keep a small top-level index over those pages. Physics should
use its own temporally coherent broad phase, not rebuild the display tree at
every solver substep.

## Implemented frustum culling

A frustum consists of six half-spaces. For inward plane normal $\mathbf{n}$,
offset $d$, AABB center $\mathbf{a}$, and half-extents $\mathbf{h}$, the box is
outside when:

$$
\mathbf{n}\cdot\mathbf{a}+d+
|n_x|h_x+|n_y|h_y+|n_z|h_z<0.
$$

This test is applied to octree nodes and their parent-held objects before mesh
instances are assembled. A radial fog-range test removes additional candidates.
Camera, viewport, pose, or fog changes invalidate the instance lists.

## Progressive distance detail

Approximate a part's projected diameter by:

$$
s_{px}\approx\frac{H\,D}{2d\tan(\theta_v/2)},
$$

where $H$ is viewport height, $D$ the metric-proportioned bound diameter, $d$ camera
distance, and $\theta_v$ vertical field of view. The implemented discrete tiers are:

- At least 24 px: detailed supplied/cached Ergo geometry.
- At least 8 px: simplified parametric mesh for known rectangular catalog parts,
  or a reviewed specialty display mesh.
- At least 2 px: nominal rectangular body box, or the specialty's far mesh.
- Below 2 px: coarse colored bounds aggregated in 8-unit spatial buckets.

The listed thresholds initialize a piece's tier. Subsequent changes use a **12%
hysteresis band**: refine above 1.12× a boundary and coarsen below 0.88× it.
Large camera moves can cross several tiers in one update. Unreviewed nonrectangular/custom
parts retain their canonical mesh through the medium tiers, rather than
substituting a box that changes their silhouette; subpixel aggregates still apply.
The selected piece keeps detailed geometry; highlighted group members retain at
least box-level individual display. Full resident geometry remains available for
picking, and physics never substitutes these render LODs for its body model.
Fog hides the final distance cutoff. A scene-wide detail bias can move a part
between the first three tiers, but aggregation still requires its **unbiased**
projected size to be subpixel-scale. Merging resolvable façade/window pieces into
colored envelopes would create overlapping bounds and damage the architecture.

### Scene-wide budget and quality controls

The navigator's **Render detail** control selects an explicit policy:

- **Auto (balanced)** targets 2,000,000 estimated visible-brick triangles.
- **Performance** targets 500,000.
- **High (per-brick LOD)** retains the original per-brick thresholds without a
  scene-wide target, and uses full source meshes for resident shadow casters.

For budgeted modes, the planner counts the cost of all camera-visible candidates
at a common projected-size divisor of 1, 2, 4 or 8. It coarsens until the estimate
fits or reaches 8, then refines only with 25% spare budget. The per-piece 12%
hysteresis still applies. This is a fixed **soft triangle target**, not a frame-time
controller or a guaranteed cap. Source-only/custom silhouettes, selected pieces,
individual highlighted pieces and the stud transition can keep it above target.
Ground is outside this brick target; aggregate costs conservatively count each
candidate before merging. Picking, collision, resident piece count and saved
models retain their full geometry. Captures and the homepage illustration keep
their full-detail policies independently of the navigator control.

Shadow LOD uses the light buffer's projected stud size, rather than the color
camera's distance: source mesh at 6 texels or more, reviewed medium mesh from
1.25 texels, otherwise a reviewed far mesh, with 12% hysteresis. Source-only, selected
and highlighted pieces remain source geometry. The independent light-frustum
query still includes resident offscreen casters. Its instance plan is cached
until the light matrix/origin/resolution, scene/meshes, selection, explosion or
policy changes. Camera-only color replans can reuse it; the existing GPU depth-map
reuse remains separate. There is no separate global shadow-triangle cap.
The [display part library](part-catalog.md#reviewed-display-detail-library) adds
open-aperture window frames 60594/60592, annular round brick 87081 and verified
parametric aliases; these specialties do not turn into solid body boxes. Its
eight-sided standard studs also reduce the cost of medium and shadow detail.

### Measurements and repeatable profiling

On the accepted **91,265-piece** district readback, the native planner comparison
at the nearer 1280×720 overview reduced color submission from **37,494,790 to
1,979,926 triangles (95%)** versus per-brick LOD using the same updated display
library, and its fixed 2048² light-volume casters from
**130,259,476 to 33,599,614 (74%)**. At three times the camera distance, color
triangles fell from 6,495,792 to 1,330,224. Auto fits its soft target in these cases;
selected/protected source geometry can still exceed it in other views. No pieces
are discarded to force a lower count.
These are submitted triangle counts, not GPU timings or an FPS speedup ratio.

An actual headless Chrome WebGL2 check on an Apple M4 Max/ANGLE Metal renderer,
1440×1000 window at device scale 1, gave **16.7–33.3 ms median presentation
intervals** during warmed district orbit in the budgeted modes. Its different
fitted camera/light submitted about 1.59 million color triangles and estimated
35.3 million brick-shadow triangles. Auto used divisor 4; Performance used 8 and
could not meet its lower target. Their final fitted-frame counts were identical;
interval samples also include changing-camera work and hardware scheduling. This is
a local presentation measurement, not a portable FPS guarantee. Repeated full
High-policy stress caused a WebGL context loss in one benchmark run, so there is
no reliable like-for-like browser FPS ratio against the old policy.

After building the viewer WASM and local server, with Playwright/Chrome installed:

```sh
cargo run --release -p ergo-client --example profile_scene -- construction.ergo.json
cargo run --release -p ergo-client --example profile_scene -- --inventory construction.ergo.json
python3 scripts/profile-rendering.py construction.ergo.json --output target/render-profile.json
```

The native example uses the actual scene planner without a GPU. The browser
script uses disposable local storage and a read-only scene-response fixture,
because combined accepted readbacks can contain prototype aliases that the
public authoring importer intentionally rejects. It runs the actual WASM/WebGL
renderer, primes the selected policies, then drives UI pointer orbit, writes
per-mode PNGs and reports canvas metrics/presentation intervals. It never saves
or publishes the preview. The JSON records the actual graphics backend and GL
errors; `--qualities` selects the policies to exercise.

Canvas diagnostics include `data-color-triangles` (submitted color geometry,
including ground), `data-brick-shadow-triangles-estimate` (resident brick casters,
not necessarily redrawn that frame), `data-detail-scale`, `data-lod-counts`,
`data-shadow-lod-counts` and `data-color-budget-estimate-exceeded`. The last flag
describes the conservative brick estimate, not a GPU timeout or a world limit.

### Relation to Nanite

[Unreal Engine's Nanite](https://dev.epicgames.com/documentation/en-us/unreal-engine/nanite-virtualized-geometry-in-unreal-engine)
builds a hierarchical multiresolution representation of triangle clusters, selects
detail according to projected error, culls visibility on the GPU and streams
compressed geometry pages as needed. That makes the selected work depend on the
visible image rather than submitting every source triangle in a huge scene.

Ergo implements the smaller WebGL2-compatible steps above: shared part prototypes,
resident-octree culling, a scene-wide screen-detail bias, light-space LOD and pass
reuse. It does **not** implement Nanite cluster hierarchies, GPU occlusion culling,
error-bounded arbitrary-mesh simplification or virtualized geometry streaming.
Construction/cell HLOD proxies and finer-grained source-mesh clusters are future
ways to reduce the protected-geometry floor; they would need revision-bound
representations while preserving real geometry for editing and collisions.

### Rendering detail versus loading detail

These tiers reduce drawing work after the region is resident. They do not avoid
reading all region piece records or downloading their source mesh prototypes.
Current shared-region loading still prepares a complete bounded region; nearby
camera-cell cache reuse can skip an unchanged rebuild, but is not coarse-first
streaming.

Construction-level coarse-first loading is a future extension, not an implemented
guarantee: it needs cheap, revision-bound construction/cell summaries or coarse
meshes, followed by full geometry only near the camera or on demand. Such visual
proxies must not be treated as complete editable or collision/physics geometry;
walking into an unloaded area must wait for its real collision data. Lowering GPU
detail alone cannot solve the measured per-record S3 traversal bottleneck.

## Picking: broad phase, then triangle tests

The mouse position is unprojected through the inverse view-projection matrix to
form a world-space ray. A slab intersection finds candidate AABBs. For axis $a$:

$$
t_a^- = \min\left(\frac{b_a^- - o_a}{d_a},\frac{b_a^+ - o_a}{d_a}\right),
\qquad
t_a^+ = \max\left(\frac{b_a^- - o_a}{d_a},\frac{b_a^+ - o_a}{d_a}\right).
$$

Intersect the three intervals. Handle $d_a=0$ explicitly: division by zero at a
box boundary otherwise produces NaNs. Candidates are sorted front-to-back, and
testing stops once the next box cannot beat the closest hit.

For brick transform $A$ and position $\mathbf{p}$, test its shared mesh using:

$$
\mathbf{o}_{local}=A^{-1}(\mathbf{o}-\mathbf{p}),\qquad
\mathbf{d}_{local}=A^{-1}\mathbf{d}.
$$

Do not normalize the transformed direction when comparing the resulting ray
parameter across differently scaled parts. Exact mesh picking can coexist with
approximate box-based physics.

## One geometry, many instances

Repeating the same brick mesh for every brick wastes both transfer bandwidth and
GPU memory. A geometry batch stores vertices once and supplies each instance's
transform, color, and selection state through instanced attributes.

For $N$ bricks, $P$ unique part meshes, $V_p$ vertices in part $p$, and $I$ bytes
per instance:

$$
M_{GPU}\approx\sum_{p=1}^{P}V_p\,s_{vertex}+(N_{color}+N_{shadow})I.
$$

The current layout uses 36 bytes per vertex and 80 bytes per instance per pass.
Color and depth VAOs share the same prototype vertex buffer. A batch uploads its
instance array only when its contents change; orbiting the complete static
homepage scene needs no geometry/instance re-upload. Retained CPU copies allow
these comparisons and are separate from the GPU-byte counter. Draw-call
count follows the number of visible part/LOD batches, aggregates, and ground draws.
This is why increasing the number of brick instances is often cheaper than
increasing the number of unique geometries or materials.

The initial palette is opaque. Adding transparent colors requires ordering or a
dedicated transparency algorithm; simply enabling blending within an arbitrary
instance batch is insufficient.

## Floating origin and numerical precision

Global coordinates are float64 on the CPU. GPU transforms are float32. At a world
coordinate of magnitude $10^9$, float32 no longer resolves single-brick details.
Before uploading positions, subtract a nearby block-aligned origin:

$$
\mathbf{o}_r=B\left\lfloor\frac{\mathbf{c}_{target}}B\right\rfloor,
\qquad
\mathbf{p}_{GPU}=\operatorname{float32}(\mathbf{p}_{world}-\mathbf{o}_r).
$$

The camera uses the same origin. Rebase instance buffers when the target enters
a new origin cell without modifying persisted coordinates.

## Three different caches

- **Persistent browser cache:** immutable CBOR responses, keyed by their
  world-authorized URL containing a SHA-256 object hash. The service worker uses
  a 128 MiB / 512-response budget and insertion-order eviction. This is FIFO,
  not a claimed full LRU implementation.
- **CPU cache:** nearby decoded blocks, deduplicated brick records, shared meshes,
  and the resident octree. A raw CBOR byte count understates decoded memory.
- **GPU cache:** shared mesh buffers and instance buffers for the current scene.
  A prototype is released when neither pass references it, when replaced, or when
  switching worlds. The shadow depth texture/driver overhead is additional memory.

Content-addressed objects never need in-place invalidation. New content gets a
new hash; manifests select a new set of hashes. The current service worker does
not cache sessions, world lists, manifests, or the app shell and does not promise
full offline operation. Logout clears model caches.

Shared-region URLs use a separate, permission-revalidated path: typed CBOR/gzip,
server revision-keyed encoded/persistent read caches, and ordinary HTTP ETags.
They are not added to the service-worker immutable-object cache. The viewer's
separate local packet CacheStorage retains construction/index bytes only for use
after a fresh authorized HTTP 304. Region refreshes
retain unchanged GPU prototypes, and selecting a snapshot-certified fully
resident assembly focuses it without a location/view fetch. See
[loading and caching](loading-and-cache.md) for limits and invalidation.
