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
`serde_json::Value` tree. The cache retains CBOR and gzip-CBOR; JSON compatibility
responses are generated only when requested, rather than prepared for browser
CBOR requests. Both formats still describe the same accepted scene.

## Revision-safe read caching

Each cache key includes world, accepted revision, centre cell and effective
piece/encoded-read budgets. A 64 MiB byte-budgeted server LRU shares the CBOR/gzip-CBOR
responses among requests; concurrent identical misses share one build.

A discardable gzip-CBOR packet is also saved under the application prefix:
`cache/read-views/v1/<region-and-limits-hash>.cbor`. There is at most one latest
packet for an occupied region/configuration, rather than another large object
for every edit. Empty regions are not persisted. Historical reads do not replace
the persistent packet. Explicit reads pinned to the current revision remain eligible
for persistence; older historical reads do not. Conditional writes resolve concurrent cache-write races
without unconditional overwrites; failed writes leave authoritative reads available.
The exact accepted revision is always checked, even if an older in-flight request
finishes after a newer one.

The stored envelope records format version, complete key/revision, logical read
bytes, decoded size, gzip bytes and their SHA-256 checksum. Cache loading checks
all bounds and decompresses through a 64 MiB limit. Missing, stale, corrupt or
unavailable packets fall back to the accepted tree. Cache output still obeys the
original operation budget; cache hits do not authorize larger fragments.
After verification and typed schema/count checks, the already stored CBOR and
gzip bytes are reused directly. Loading a persistent packet does not recompress
it or regenerate JSON. The district's two retained representations total about
39.9 MB and fit the existing 64 MiB memory budget; retaining every JSON/CBOR/gzip
representation could previously prevent that admission.

The viewer can supply its previous centre as `from_x`, `from_y`, `from_z`. A
one-cell camera move reuses the previous packet only for the exact same accepted
revision and budgets, when all its occupied blocks remain inside the new region
and every newly included cell has no allocated-block directory entry. Otherwise
the normal read runs. Allocated-but-empty cells conservatively prevent reuse.
The hint never authorizes missing geometry or avoids the current permission check.
If no useful hint is supplied, the server probes the 27 allocated-block directory
entries and tries one deterministic nearby center, computed from their coordinate
extrema. The same empty-added-cell / retained-occupied-cell proof must pass before
reuse. Clicking a construction whose first piece crosses a cell boundary can
therefore find an already-warmed packet instead of starting another per-piece walk.

**Current read permission and accepted-history membership are checked before
every cache hit and HTTP 304.** Weak ETags vary with revision, region, format and
limits; `Cache-Control: private, no-cache` requires revalidation. Revoked users
cannot obtain a cached historical response through the API. Read packets never
feed commit validation, permission decisions or authoritative state recovery.

Read-packet caching does not change canonical geometry hashes or accepted history.
These packets can be discarded without losing constructions. The separately
versioned, optional spatial projection below has its own layout-3 writer fence.

## Fewer storage round trips

