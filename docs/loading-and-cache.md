# Shared construction loading and compressed read packets

The shared browser region is a read-only view, not the whole world or a certified
mechanical domain. Authoritative geometry/history remains in the transactional
world namespace. Loading optimizations do not change publication validation.

## Dense-region budgets

The read-only region accepts a radius of 1–32 storage cells (default 1, or 3×3×3).
Wider reads enumerate the sparse allocated-cell directory with bounds of 4,096
selected allocated cells and 16,384 visited directory rows, rather than scanning
an enormous cube of empty cells. It has independent budgets: by default 50,000 pieces
and 64 MiB of charged CBOR reads. `ERGO_SHARED_VIEW_PIECES` accepts 1–100,000 and
`ERGO_SHARED_VIEW_BYTES` accepts 1–67,108,864. Invalid settings fail startup.
Regions exceeding the configured budgets return HTTP 413 rather than silently
losing geometry or claiming complete visual membership for a truncated scene. The response/cache's decoded
CBOR packet also retains its separate 64 MiB bound.

The navigator measures reach in world units: it requests
`ceil((fog_distance + 384) / 128)` cells, clamped to 5–32. At the default 2,200-stud
distance this is radius 21, not a fixed 384-stud-wide region. The complete accepted
response is retained until a replacement arrives. Read-only HTTP 413 responses
can reduce the requested radius by half, down to 5; the visible fog cutoff is
then bounded to `radius × 128 − 384`, retaining the prefetch margin. Failure at
the minimum remains an explicit error. Authoring writes are never retried by this
read fallback. Known-empty cells inside the confirmed region do not act as false
navigation barriers; cells outside confirmed coverage still block movement.

These settings do not enlarge ordinary authoring budgets or the 5,000-body
dynamic simulation limit. Both read budgets are part of cache keys, so lowering a
budget cannot reuse a previously cached oversized region. Large responses still
need more decoded client memory; these are operation limits, not process heap quotas.

## Resident navigation and GPU reuse

The server includes `fully_resident_assemblies` in each region response. New
headers provide exact membership counts; legacy headers inspect membership pages
only until completeness is established or a non-resident member is found.
This is visual membership, not mechanical-dependency completeness.

Initial location-based framing uses complete resident assemblies intersecting
the requested local area, rather than distant models loaded only for background
and caster coverage. Clicking an assembly whose complete geometry is resident focuses its
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

Each format-3 cache key includes world, accepted revision, centre cell, radius and effective
piece/encoded-read budgets. A 64 MiB byte-budgeted server LRU shares the CBOR/gzip-CBOR
responses among requests; concurrent identical misses share one build.

A discardable gzip-CBOR packet is also saved under the application prefix:
`cache/read-views/cell128-v1/<region-and-limits-hash>.cbor`. There is at most one latest
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

A client can supply its previous centre as `from_x`, `from_y`, `from_z`. A
nearby camera move reuses the previous packet only for the exact same accepted
revision, radius and budgets, when all its occupied blocks remain inside the new region
and every newly included cell has no allocated-block directory entry. Otherwise
the normal read runs. Allocated-but-empty cells conservatively prevent reuse.
The hint never authorizes missing geometry or avoids the current permission check.
Wider views prove newly added space empty through the sparse allocated-cell
directory, avoiding another per-piece materialization merely because the camera
crossed a small storage cell. Proof work retains the read-byte bound.
For radius-1 views without a useful hint, the server probes the 27 allocated-block directory
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
versioned spatial projection below runs only in the current layout-4 cell format.

## Fewer storage round trips

