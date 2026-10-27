# Object-store organization

The private Scaleway bucket `ergo` contains one authoritative shared world,
native asset/terrain caches, private account context and operational backups.
The application serves authorized/public data; the bucket itself is not public.

```text
s3://ergo/
  _ops/ergo/production.env
  _ops/ergo/migrations/<migration-id>.json
  ergo/
    world/
      refs/main.cbor
      maintenance.cbor
      objects/<sha256>.cbor
    assets/parts/<part-id>.json
    objects/<mesh-sha256>.cbor
    ground/rasters/<provider-hash>/<z>/<x>/<y>.cbor
    ground/blocks/<signature>/<east>_<north>.cbor
    cache/read-views/v1/<region-and-limits-hash>.cbor
    users/<user-id>.json
    user-positions/<actor-hash>.json
    edit-pacing/<actor-hash>.json
    account-plans/<actor-hash>.json
```

S3 folders are key prefixes, not directories. `S3_PREFIX=ergo` supplies the
application prefix; `ERGO_WORLD_PREFIX=world` supplies the world repository.
`ERGO_COMMON_WORLD_ID` identifies the one Sandbox, independently of its prefix.

## Authority and retention

`world/refs/main.cbor` is the conditional-write acceptance point. Its commit and
typed tree/blob references select accepted state and retained history. Uploading
an immutable object does not publish a construction. Use the transaction library
for head changes and coordinated, schema-aware collection for object deletion.

World objects include piece births/current states, reusable native model
definitions, geometry, paged rules/spatial references, policy, frozen physical
model and audit records. There is no folder per brick and no global piece-count
object. Hashes name content; logical octree paths index records inside the root.

## Assets and ground

`assets/parts` contains native Ergo descriptions, referring to cached geometry in
**`ergo/objects`**. This geometry namespace is active and separate from
**`ergo/world/objects`**. A world retains its authoritative mesh blobs independently
of the reusable cache. Source imports are converted before persistence; source
text only occupies a bounded transient import cache.

Ground source tiles and derived single-color baseplates are separate cache
layers. Cached cells are not constructions, alternative worlds or physical anchors.
World GC does not manage asset, terrain or account namespaces.

## Derived shared-region packets

`cache/read-views/v1` stores discardable gzip-CBOR packets for occupied browser
regions. Exact revision/limits and current permission checks precede every hit.
One region/configuration slot is reused across edits; old packets are never world
GC roots or authoring input. World GC does not manage this separate namespace.
See [loading and caching](loading-and-cache.md).

## Private account context

Profiles support sign-in and display-name attribution. A name copied into a public
birth/audit is public attribution; the profile/email remains private. Last camera
positions are private per-user records, exposed only to that authenticated user
and their delegated MCP. Pacing and unlimited plans are independent account state.

`_ops/ergo/production.env` is a private dotenv recovery copy and may contain
secrets. Migration receipts contain verification/count/hash metadata, not credential
or profile bodies. Neither is a construction or part of world reachability GC.

See [file formats](object-store-formats.md), [transactional storage](transactional-storage.md)
and [operations](operations.md) for record fields, maintenance and migration.