Layout-3 blocks marked `packed:true` use bulk spatial pages rather than the
per-brick reference walk. Pages contain complete Piece metadata plus mesh CIDs;
their records are deduplicated and charged to the same logical piece/byte budgets.
Each fetched page is at most 1 MiB/1,024 records. Unpacked/legacy blocks retain the
reference path below; existing worlds need an explicit one-time backfill to gain
the packed path. See [spatial packing](persistence-indexing.md#bulk-spatial-reads-and-local-traversal).

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

## Local construction, index and specification caches

The viewer's origin-local IndexedDB **`ergo-local-data-v2`** persists construction
CBOR/index packets and revalidatable discovery/location/part JSON. Its `entries`
store maps normalized request URLs to ETag, SHA-256, byte length and last-use time;
`blobs` stores immutable `Blob` data once per content hash. Identical packets for
several camera cells therefore share one copy rather than duplicating 35 MB each.
Camera-only hints are removed and query parameters sorted; accepted revision and
region remain in geometry keys.

Entries have no age-based expiry: reads update last-use metadata, and pressure
evicts least-recently-used URLs and only then unreferenced blobs. The data budget
is 10% of the reported origin quota, clamped to **64–512 MiB**, at most **64 MiB per
entry / 1,024 URLs**. Active region leases are refreshed every 30 seconds and last
75 seconds, protecting data in active tabs. If protected data leaves insufficient
space, an optional write is skipped rather than evicting active data. These are
logical cache limits; browser storage eviction and actual quota remain browser-owned.
Construction selection makes a best-effort persistent-storage request.

SHA-256 verification uses native asynchronous Web Crypto, rather than hashing
large buffers synchronously in WASM. Corruption invalidates all aliases of the
bad blob and becomes a network miss. Cache writes run **after decoding in the
background**, outside the display completion path; bounded database operations,
quota errors and unavailable storage never keep the loading overlay waiting.
Old `ergo-read-packets-v1` CacheStorage entries migrate lazily after verification:
an old entry is removed only after its replacement commits. No blanket cache or
local-storage reset is required.

Every use still sends a fresh conditional request: bytes are displayed only after
an authorized HTTP 304, never as an offline/error fallback. Read denial evicts that
URL; logout suspends pending writes and clears origin storage. Cache failures do
not prevent ordinary network reads.
The renderer retains its local spatial octree while resident and rebuilds it
from locally cached records after reload, rather than downloading an index again.

### Construction-list metadata

Assembly pages and location hints return weak ETags and `private, no-cache`.
Authorization precedes every 304 or 4 MiB process-local metadata-LRU hit. Page keys
include world, accepted revision, cursor and limit; locations include assembly and
revision. There is no periodic catalogue invalidation for an unchanged snapshot.
Creation-v2 author labels remain frozen creation metadata. Legacy profile-name
fallbacks are best-effort labels captured when that snapshot's page is built;
they can refresh on a new accepted revision or an uncached rebuild, not every minute.
Membership/geometry changes invalidate immediately through the accepted revision.
Cold page rows use bounded ordered parallel reads rather than serial per-model
S3 walks. Live membership is still probed even when a summary count exists, since
individual deletions can leave a rule header behind.
Logical-depth catalogue paging also uses bounded immutable-node lookahead. Legacy
rows without `piece_count` return an unknown size (`—` in the viewer), rather than
walking up to 5,000 member keys just to populate a navigation label. A normal
assembly edit records a size summary. Legacy current-profile names are fetched
once per distinct actor within a page request, not once per construction.

The API also persists catalogue JSON in sixteen hash-selected discardable object
slots per world. Each page is at most 512 KiB and its JSON envelope at most
1,052,672 bytes; slot collisions merely miss. Loads verify format, ETag, accepted
revision, body revision and checksum after authorization. Optional cache reads
are bounded to 750 ms and writes to one second; malformed, oversized or unavailable
cache data falls back to ordinary discovery. Current revision checks never trust
an older slot. This survives process restarts without retaining one object for
every historical revision/query; a genuinely uncached page still needs its first
index traversal and becomes reusable after successful persistence.
`X-Ergo-Metadata-Cache` distinguishes `miss`, `memory`, `persistent` and `revalidated`.

The browser starts list loading as soon as configuration is available, in parallel
with account/head setup, and retains verified list/location bytes in IndexedDB.
Cached JSON is revalidated before use. A corrupt typed cached response gets one
unconditional **read** recovery; no mutation retry is introduced.

Brick descriptions and decoded meshes have a shared process-local 64 MiB LRU with
60-second description revalidation. Only sourced assets with descriptions are
admitted; temporary proxy/failure responses are not. Public part HTTP responses
use weak description/content-version ETags and a 60-second browser cache lifetime;
uncached fallbacks use `no-store`. Native filesystem part caches remain supported.
These caches never replace server-owned physical normalization or validation.

Complete authoring reads retain canonical member/identity and byte-budget checks.
Their conservative external-dependency guard stops after its first verified
external spatial witness instead of materializing every nearby building inside a
street assembly's broad bounds. A negative result still traverses the full query
and checks remote explicit dependencies; edit validation is unchanged.

## Observe and measure

### Loading overlay

The browser shows discovery, brick reading, construction checks, download,
decoding and display preparation, with elapsed time. Only known-total stages
show a percentage. Discovery shows the bricks found and cells checked with an
indeterminate bar; cell counts are not presented as a time estimate. Percentages
refer to the named stage, not an estimated overall completion time.

Progress polls are optional, at most one outstanding every two seconds, and pinned
to the same accepted revision as the read. Generation/request guards ignore stale
responses, and delayed server status cannot replace download/display stages.
Refreshes keep existing geometry usable. Failed polling does not abort or retry
the main read. Errors and completed loads remove the progress panel. Reduced motion
and native accessible progress semantics are retained.
Read-only navigation can be superseded by another construction selection: the
previous scene remains visible while a replacement loads, old fetches are aborted,
and generation/request guards reject late head/location/body callbacks. Authoring
mutations remain exclusive. Each discovery GET attempt has a 15-second deadline;
transport interruptions, HTTP 408 and 5xx responses get one idempotent retry after
250 ms. Permission and protocol failures do not retry. Region reads retain a
180-second deadline; a failed region is not immediately retried in a loop.
Select it again or choose another construction to recover without reloading the app.
Transient catalogue-refresh failures keep already loaded rows and show a scoped
retry status. Automatic list retries back off from 5 to 60 seconds; a manual
refresh supersedes an older scheduled retry. Background revision polling backs off
to at most 60 seconds and retains the current scene without a global timeout
alert; a successful poll reports recovery. These are GET-only recovery paths,
not mutation retries or permission-free/offline cache reuse.

### Operator warm-up

Source-built servers after v0.5.11 provide a storage-operator command for a first
dense materialization that would exceed the hosting proxy's request deadline:

```sh
ERGO_SHARED_VIEW_PIECES=100000 cargo run --release -p ergo-server -- \
  shared-warm-view "$ERGO_COMMON_WORLD_ID" 19 0 0
```

Use the existing application S3 credentials, prefix and common-world settings.
This has no public HTTP mutation route. It runs the same bounded region builder
against the actual accepted S3 snapshot, holds its ordinary reader pin, and writes
only the discardable cache packet. It does not import local construction JSON,
advance the world head, change identities, or bypass publication validation.
New packets are not persisted if the head changes during materialization.
Conditional persistence is confirmed by reading back the verified packet; API
readers still check current permission, accepted revision and budgets before use.
The report separates materialization time, logical bytes and compressed size.
Warm-up does not make cold reconstruction intrinsically fast or raise read limits.

### HTTP diagnostics

The same authorized view URL with `status=true` returns the accepted revision and
an ephemeral `phase` and optional structured `progress` containing `stage`,
`completed`, `total`, and `found_pieces` (null when not relevant). It is
`private, no-store`, exposes no geometry, and does not start a build. Progress is
removed when the request completes, fails or is cancelled.

`X-Ergo-Read-Cache` reports `miss`, `memory`, `persistent`, `neighbor` or `revalidated`;
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
Rebuilt/persistent responses also report `view_cache`, `view_discovering`,
`view_pieces`, `view_membership`, and `view_encoding` durations when those stages
run. The profiling script separates time to headers, body transfer and server
phase durations. These do not measure GPU frame time.

The 2026-10-28 district check covered 91,265 pieces in 15 assemblies. A direct
storage-operator cold materialization took 1,866.98 seconds; this is still slow.
Its packet contained 39,946,154 logical read bytes and 35,688,077 CBOR bytes,
compressed to 4,196,490 bytes. The warmed browser request took 4.247 seconds in
that run; a separate HTTP/decode/exact pose-colour-part verification took 8.053
seconds. Every expected assembly was fully resident, and the live UI displayed
all 91,265 pieces without console errors. These are scenario measurements, not
a general latency guarantee. Later edits can invalidate the revision-keyed cache.

The cold log's ten-second samples put spatial discovery at roughly 1,200 seconds
(64% of the run) and brick/mesh reading at roughly 660 seconds (35%). The remaining
last-record/membership/encoding/persistence tail was only several seconds; that
sampling cannot give exact substage durations. A separate cached request measured
7.279 seconds to headers, 0.245 seconds reading the 4.2 MB body, and 6.962 seconds
of reported server work. This identifies storage traversal for cold reads and
avoidable decode/re-encoding for warm reads, not transfer or distant drawing, as
the dominant costs. The CBOR-cache and neighboring-view changes address warm
costs; cache-only changes do not eliminate the cold canonical-reference walk.
The packed-page path described above addresses that walk for backfilled blocks.

On 2026-10-29 the v0.5.12 existing-browser check preserved model storage and
verified/migrated all five legacy packet entries. Identical content shared blobs
in the new database; eight retained URL entries referenced five unique blobs
(72,014,659 bytes at that observation, including historical revisions). Current
application assets were fetched explicitly so the check exercised the new cache.
The 30-row construction list appeared at 3.135 seconds including startup; its
initial list HTTP request took 1.861 seconds. Under concurrent authoring, a later
cold metadata refresh took 8.840 seconds and its immediately repeated authorized
304 refresh took 77 ms. The 91,269-piece region after the first doorway edit
transferred 4,201,683 wire bytes in 17.075 seconds during that workload, completed
its overlay and displayed without browser errors. These distinguish cold work,
authorization revalidation and rendering; they are not latency guarantees.

The v0.5.13 live follow-up reproduced a 15-second catalogue abort without active
geometry edits. Recovery eventually populated all 30 rows without an alert, but
the first cold browser load still took 87.557 seconds; a subsequent different
page-limit cache miss took 916 ms. This motivated revision-only catalogue keys
and persistent catalogue packets, rather than treating retries as a speed fix.

For v0.5.14, building the uncached catalogue through a production-backed loopback
worker took 304.443 seconds. After that worker was restarted with empty process
caches, the same 30-item page loaded from its verified persistent packet in
869 ms. The newly deployed public API's first catalogue request reported
`X-Ergo-Metadata-Cache: persistent` and took 118 ms. A fresh browser load fetched
current application assets, populated all 30 rows at 2.223 seconds including
startup, and showed no global alert or browser error. All these checks used
accepted revision `53ef52c364aed87a0a974722ab1dc2db7ddf218b5fdfedb27f28c031b18ff8c8`
without changing geometry. First traversal cost is not eliminated for an actually
uncached revision; persistence and permission/version revalidation avoid repeating
it just because a process restarted or a minute elapsed.
After the district edits and bridge names advanced the world to
`2c6d3d3a102ae9294bd86f6ad7f7e3387516f6f116ed4c93913c24b2c450eb31`,
a live list refresh returned HTTP 200 from the memory cache in 79 ms, displayed
both new bridge names and all 30 global rows, and showed no alert. A preceding
unchanged-revision refresh returned an authorized HTTP 304 in 112 ms across the
former minute-bucket boundary.

Source: `crates/server/src/shared/{read_cache,view}.rs`,
`metadata_cache.rs`, `authoring.rs`, `crates/transactions/src/lib.rs`, and
`crates/viewer/src/{common,app,read_cache}.rs` / `browser_cache.js`.
Increment the read-packet format/key version when changing cache semantics or
incompatible transport normalization.