Current layout-4 blocks marked `packed:true` use bulk spatial pages rather than the
per-brick reference walk. Pages contain complete Piece metadata plus mesh CIDs;
their records are deduplicated and charged to the same logical piece/byte budgets.
Each fetched page is at most 1 MiB/1,024 records. Current unpacked cells use the
reference path below and can be explicitly packed. Old cell layouts are migrated,
not served through a compatibility reader. See [spatial packing](persistence-indexing.md#bulk-spatial-reads-and-local-traversal).

For existing worlds, `shared-pack-world WORLD_ID [--check]` inventories/backfills
all allocated cells, verifies unchanged canonical Merkle subtrees and advances the
root only for newly packed cells. See the [whole-world runbook](storage-operations.md#whole-world-packing).
This reduces small storage requests rather than merely increasing concurrency.
It accelerates browser spatial reads; native authoring dependency discovery still
uses the canonical contact/rule indexes and is a separate cost.

### Measured whole-world backfill

For a production radius-21 browser region centred at `[156,0,0]` (73,660 pieces),
the pre-backfill read did not return response headers within a bounded 180-second
observation. This is a censored observation, not a measured complete load time.
After [all allocated cells were packed](storage-operations.md#verified-common-world-backfill),
the first server-cache-miss request took 29.400 seconds: 29.184 seconds until
headers, then 0.216 seconds to transfer 3,564,673 gzip-CBOR bytes. Server timing
reported 15.358 seconds of discovery, 8.586 seconds of piece materialization,
1.874 seconds of membership checks and 2.539 seconds of encoding. These phases
include their storage waits and CPU work; they do not isolate an exact per-GET
latency percentage.

An immediate server-memory-cache response took 0.391 seconds for the same bytes;
an authorized unchanged ETag check took 0.120 seconds and returned zero geometry
bytes. The persistent packet was independently verified (30,253,154 decoded CBOR
bytes) without a world commit. This shows that response-body volume was not the
dominant cost in this run. It is a single post-migration server-side/transfer
measurement, not a guarantee or a measurement of browser decode/GPU time.

### Remaining reference-path reads

Visual enumeration decodes the complete piece birth ID from each validated
unpacked spatial key, without downloading its unused bounding-box payload.
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

Large imports also warm the immutable index paths for their newly generated birth
IDs before the ordinary identity-collision checks. This follows only requested
paths in the transaction's pinned read snapshot, with at most 64 concurrent traversals and a
64 KiB speculative-node limit. No application payloads or mutable heads are
prefetched, no predicates are registered by speculation, and failed speculation
falls back to the required reads. The unchanged `piece`/`put` checks still record
identity dependencies and reject collisions. Pending edits remain authoritative.
This avoids serial cold S3 node lookups across thousands of random birth IDs;
it does not scan the world's geometry or raise admission budgets.

Operator logs identify import preparation, identity-path prefetch, staging counts,
validation and publication, with elapsed seconds. These are progress diagnostics,
not accepted receipts or permission to replay an uncertain mutation.
Required authoring rule rows and spatial-reference payloads are also loaded in
ordered batches of at most 64 reads within their existing 128-row pages. Every
record is still charged once to the working budget, and spatial predicates and
complete publication dependency discovery remain unchanged.

Assembly replacements also warm the selected accepted piece records through a
pinned read snapshot before the ordinary identity-preserving edit. The complete
dependency walk deduplicates its pending frontier and processes waves of at most
64 IDs. Each wave, and each full spatial-neighbour page, can warm the requested
immutable piece records before required reads. First-witness guards retain their
early return rather than warming every candidate. Speculative nodes and records
are individually limited to 64 KiB; record completions stop at the supplied
remaining-byte threshold, with at most 64 concurrent traversals. This threshold
is not a strict aggregate transfer or heap bound for in-flight node/value reads.
Speculation does not register dependencies, authorize an edit or replace pending
values. Required reads still enforce all byte charges, absent/present predicates,
permissions, external-joint closure and spatial-conflict protection. Failures of
optional warming are cache misses, not permission to skip required records.
Whole-assembly replacements share the accepted rule records rather than cloning
them, and retain old piece identities rather than a second decoded piece inventory.
They discover outside neighbours with one combined-bounds query, without first
warming every member's spatial-reference path. This removes redundant speculative
traversal; required membership, permissions and old/new dependency checks remain.
Ordinary discovery waves with at least
16 planned IDs can look
ahead at up to 2,048 spatial paths through six cell-subdivision levels, skipping
individual bounds spanning more than eight cells. First-page lookahead examines
at most 256 independent prefixes / 4,096 rows with 64 concurrent reads and a
five-second index deadline, then optionally warms bounded referenced values.
Returned rows are hints only: their birth IDs can warm canonical piece lookups,
but they never become observed membership or a validation domain. The ordinary
queries still record their precise predicates, inspect pending edits, verify
spatial references and charge every required value. No whole-block predicate is
substituted, and large/custom bounds fall back to the unchanged required query.
Replacement logs identify preparation, old dependency discovery, staging,
checking, publication and acceptance; periodic closure counts are diagnostics,
not a receipt. A timeout must be reconciled before another mutation.
The read-only `shared-profile-dependencies` operator command additionally reports
phase timings on a pinned snapshot, with a bounded deadline and no staged edit;
see [the operations workflow](operations.md).

The encoded read budget is enforced on the final operation, including the
additional membership headers. It is not a complete allocator-RSS bound; bounded
parallel readers, decoding and response encodings also consume memory.
Each transaction/snapshot tree now keeps a separate, FIFO-evicted decoded-read
node cache with an 8 MiB accounted budget; walking further pages no longer retains
every visited decoded index node until the operation ends. The shared verified
encoded-object LRU remains separate. Staged nodes, payloads and transaction
predicates are not evicted by this cache: they still survive until publication or
discard, so this is not a constant-memory guarantee for a whole edit.
Canonical encoding in `crates/transactions/src/encoding.rs` writes array elements
incrementally rather than building a second `ciborium::Value` tree. Maps buffer
encoded key/value bytes for deterministic sorting and duplicate-key checks.
The resulting CBOR bytes, normalized floats and content IDs are unchanged.
The HTTP request still materializes. Shared-native geometry validation reads
normalized disk-backed bodies and bounded candidate pages; it no longer constructs
a complete affected pose vector. Stability
discovers independent contact/joint islands and checks one at a time, constructing
contacts and solver matrices only for the current island; the compression path
keeps an active pending-load frontier rather than every body's accumulated load.
Global joint lists, scalar component/membership indexes and the current complete
island's body/contact inputs remain resident and linear in their domain; one
connected island can still be large. See [native input storage](native-working-state.md#streamed-physical-input)
and [the static certificate](rigid-body-physics.md#lightweight-static-certificate).
This does not implement streaming HTTP uploads, fully out-of-core validation or partial acceptance.
Transport CBOR is serialized directly from the typed packet to avoid a second
complete generic value tree at the dense-view encoding peak. It is ordinary CBOR,
not a new canonical world-record encoding; authoritative record hashes and cache
key canonicalization are unchanged.

## Local construction, index and specification caches

The viewer's origin-local IndexedDB **`ergo-local-data-cell128-v1`** persists construction
CBOR/index packets and revalidatable discovery/location/part JSON. Its `entries`
store maps normalized request URLs to ETag, SHA-256, byte length and last-use time;
`blobs` stores immutable `Blob` data once per content hash. Identical packets for
several camera cells therefore share one copy rather than duplicating 35 MB each.
Camera-only hints are removed and query parameters sorted; accepted revision and
region remain in geometry keys.

Entries have no age-based expiry: reads update last-use metadata, and pressure
evicts least-recently-used URLs and only then unreferenced blobs. The data budget
is 20% of the reported origin quota, clamped to **128 MiB–2 GiB**, at most **64 MiB per
entry / 4,096 URLs**. This is a disk-data budget, not a preallocated WASM/GPU buffer.
The lower bound provides room for a maximum-sized valid region plus the basic
library. Active region leases are refreshed every 30 seconds and last
75 seconds, protecting data in active tabs. If protected data leaves insufficient
space, an optional write is skipped rather than evicting active data. These are
logical cache limits; browser storage eviction and actual quota remain browser-owned.
Construction selection makes a best-effort persistent-storage request.

SHA-256 verification uses native asynchronous Web Crypto, rather than hashing
large buffers synchronously in WASM. Corruption invalidates all aliases of the
bad blob and becomes a network miss. Cache writes run **after decoding in the
background**, outside the display completion path; bounded database operations,
quota errors and unavailable storage never keep the loading overlay waiting.
Incompatible old databases and `ergo-read-packets-v1` CacheStorage are deleted
on the cell-format cutover. Old packets are never lazily imported or served in a
legacy grid mode. See [cell migration](cells-and-migration.md).

Every construction/discovery use still sends a fresh conditional request: bytes are displayed only after
an authorized HTTP 304, never as an offline/error fallback. Read denial evicts that
URL; logout suspends pending writes and clears origin storage. Cache failures do
not prevent ordinary network reads.
The renderer retains its local spatial octree while resident and rebuilds it
from locally cached records after reload, rather than downloading an index again.

### Part library document

`/api/config` advertises the installed `part_library` content revision. The
viewer installs the library before loading any construction:

1. `partLibrary(revision)` returns the immutable
   `/api/library/{revision}/library.json`, from IndexedDB or the network
   (gzip transport, about 3.6 MB for the full library).
2. It leases the entry alongside the active region.
3. `ergo_world::part_library::install_json` verifies the content revision before
   installing. An unverifiable document is never used.

The document holds every part's catalogue record and geometry hashes, not the
meshes. Part meshes stay on their normal routes: `/api/parts/{id}` and the
content-addressed, immutable `/api/part-objects/{mesh_hash}`.

The library document can be reused offline after reload. World geometry still
requires its normal authorized revision check. Quota/private-mode failures are
best-effort cache misses, not a promise of browser-owned persistent storage. The
native MCP keeps an equivalent verified mirror in its `.ergo-assets` directory;
see [distribution](part-library.md#distribution).

Public floor colour fields have their own versioned, seven-day local cache;
see [floor-pattern reuse](ground-and-coordinates.md#lazy-generation). They and the
public part library are distinct from permission-checked construction packets.

### Actual capacity and timeout retention

In the viewer's browser console, `await ergoCacheInfo()` reports the effective
logical capacity, stored/protected bytes, unique blobs, region/floor/library entry
counts, browser origin usage/quota, persistent-storage status and successful/skipped
write counts with the last skip reason. The report contains no URLs or geometry.
It is an on-demand diagnostic, not polling on the rendering path. A skipped quota,
protected-capacity or suspended write is not a saved model. Browser quota and
eviction can still prevent persistence even below the configured logical ceiling.

A model has several independent caches; none make its first cold load free:

- The basic source meshes/constructs are proactively stored in IndexedDB; native
  MCP packages embed the pack and keep resolved assets in `.ergo-assets` beside
  the workspace (or `ERGO_MCP_ASSET_DIR`).
- Completed browser region/index packets are content-addressed on local disk.
  Authorization and accepted-revision checks still run before reuse. Different
  revisions/coverage/budgets can require a new packet; a cold packet still requires
  server-side S3 traversal even if the browser already has the basic meshes.
- Floor colours are public, source/style-versioned and retained locally for seven
  days. They share the logical disk budget above, not the model's permission grant.
- Native MCP workspace files remain on disk. A checksum-verified file with a saved
  server ETag can be reopened after an authorized lightweight `HEAD`/304 check,
  without another geometry download. An absent/mismatched checksum, pending write,
  changed token or old server falls back to the ordinary complete read. The file
  is not an offline permission grant or an incremental initial-workspace checkpoint.

On timeout/cancellation, accepted geometry remains in S3, earlier completed local
entries are retained, and an earlier completed viewer scene is restored. A partial
preview is deliberately discarded and is never saved as a complete model. Browser
region preparation now has the durable checkpoints described below; an in-flight
batch can repeat, and HTTP body transfer starts again if interrupted. Initial
native authoring-workspace downloads/exports are not yet chunk-checkpointed.
Verified immutable server objects can remain warm in
the 256 MiB process cache, but that is bounded and disappears on process restart.
Successful complete server packets are separately persisted in S3. Parsing,
spatial-index reconstruction and GPU upload still cost time after a cache hit.

### Durable browser loading checkpoints

The region builder saves its sparse allocated-directory cursor, spatial-page/cell
cursor, discovered IDs and packed records, remaining logical byte budget,
materialized occurrences/shared meshes and membership cursor. Save attempts occur
at completed page/batch boundaries roughly every 30 seconds and at important stage
boundaries. A current cell/page, network read, encoding or GPU preparation may need
to repeat; this is not HTTP byte-range transfer resume or a zero-redo guarantee.

Each private `cache/read-loads/cell128-v1/<world-hash>/<slot>.cbor` envelope binds the
full world/revision/centre/radius/piece/read-byte key. It records format, update time,
monotone progress, decoded length, gzip bytes and SHA-256. Reads bound decompression
to 128 MiB; stored records are at most 32 MiB. Four candidate slots are probed among
64 per world, bounding retained checkpoint objects to 2 GiB/world. Fresh foreign
keys are not overwritten; expired records are reusable after seven days. Completed
empty loads retire their checkpoint. Collisions/capacity, oversize data, unavailable
storage or failed writes can prevent a new checkpoint: status reports availability
and the last saved counts rather than claiming in-memory work is durable.

Conditional writes prevent a delayed writer from rolling progress backwards.
Lost write acknowledgements are checked against persisted data. Corrupt or
inconsistent checkpoints are misses. Every retry checks current world permission
and accepted-history membership before restoring data; no checkpoint grants access,
holds a durable GC pin while idle, supplies a mutation dependency or advances a head.
An interrupted server can therefore resume from a valid checkpoint even after its
process caches disappear. Retention/pruning can make an old revision unavailable.

The browser stores a small pending-read descriptor in the existing IndexedDB data
cache, separately from completed geometry. It retains the exact accepted revision
for up to seven days, subject to normal quota/eviction/logout. Descriptor reads use
the same four-second local-cache grace as region reads, so a cold/contended disk
does not discard the snapshot after the ordinary 300 ms metadata-cache deadline.
Reloading the same view reauthorizes that snapshot; HTTP 410 can restart once at
the current revision, discarding the old preview. Different revisions are never spliced together. Late
completion of an older request cannot erase a newer pending descriptor. Successful
completion leaves a tiny completion marker. The loading panel reports saved/resumed
counts (cells checkpointed during discovery, bricks and cells afterwards), and
`X-Ergo-Load-Resumed` identifies a resumed materialization response.

### Fast Merkle-backed freshness checks

`HEAD /api/shared-worlds/{id}/view?...` and the assembly `/workspace` manifest compare
small ETags before any geometry build. View tokens include the accepted root,
coverage, format and budgets. Workspace tokens bind manifest data, actor,
configured budgets and reader version. Views expose `X-Ergo-Merkle-Root`; both
expose `X-Ergo-Revision`.
Matching `If-None-Match` returns 304; an unmatched HEAD returns headers only and
never starts/checkpoints a load. Native HEAD checks have a 30-second client deadline.

Current permission and historical acceptance are rechecked first. A changed root,
actor, reader version or applicable budget conservatively invalidates the token;
this is not yet fine-grained reuse across unrelated world edits. Paged MCP sidecars
store the binding, loaded posed-record versions and read continuation; opening
reloads the manifest rather than trusting an old full-file geometry cache.
No cached data authorizes a write, avoids server normalization or bypasses
complete-domain validation. See [paged authoring](paged-authoring.md) for bounded
region reads, sparse updates and the explicit forward cache migration.

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
Cold authoring rule reconstruction now scans each member prefix once (1,024-row
pages) instead of first running a separate full snapshot warm-up and then
repeating the scan in the transaction. For 512–20,000 declared members, bounded
512-path waves speculatively prefetch consecutive local-ID member rows; sparse
or missing IDs simply miss the cache and the ordinary observed scan remains the
authority. Once IDs are verified, bounded waves prefetch immutable piece records
before the usual identity-checked and charged reads. Neither lookahead serves an
unchecked scene, changes transaction predicates, or expands authoring budgets.
Server logs report the elapsed rules, piece-prefetch and dependency stages by
assembly ID without logging actor tokens or construction contents. The read-only
`shared-profile-dependencies` report also separates rule reconstruction and
piece-prefetch durations (null when its deadline fires before that phase).
For compact authoring assemblies spanning at most one 128-stud cell width,
the external-dependency guard stops after its first verified external spatial
witness. A negative result still traverses the full query and checks remote
explicit dependencies. A broader assembly returns `external_dependencies:true`
with `external_dependency_check:"conservative_large_span"`: **possible** external
dependencies are not asserted as verified. This avoids a potentially unbounded
per-member read-only spatial closure before an MCP session can start, at the
cost of disabling isolated local simulations even for some independent broad
assemblies. It does not skip or weaken the complete affected-domain validation
on edits. Complete authoring downloads remain revision-pinned and charge
authoritative records; the `HEAD` freshness check is still not a geometry read.

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
**one-hour (3,600,000 ms) deadline**; a failed region is not immediately retried in a loop.
Select it again or choose another construction to recover without reloading the app.
Transient catalogue-refresh failures keep already loaded rows and show a scoped
retry status. Automatic list retries back off from 5 to 60 seconds; a manual
refresh supersedes an older scheduled retry. Background revision polling backs off
to at most 60 seconds and retains the current scene without a global timeout
alert; a successful poll reports recovery. These are GET-only recovery paths,
not mutation retries or permission-free/offline cache reuse.

### Progressive read-only preview

While a cold region builder is materializing brick records, progress polls opt in
to `preview_after=<received-count>`. Each response carries at most 1,024 contiguous
occurrences and only prototypes first introduced by those occurrences. Part
geometry is sent once per preview cursor sequence and all occurrences refer to
that shared prototype. The initial batch can appear after the first materialized
brick; discovery itself does not expose unread geometry. The preview remains
subject to the builder's normal piece/read-byte limits.

Every poll rechecks current read permission and accepted-history membership for
the exact world/revision/region/radius/budgets. These ephemeral, `no-store` batches
do not start builds, write caches, advance cursors server-side, establish complete
membership or claim mechanical certification. Their process buffer disappears
with the builder; a durable preparation checkpoint can reconstruct it on retry.
Late or noncontiguous batches are ignored by the client. If the full response is
already available, no partial batch is needed.

Initial loads and construction selections show an explicitly labelled partial
preview in orbit mode. The view is fitted once when the preview begins; later
preview batches never re-frame the camera, and completion only re-fits when the
camera is untouched since that first fit, so a view the user adjusted while the
region streamed in is kept. Background navigation/refreshes retain the completed
resident scene. Editing, export and walking are blocked on a partial preview;
its geometry never enters the authoritative client blocks or a completed region
cache. The previous completed scene/camera is retained for cancellation, error
or radius fallback, including selection of a resident construction. Only a
successfully decoded and partitioned full response replaces it.

The MCP HTTP client also gives complete construction GETs (`workspace`, `view`,
`download`, `export`) one hour by default. Other requests retain their five-minute default;
an explicit `ERGO_MCP_HTTP_TIMEOUT_SECONDS` still overrides both. The existing
outer tool deadline is one hour; the host should allow 3,900,000 ms (65 minutes).
Basic-library background warming also has a one-hour deadline. Hosting-proxy request limits are separate:
the client deadline does not alter them; exceptionally cold deployments can still
need the operator warm-up below.
The Scaleway container processing deadline supports 3,600 seconds and must be
configured alongside the clients. A longer deadline does not make cold traversal
faster or turn a partial preview into resumable/durable progress.

### Operator warm-up

Source-built servers after v0.5.11 provide a storage-operator command for a first
dense materialization that would exceed the hosting proxy's request deadline:

```sh
ERGO_SHARED_VIEW_PIECES=100000 cargo run --release -p ergo-server -- \
  shared-warm-view "$ERGO_COMMON_WORLD_ID" 156 0 0 21
```

Use the existing application S3 credentials, prefix and common-world settings.
Pass the radius browsers actually request: the navigator derives it from the fog
distance (radius 21 at the default 2,200-stud cutoff). A default radius-1 warm-up
only covers the 3×3×3 centre cube and never matches a radius-21 browser read.
For a district-sized region, first backfill the packed read projection with
`shared-pack-region "$ERGO_COMMON_WORLD_ID" 156 0 0 21` (see [the storage
runbook](storage-operations.md#packed-block-backfill)); bulk pages remove the
per-brick discovery walk that dominates cold materialization. Each cell pack is
its own storage-only commit that advances the head, so warm the packet once
after the backfill finishes.
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
`private, no-store` and does not start a build. Without `preview_after`, `preview`
is null and no geometry is exposed. With an opt-in cursor, `preview` is null or
`{after,next,bricks,meshes}` as described above. Progress and partial geometry are
removed from process memory when the request completes, fails or is cancelled.
Separate durable preparation checkpoints can reconstruct that progress on retry.
The optional `checkpoint` status contains `resumed`, `attempted`, `available` and
`saved` stage/step/cell/piece/row counts. Only persisted writes advance saved counts;
in-memory preview counts are separate and do not claim durability.

`X-Ergo-Read-Cache` reports `miss`, `memory`, `persistent`, `neighbor` or `revalidated`;
`Server-Timing: view;dur=...` reports server-side request duration. Use:

```sh
python3 scripts/profile-shared-loading.py --cell 156 0 0 --radius 21 \
  --output target/loading-measurements.json
```

It compares identity JSON, gzip JSON, gzip CBOR, a repeated read and conditional
revalidation. It prints timing/size/header summaries, not private geometry.
The default district centre is current cell 156; its physical position did not
move during cell migration. The profiling radius defaults to 1; use 21 as above
to reproduce the default navigator's coverage rather than a small point region.
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
