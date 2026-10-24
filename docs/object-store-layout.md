# Object-store folders and the single sandbox

The bucket remains private. The application serves public accepted sandbox data;
making the sandbox viewable never makes credentials, profiles, or the bucket
itself anonymously readable.

## Target organization

```text
s3://ergo/
  _ops/ergo/production.env                 private configuration backup
  ergo/
    world/
      refs/main.cbor                      the single authoritative head
      objects/<sha256>.cbor                immutable trees, pieces, models, history
      maintenance.cbor                    reader/writer/maintenance coordination
    assets/ldraw/source/...               cached LDraw source and provenance
    assets/v2/parts/<part>.json            part-to-mesh cache indexes
    objects/<sha256>.cbor                  meshes referenced by those indexes
    ground/source/...                     cached imagery tiles
    ground/blocks/<signature>/...          derived single-color baseplates
    users/<stable-user-id>.json            private sign-in profiles
    edit-pacing/<actor-hash>.json          edit allowance/reservation records
```

These folders have separate consumers. In particular, **`ergo/objects/` is active**:
`crates/assets/src/lib.rs` resolves `assets/v2/parts/*.json` indexes to mesh hashes
there. It is not safe to delete it merely because world objects moved to
`ergo/world/objects/`. Ground source tiles and generated cells are different cache
layers, also both used. Cached cells outside the current view are reusable data,
not alternative worlds. Profiles and pacing are not construction geometry.

`world/objects` contains mixed CBOR record types by content hash, not a folder per
brick/submodel. The root indexes spatial cells and assembly rules. Immutable
piece births preserve identity/author; accepted audit records carry the stable
actor ID and server-resolved name. Reusable LDraw source models are linked from
assembly headers. The reference/retention tracer must follow those schema links.

## Inventory before consolidation

The complete metadata-only inventory on 2026-10-24 contained **4,174 objects /
3,283,084 bytes**. No credential/profile bodies were downloaded for that inventory.
The report is generated at `target/object-store-inventory-20261024.json`.

- `_ops/ergo/`: one private environment backup; retained.
- `ergo/assets/ldraw/`: 40 cached source records; active.
- `ergo/assets/v2/`: 10 part indexes and `ergo/objects/`: 10 referenced meshes;
  active and retained.
- `ergo/ground/blocks/`: 525 generated cells; `ergo/ground/source/`: 71 source
  tile records. Active caches; retained.
- `ergo/users/`: two profiles. The Google profile is active; the historical
  `development.json` is not a hosted account and is a cleanup candidate.
- `ergo/edit-pacing/`: one active account ledger; retained.
- `ergo/shared/fde184c4-222b-4a16-8a1f-c320da8a2f35/`: 1,820 records for the
  common world, including provisional/unreachable objects. This is the source
  for migration, not a deletion candidate until the new root is verified.
- The other three UUID directories under `ergo/shared/` contain 1,179 records
  for the old private demos. Their constructions have been copied into the common
  world at distinct positions. They are retired after the single-world rollout.
- `ergo/shared/_catalog/`: 195 discovery/index records for multiple worlds. The
  single-world API no longer needs or writes this cross-world discovery catalog.
- `ergo-transaction-tests/`: 48 historical live-test records;
  `ergo-transaction-bench/`: 272 benchmark records. They are test artifacts, not
  application roots, and can be retired once those jobs are stopped.

S3 “folders” are key prefixes, not actual directories. Listing/counting an object
or comparing an ETag is not proof that a transaction object is unreachable.

## Migration and cleanup

Use the library/operator path rather than manually moving a mutable head:

```sh
export WORLD_ID=fde184c4-222b-4a16-8a1f-c320da8a2f35
target/debug/ergo-server shared-migrate "$WORLD_ID" world
```

This acquires the source maintenance barrier, traces application-defined blob
links during GC, copies hash-verified immutable objects, and publishes the target
head **last**. It retains accepted history and stable IDs. Source and target must
be non-overlapping namespaces on the same backend. The source deliberately stays
fenced while configuration switches; interrupted/uncertain migrations do not
silently unlock either side.

Deploy the verified image with `ERGO_COMMON_WORLD_ID=$WORLD_ID` and
`ERGO_WORLD_PREFIX=world` (OpenTofu `common_world_id` / `world_prefix`). Verify the
accepted head, bounded region and construction read-backs. Then:

```sh
ERGO_COMMON_WORLD_ID="$WORLD_ID" ERGO_WORLD_PREFIX=world \
  ERGO_RETIRED_WORLD_IDS='3121f1fe-a5c3-4425-86b0-3a3c8f6ecf2f,9c2b859b-cb91-410c-957f-c37aeb9abe14,18a73d9d-29e0-4ad6-acde-32ba3b7e71b1' \
  target/debug/ergo-server shared-cleanup-legacy "$WORLD_ID"
```

The cleanup refuses to run unless `ergo/world` is active. It names that root
**Ergo Sandbox**, records the stable sandbox ID, and retires old `shared/` roots,
old private snapshot manifests, isolated test/benchmark namespaces, and the
historical development profile. It does not delete caches, the active world,
Google profiles, the pacing ledger, or `_ops`. Neither operation has a public
HTTP endpoint. The operator must stop old test jobs/writers before retiring their
namespaces. CLI reports and the final post-rollout inventory are the evidence of
what was actually removed, not a promise that every cached object is currently
visible on screen.

Retired UUIDs are an explicit operator allowlist, not a wildcard. Preflight
refuses unknown legacy namespaces/private user snapshots, verifies that the old
common root is fenced, and verifies its accepted head is retained in the target.
Newly discovered user data must be inspected/migrated before deletion.

Implementation: `crates/server/src/shared/administration.rs`,
`crates/transactions/src/maintenance.rs`, `crates/assets/src/{lib,ground}.rs`, and
`crates/server/src/shared/retention.rs`. See [transactional storage](transactional-storage.md)
for fencing, uncertain outcomes, historical roots and garbage collection.
