# Native disk-backed working state

Implemented in **unreleased source after v0.5.34**. This reduces native
transaction/authoring execution memory. It is **not** fully out-of-core physics,
a resumable authoring session, or an additional accepted-world database.

## One authoritative publication path

The canonical CBOR objects, birth IDs, octree paths, ordered edit records,
generation/tombstone fences and accepted-history format are unchanged. Repository
reads still verify immutable objects; publication still uploads complete reachable
objects, validates all read predicates and performs the one existing head CAS.
There is no partial island publication, substitute root or unconditional save.

`crates/transactions/src/spill.rs` provides typed, ordered local disk maps and an
object spool using bundled SQLite through rusqlite. A private temporary directory
is created lazily, only when working records need storage. Snapshot-only reads do
not create a database. No SQL cursor or connection lock is held across an await.
Keys are bound BLOB parameters, never SQL fragments or filesystem paths. Logical
key order is preserved for octree paths, content IDs and numeric edit sequence IDs.

The native transaction uses this **single** working-state implementation for:

- staged canonical node/payload bytes, including superseded intermediate roots;
- observed subtree versions and directory-presence predicates;
- the ordered edit journal and explicit attachment IDs;
- the reachable upload frontier, its visited marks and successful-upload flags;
- application-local maps sharing the same transaction cache and disk budget.
- native piece-cache records, including cached absence/deletion, preserving
  distinct-identity accounting and read-your-writes without retaining every pose;
- coupled-solver partition/interface records and bounded-cache scalar pages.

