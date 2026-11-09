# Native disk-backed working state

Implemented in **v0.5.35 source**. Native transaction, authoring traversal and
physical working records are disk-backed. This is not a fixed whole-process RSS
guarantee, a resumable authoring session, or an additional accepted-world database.

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
- normalized physical-body records, global-to-local IDs and conservative spatial
  candidates, without retaining a complete affected pose dictionary.

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

### Streamed physical input

`shared/validation_input.rs` supplies `validation::ValidationInput` to the same
physical gate as the resident local/WASM slice adapter. The shared server no
longer constructs a complete `Vec<Brick>`/`WorldExport` before checking geometry.
It reads affected canonical pieces in deterministic ID order, normalizes each pose
with the frozen physical model **before indexing**, and stores it in the existing
transaction-local database. SHA-256 binds each body to its ordinal and canonical
body bytes. A separate checksum binds each global/local identity mapping; its
complete ordered scan also checks for missing rows before resolving constraints.
A decoded FIFO retains at most 64 bodies, each limited to 16 KiB of
encoded body data (at most 1 MiB encoded-equivalent, plus allocator overhead).

The transient SQLite RTree covers nominal bounds, registered connectors, reviewed
bearing faces and collision profiles. Its stored f32 bounds round outward; exact
f64 predicates still decide physical relations. Queries enumerate all candidates
in ordinal order through at most 128 IDs per page, never a truncated neighbor set.
The adapter checks the actual index row count and SQLite `rtreecheck` parent/node
containment and mapping integrity, then the common gate checks every
body's identity, frozen-model normalization and conservative stored envelope before
pair enumeration. Missing, corrupt or inward-rounded records refuse validation;
they never turn into empty space. Nonfinite/inverted bounds or magnitudes beyond
half the f32 maximum are inconclusive, not an alternate all-pairs/RAM fallback.
No index cursor survives a page read or await. The accepted spatial octree and
its dependency-closure/publication semantics are unchanged: this RTree is only
disposable validation scratch, not a second authoritative world index.

After streamed geometry and joint inference, discovery forms **complete mechanical
components** through every joint and admissible nominal/reviewed bearing. Its
union-find, body/joint membership, anchor flags and component metadata are fallible
records, not scene-sized vectors. Spatial pages are never independently supported
islands. `ValidationConstraints` stores joints, anchors, pair indexes and incident
markers in the same scratch store and audits their identities before certification.

Compression reads one body and its real candidate bearings at a time. Its ordered
queue uses descending finite height and source ordinal as a stable tie-break;
both resident and native adapters use this order. Load records distinguish fresh,
pending and processed states explicitly: a lost pending row cannot be recreated
with only its own weight. Own weight is still added before received reactions;
processed accumulator values are discarded. Queue count/identity checks precede
early physical diagnostics. At most 16,384 support points are collected for one
body; exhausting this work buffer is inconclusive, not a weaker compression proof.

When a component needs general equilibrium, its bodies/masses, joints, all nominal
and reviewed bearing contact patches, ground points and anchors are written to a
`StoredProblem`. `ProblemInput` reads these canonical records fallibly; it does not
load the complete component. Morton ordering, core ownership and planner incidence
also use records, with bounded active core/halo sets. The independent verifier
derives a separate incidence index and checks its per-body counts while streaming
the complete witness. No planner omission can remove a canonical body balance.

`shared/validation_workspace.rs` checks SHA-256 bindings between every record and
its key, caps an encoded record at 64 KiB and retains at most 256 records / 256 KiB
of accounted decoded payload per store (plus map/allocator overhead). Pages use the
existing 128-row / approximately 1 MiB encoded bound. The bounded local SOCP and
optional 32 MiB-estimated numerical correction are the only materialized solver
work buffers; correction's numeric metadata is included in that same cap, with
part/source strings discarded. Storage/resource failure is inconclusive and
prevents publication of every component, including those already checked.

Rust `SolverWorkspace` implementations now supply `new_state`, separate
component/compression stores and independent planner/verifier stores. Numerical
`reset` must not erase component membership mid-validation. `Transaction::local_store()`
is the shared disposable-state factory under the transaction quota; it grants no
publication capability. `validate_constraints_with_workspace` is the shared-native
entry point; vector/slice entry points adapt explicit resident scenes to the same
gate rather than providing another validation algorithm or transport endpoint.

### Solver and database budgets

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
`SQLITE_TMPDIR` selects SQLite's separate temporary/sort files. The v0.5.35 image
and Scaleway module set both to owned mode-0700 `/var/lib/ergo-tmp`, on the writable
disk-backed layer. Do not use `/tmp` for production spilling on Scaleway sandbox
v2: that mount is tmpfs and counts directly toward container RAM. The deployment
provisions 4,295,000,000 bytes of ephemeral disk while keeping the previous
2,148,000,000-byte RAM setting unchanged. Other hosts must likewise verify that
their chosen scratch filesystem is disk-backed; an in-memory mount defeats the
memory reduction. See [Scaleway's sandbox directories](https://www.scaleway.com/en/docs/serverless-containers/reference-content/containers-sandbox/).
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
ID/audit set, HTTP request/response buffers and explicitly requested local/full
exports still materialize under their existing budgets. Shared-native physical
poses, constraints, component records, canonical equilibrium inputs, planner and
independent verifier indexes no longer require full resident copies. The piece
cache, partition/interface records and global reaction state spill too.
Local/WASM numerical entry points default
to a resident workspace. Commit construction, snapshot loading and GC no longer retain decoded
edit/blob vectors. The storage SDK's conditional create API still requires one
size-capped encoded upload buffer; commit uploads are serialized within a batch
and do not populate the ordinary byte LRU. Explicit general-purpose serialization
and HTTP JSON history responses can also materialize their bounded output buffers.
The [coupled solver](partitioned-stability.md) bounds its local materialization and
factorization. Durable sessions and asynchronous publication remain future work.
Do not infer a fixed end-to-end RAM
guarantee for a large castle from these local limits.

Sources: `crates/transactions/src/{spill,tree,lib,batch}.rs`,
`crates/server/src/shared/{working,working_graph,validation_input,validation_workspace,equilibrium_workspace,paged,model}.rs`,
`crates/world/src/{validation,validation_input,validation_components,validation_constraints,validation_workspace,equilibrium_input,equilibrium_planner}.rs`.
See [SQLite RTree rounding](https://sqlite.org/rtree.html#roundoff_error) for the
conservative candidate storage (exact geometry is not rounded).
