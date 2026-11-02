# 128-stud cells and storage migration

The active cell format is **128 studs east × 128 studs north × 128 standard
bricks high**. Native Z units are brick heights: the vertical span is 128 native
units, equivalently **384 plates**. Physical dimensions are 1.024 × 1.024 × 1.2288 m.
Each horizontal cell still covers **one arc-second of latitude/longitude** with
the default geographic reference. Geographic addressing and physical scale are
separate; see [coordinates](ground-and-coordinates.md).

`BLOCK_SIZE=128` is shared by partitioning, spatial predicates, camera rebasing,
unloaded-cell navigation, ground meshes and geographic conversion. `/api/config`
reports `block_size:128` and `block_height_plates:384`. Geometry units and public
east/north/up axes have not changed.

## One active format

Shared metadata requires **`layout:4, cell_size:128`**. Manifests and generated
ground records also carry the current cell size. Reads and edits reject missing
or incompatible markers; there is no running 1024-cell compatibility mode.
Old layouts are decoded only inside the explicit migration tool.

Keeping one arc-second per smaller cell changes GPS offsets for unchanged world
coordinates by a factor of eight. The reference origin stays fixed. Migration
preserves authored poses/dimensions, not their previous derived GPS labels.

## Atomic migration

With the normal storage environment and existing world ID:

```sh
cargo run --release -p ergo-server -- shared-resize-cells WORLD_ID
```

For the local sandbox, include `ERGO_COMMON_WORLD_ID=WORLD_ID` and
`ERGO_WORLD_PREFIX=world`, matching its namespace. Production uses existing S3
settings. No new world or network migration endpoint is created.

The command watches the complete accepted root and reads canonical pieces in
bounded pages with at most 64 payload reads in flight. It recomputes block
replicas/spatial references from the same frozen meshes and poses. Birth IDs,
assembly membership, colours, groups, joints, physical-model records and source
models remain unchanged. Permission prefixes are rescaled exactly: one old cell
becomes an 8×8×8 group covering the same world-unit region.

The directory, replica records, discardable packed projection and layout metadata
publish in **one compare-and-swap commit**. Concurrent accepted edits invalidate
the whole-root watch. New unpacked cells can be packed with `shared-pack-block`.
The operation permits at most one million pieces and 512 MiB of charged metadata
and meshes; excessive per-part replication fails before publication.

Failed staging does not advance the head. An uncertain publication must be
reconciled from accepted history, never blindly retried. Cache-cleanup warnings
after a confirmed commit explicitly say migration already published. Re-running
the command is index-idempotent and retries incompatible-cache cleanup.

Old immutable history remains under normal retention; it is not a runtime
old-grid mode. Tests verify poses, meshes, identities, exact permissions and
historical record preservation.

## Incompatible caches are deleted

The browser uses `ergo-local-data-cell128-v1`, deleting older local databases and
old read-packet CacheStorage. No lazy legacy import remains. The service worker
deletes incompatible model caches and reloads app windows at the format cutover.
Hash verification, permission revalidation, LRU and logout fencing still apply.

Migration deletes old derived read/discovery/ground caches while retaining
current ground fields and raw imagery. Ground loads delete malformed/wrong-size
cache entries. Native construction decoding drops incompatible cached ground
fields while preserving authored geometry. Accepted world storage is migrated,
never deleted as though it were a cache.

Implementation: `crates/world/src/lib.rs`, `crates/server/src/shared/resize.rs`,
`crates/viewer/src/browser_cache.js`, and `web/sw.js`.
