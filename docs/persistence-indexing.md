# Spatial persistence indexing

The authoritative world is an immutable, content-addressed, packed octree selected
by one conditional head. Its address space and derived rendering index serve
different purposes; neither is a mechanical attachment graph.

## Coordinates and cells

Public axes are X east, Y north, Z up. A storage cell has side B=1024 construction
units. For a position p, each cell index is `floor(p/B)`, including negative values.
One default horizontal cell maps to one arc-second; geographic origin/resolution
are stored in Metadata. This address mapping does not determine physical length.

Nominal physics uses 8 mm horizontal pitch and 9.6 mm vertical brick height.
The global floor is Z=0 independently of vertical cell boundaries. See
[ground and coordinates](ground-and-coordinates.md).

## Persisted hierarchy

Signed i64 cell coordinates are sign-biased and interleaved into 64 octants, with
`q=x_bit+2*y_bit+4*z_bit`. The application adds typed leading octants for policy,
spatial rows, metadata, pieces, assembly rules, allocated-cell directory and joints.
Full hash identities are padded/interleaved into 86 octants.

Unary unfenced runs use Extension records; fenced Branches remain explicit. A leaf
points to an immutable typed payload. These logical paths are inside the root,
not separate S3 directories. See [wire formats](object-store-formats.md).

## Boundary-crossing objects

Render bounds, including details, determine intersected storage cells. A piece
keeps one stable Birth identity and separately versioned pose/mesh state. Cells
contain reference-only SpatialRef rows. Replicas are deduplicated before rendering;
the entire part remains available across a boundary. Replication is bounded to
64 cells per piece.

Within a cell, adaptive reference nodes retain large split-crossing extents near
their parent. Child `0` holds own references; `1/q` descends by octant, up to ten
local levels. An allocated-cell directory supports sparse bounded enumeration.

## Bulk spatial reads and local traversal

Spatial layout 3 adds a complete read projection per packed block, alongside the
unchanged edit/contact index. Up to 256 bounded pages hold full brick metadata
and shared-mesh references. A region reader bulk-downloads pages, deduplicates
boundary replicas, then builds/traverses the resident octree locally. It avoids
one path lookup and payload GET for every brick. Page bytes and index nodes reuse
the server's verified 256 MiB object LRU by immutable CID.

Only touched hash shards are read/updated during an edit. The projection and
canonical records publish in the same head CAS; colour/mesh changes, moves and
deletions cannot leave a separately published stale page. Mechanical discovery
and validation still use canonical pieces and their original spatial predicates.
Projection updates have a separate bounded 64 MiB allowance for loaded pages and
added record bytes; overflow
falls back to the reference index rather than rejecting otherwise valid geometry.

New worlds use layout 3. Layout 2 remains readable/writable without packed pages.
`shared-pack-block WORLD_ID X Y Z` performs an explicit one-block backfill and
promotes metadata to layout 3 under a whole-root optimistic watch. It changes no
Piece payload, birth, pose, geographic origin or physical model. Older layout-2-only
writers reject layout 3, preventing writes that would omit page maintenance.
Run only page-aware servers/operators after promotion; backfill refuses concurrent
world changes. Untouched legacy blocks still use the normal fallback reader.

The 4,096-brick cold-store test observed 574 GETs for packed reads versus 25,263
for the reference/per-piece path, with identical scene geometry. This measures
request-count reduction in an isolated store, not a production latency guarantee.

## Rules and dependencies

Assembly headers link native model descriptions and mesh blobs. Membership,
adjacency, endpoint joints, anchors and groups are paged; ordinary geometry edits
load the relevant rows instead of a world-sized graph. Explicit global joints
link assemblies. The server follows old/new contact/constraint/group closures and
swept spatial predicates; oversized domains are rejected, not truncated.

## Publication and residency

Immutable payloads/nodes and audit metadata stage before head CAS. Accepted
history is indexed by CID/sequence, and each Commit has a materialized root.
World GC traces schema links under a durable maintenance barrier.

The browser separately selects bounded nearby cells, decodes/deduplicates records,
builds a resident octree and plans part/LOD batches. Camera eviction never defines
a complete physics domain. Native asset, terrain and account caches have separate
namespaces and retention rules.

Source: `crates/transactions/src/{path,tree,lib}.rs`,
`crates/server/src/shared/{address,spatial,bulk,paged,working}.rs`.
