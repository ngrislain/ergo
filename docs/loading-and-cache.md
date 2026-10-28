# Shared construction loading and compressed read packets

The shared browser region is a read-only view, not the whole world or a certified
mechanical domain. Authoritative geometry/history remains in the transactional
world namespace. Loading optimizations do not change publication validation.

## Dense-region budgets

The 3×3×3-cell region has independent read-only budgets: by default 50,000 pieces
and 64 MiB of charged CBOR reads. `ERGO_SHARED_VIEW_PIECES` accepts 1–100,000 and
`ERGO_SHARED_VIEW_BYTES` accepts 1–67,108,864. Invalid settings fail startup.
Regions exceeding the configured budgets return HTTP 413 rather than silently
losing geometry or claiming complete visual membership for a truncated scene. The response/cache's decoded
CBOR packet also retains its separate 64 MiB bound.

These settings do not enlarge ordinary authoring budgets or the 5,000-body
dynamic simulation limit. Both read budgets are part of cache keys, so lowering a
budget cannot reuse a previously cached oversized region. Large responses still
need more decoded client memory; these are operation limits, not process heap quotas.

## Resident navigation and GPU reuse

The server includes `fully_resident_assemblies` in each region response. New
headers provide exact membership counts; legacy headers inspect membership pages
only until completeness is established or a non-resident member is found.
This is visual membership, not mechanical-dependency completeness.

Clicking an assembly whose complete geometry is resident focuses its existing
bounds immediately. It does not fetch its location or reload the region. Unknown
or partial membership falls back to the normal server-location path. Region
refreshes retain unchanged LOD/GPU prototypes; changed/removed meshes are still
invalidated by `Renderer::set_meshes`. The existing five-second head polling
continues to detect accepted changes.

## Compact negotiated transport

`GET /api/shared-worlds/{id}/view?...&format=cbor` returns a typed
`application/cbor` response; omitted/`json` format retains JSON compatibility.
The browser requests CBOR and can fall back to JSON during a rolling deployment.
Gzip is negotiated through `Accept-Encoding`; explicit `gzip;q=0` disables the
precompressed response. Both representations describe the same accepted scene.

Gzip was already enabled for HTTP before this optimization. CBOR additionally
reduces expanded numeric geometry and avoids the shared route's intermediate
`serde_json::Value` tree. JSON and CBOR, compressed and identity representations
are encoded once for a cache entry, rather than rebuilt per visit.

## Revision-safe read caching

Each cache key includes world, accepted revision, centre cell and effective
piece/encoded-read budgets. A 64 MiB byte-budgeted server LRU shares the encoded
responses among requests; concurrent identical misses share one build.

A discardable gzip-CBOR packet is also saved under the application prefix:
`cache/read-views/v1/<region-and-limits-hash>.cbor`. There is at most one latest
packet for an occupied region/configuration, rather than another large object
for every edit. Empty regions are not persisted. Historical reads do not replace
the persistent packet. Conditional writes resolve concurrent cache-write races
without unconditional overwrites; failed writes leave authoritative reads available.
The exact accepted revision is always checked, even if an older in-flight request
finishes after a newer one.

The stored envelope records format version, complete key/revision, logical read
bytes, decoded size, gzip bytes and their SHA-256 checksum. Cache loading checks
all bounds and decompresses through a 64 MiB limit. Missing, stale, corrupt or
unavailable packets fall back to the accepted tree. Cache output still obeys the
original operation budget; cache hits do not authorize larger fragments.

**Current read permission and accepted-history membership are checked before
every cache hit and HTTP 304.** Weak ETags vary with revision, region, format and
limits; `Cache-Control: private, no-cache` requires revalidation. Revoked users
cannot obtain a cached historical response through the API. Read packets never
feed commit validation, permission decisions or authoritative state recovery.

The canonical world layout and existing object hashes/history are unchanged.
These read packets can be discarded without losing constructions.

## Fewer storage round trips

Visual enumeration decodes the complete piece birth ID from each validated
layout-2 spatial key, without downloading its unused bounding-box payload.
Piece reads use ordered batches with at most 64 reads in flight.
Immutable-object reads sharing a namespace/key coalesce into one
verified download. Read-only spatial pages use four-level node lookahead with at
most 64 node GETs in flight and a 64 KiB speculative-node limit. Internal spatial
pages contain up to 512 rows. Speculation
retains validated decoded nodes for the page, avoiding repeated lookahead decoding.
It does not fetch application values or change page order/cursors; speculative
failures fall back to normal required reads. Authoring spatial and rule pages use
the same node-only lookahead while preserving staged-node reads and whole-prefix
transaction predicates. Acknowledged immutable uploads populate the server's 256 MiB
verified-object LRU, avoiding a download immediately after writing the bytes.
Mutable heads and explicit uncached CAS verification remain uncached. Uploads
are never skipped based on an LRU hit, preserving GC/orphan safety.
Required authoring rule rows and spatial-reference payloads are also loaded in
ordered batches of at most 64 reads within their existing 128-row pages. Every
record is still charged once to the working budget, and spatial predicates and
complete publication dependency discovery remain unchanged.

The encoded read budget is enforced on the final operation, including the
additional membership headers. It is not a complete allocator-RSS bound; bounded
parallel readers, decoding and response encodings also consume memory.
Transport CBOR is serialized directly from the typed packet to avoid a second
complete generic value tree at the dense-view encoding peak. It is ordinary CBOR,
not a new canonical world-record encoding; authoritative record hashes and cache
key canonicalization are unchanged.

## Observe and measure

The same authorized view URL with `status=true` returns the accepted revision and
an ephemeral build phase (or null when no matching build is active). It is
`private, no-store`, exposes no geometry, and does not start a build. Progress is
removed when the request completes, fails or is cancelled.

`X-Ergo-Read-Cache` reports `miss`, `memory`, `persistent` or `revalidated`;
`Server-Timing: view;dur=...` reports server-side request duration. Use:

```sh
python3 scripts/profile-shared-loading.py --cell 19 0 0 \
  --output target/loading-measurements.json
```

It compares identity JSON, gzip JSON, gzip CBOR, a repeated read and conditional
revalidation. It prints timing/size/header summaries, not private geometry.
Measurements depend on server warmth, network and construction. Transfer timing
does not measure WebGL frame time; resident-focus tests separately verify that
cached navigation issues no location/region request.

Source: `crates/server/src/shared/{read_cache,view}.rs`,
`crates/transactions/src/lib.rs`, and `crates/viewer/src/{common,app}.rs`.
Increment the read-packet format/key version when changing cache semantics or
incompatible transport normalization.
