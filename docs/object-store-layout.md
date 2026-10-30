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
    read-cache/metadata-v1/<world-id>/<00..15>.json
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

`read-cache/metadata-v1` contains sixteen bounded UTF-8 JSON catalogue-cache slots
per world. Each wraps revision/ETag/checksum and an encoded JSON response string.
These are separate from gzip-CBOR geometry packets and from authoritative state.

## Canonical resources and reusable models

The [part library](part-library.md) defines canonical part IDs and immutable
minifigure/car construct IDs. Its checksum-verified native geometry pack is shipped
with the application. Requested assets populate `assets/parts` and `objects` once
per canonical resource; models use references, transforms and colours rather than
repeating mesh vertices or common component definitions for each copy.

Accepted source models in `world/objects` are deterministic CBOR with typed
part/local-definition/common-construct references. Current Piece records retain
independent brick identity and pose; rules map canonical IDs to immutable mesh CIDs.
The [format reference](object-store-formats.md#resource-references-parts-constructs-and-occurrences)
specifies encodings, hash inputs, gzip envelopes and the distinction between
catalogue resources, common models and placed occurrences.

## Additional development/compatibility namespaces

Code also supports `worlds/<owner>/<uuid>/manifest.json` and
`_private_catalog/<owner-hash>/{refs,objects,...}` for private/development snapshots.
The production common-world configuration disables new private snapshots. When
multiple shared worlds are enabled without a common-world override, their
repositories live under `shared/<uuid>` and derived discovery repositories under
`shared/_catalog/{users/<actor-hash>,geo-directory/<slot>,geo/<quad>}`. Each discovery
repository uses ordinary transaction heads and deterministic CBOR objects; its
advertisements are verified against actual policy/data before display. These are
supported optional layouts, not extra authoritative worlds in the deployed Sandbox.

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