Decoded staged and remote nodes share one disposable, approximately 8 MiB node
cache. Eviction reloads staged bytes from disk, not from an uncommitted S3 object.
Snapshot clones share immutable commit metadata with `Arc`; opening the current
base reuses that metadata instead of decoding the same commit twice. Commit edit
and attachment lists now use [verified, file-backed records](commit-history.md#bounded-native-commit-records),
not decoded vectors retained by each snapshot.

## Bounded uploads, retries and failures

Flush traverses staged reachability through the disk queue. It does not collect
all reachable bytes in a RAM map or all uploaded IDs in a RAM set. Already-uploaded
parents are still traversed: an earlier parallel upload may have succeeded for a
parent but failed for a child. Each successful immutable upload is marked locally;
a retry still requires the complete reachable closure before publication.

Staged bytes are checked against their content IDs when read for traversal/upload.
SQL, filesystem and scratch decode failures poison the local store: later staging,
replay and publication cannot silently continue with a truncated index or fall
back to RAM. A failed read-size probe is not corruption and can be retried with a
different explicit read limit. Ordered map iteration checks its expected row count.

Shared HTTP scratch failures return 422, `validation_state:"inconclusive"` and
`published:false`, with no candidate revision. They are not evidence of physical
instability. Unreferenced immutable objects or candidate commits can already have
been uploaded before such a pre-CAS failure; their existence is not acceptance.
Network/CAS uncertainty keeps its existing distinct recovery rules;
local file existence is never proof of accepted publication.

## Authoring dependency graph and rule comparison

`working_graph.rs` indexes conservative authoring-group peers, explicit incident
joints and fixed-body markers on disk. A member query loads bounded pages, not a
second complete in-memory adjacency map. Replacing rules replaces that assembly's
graph prefix, so removed group/constraint edges cannot survive in the cache.
Groups remain authoring structure, not physical welds.

Generic dependency traversal keeps its pending and visited sets on disk, retaining
the previous deterministic largest-ID-first order. It still follows every explicit
peer, external joint and admissible spatial neighbor. Failure aborts rather than
pretending an unseen boundary is empty. The returned affected-ID/audit set still
materializes at the validation boundary; the compact whole-member fast path also
retains its input sets and spatial index.

`Rules::dependency_edges` streams directed edges one group's descendants at a
time. Paged-rule creation and comparison deduplicate them in local disk maps and
emit the same canonical record families. They no longer collect full old/new row
dictionaries and a separate full adjacency graph in RAM. Existing permissions,
whole affected-domain checks, quota and atomic publication remain mandatory.

## Resource controls and operations

Shared-native physical validation now uses the
[paged solver workspace](partitioned-stability.md#paged-coordinator-state), sharing
the same transaction-local SQLite quota. Plans are decoded one at a time; reaction
state uses 256-column checksummed pages with at most 32 decoded pages (320 KiB of
scalar records plus overhead). Neither storage failure nor page loss enables an
in-memory fallback or a partial certificate. These limits do not remove the
existing 32 MiB matrix-free correction scratch estimate or local KKT/work caps.

Defaults per transaction (or combined batch working store) are:

- 512 MiB maximum main SQLite database, rounded down to 4 KiB pages;
- 1,024 KiB configured SQLite page cache, with memory mapping disabled;
- at most 128 rows and approximately 1 MiB encoded data per map/merge page;
- approximately 8 MiB staged-byte upload batches, additionally constrained by
  existing upload concurrency;
- 128 MiB maximum individual staged/working record. A single large record can
  exceed the page/upload batch target; serialization and SDK buffers still exist.

These are working-buffer and main-file limits, **not an allocator/process RSS or
total filesystem quota**. SQLite bookkeeping, SDK buffers, filesystem cache,
parallel transactions and individual decoded records add overhead. SQLite
temporary storage uses files. No server RAM setting is increased.
An unlimited authoring account does not remove these processing-resource limits.

Rust callers can use `Repository::with_spill_limits(SpillLimits)` and inspect
`Transaction::spill_database_bytes()`. Shared-server configuration uses:

- `ERGO_SHARED_SPILL_BYTES` (default 536870912; minimum 65536; at most 2³¹−1 pages);
- `ERGO_SHARED_SPILL_CACHE_KIB` (default 1024; accepted range 4–8192).

`TMPDIR` selects temporary storage through the normal platform tempfile behavior.
Provision private, writable local space for all concurrent transactions. Disk
exhaustion refuses the edit; it does not increase memory budgets. The database is
disposable (`journal_mode=OFF`, `synchronous=OFF`), not crash-recoverable state.
Normal final-handle drop removes the private directory. A crashed process can
leave scratch files; remove only directories whose owning process is stopped.
Deleting scratch does not settle an ambiguous remote publication or release its
durable pin: follow the normal history/pin recovery procedure first.

Bundled SQLite is compiled into native builds; no system SQLite service or database
migration is required. Rebuild matching native components and retain the separate
[authoring-cache migration](paged-authoring.md#forward-migration) for old MCP caches.
Accepted world records need no rewrite for this working-state change.

Commit metadata has separate limits: `ERGO_SHARED_COMMIT_BYTES` defaults to
134217728 encoded bytes and `ERGO_SHARED_COMMIT_ITEM_BYTES` to 1048576 per header
value/list item. Both must be positive; item bytes must not exceed total bytes,
and total bytes cannot exceed 536870912. Rust callers use
`Repository::with_commit_limits(CommitLimits)`. A grouped publication also applies
the total encoded-commit budget across all accepted members before its head CAS.
These private commit files are outside the main SQLite-file quota; active snapshot
files add to temporary disk use. Oversized accepted records are refused under the
configured budget, not decoded through a whole-vector fallback.
When repository caching is enabled, up to 32 verified commit files can be retained
under an additional 128 MiB logical-file/approximately 1 MiB retained-header budget
(the file budget is also limited by ordinary cache capacity). Active metadata/list
handles can retain other files. No decoded edit vector or encoded-body byte LRU
entry is kept merely to cache a commit. These caches do not pin remote history.

## Remaining resident state

Assembly rules and membership maps, cached meshes/spatial references, the affected
physical scene, planner incident/owner indexes and independent verification
incidence still materialize. The piece cache, native partition/interface records
and global reaction state now spill. Local/WASM numerical entry points default
to a resident workspace. Commit construction, snapshot loading and GC no longer retain decoded
edit/blob vectors. The storage SDK's conditional create API still requires one
size-capped encoded upload buffer; commit uploads are serialized within a batch
and do not populate the ordinary byte LRU. Explicit general-purpose serialization
and HTTP JSON history responses can also materialize their bounded output buffers.
The [coupled solver](partitioned-stability.md) bounds local factorization, not these
inputs. Durable sessions, asynchronous publication and fully out-of-core physical
input/index handling remain future work. Do not infer a fixed end-to-end RAM
guarantee for a large castle from these local limits.

Sources: `crates/transactions/src/{spill,tree,lib,batch}.rs`,
`crates/server/src/shared/{working,working_graph,equilibrium_workspace,paged,model}.rs`.
