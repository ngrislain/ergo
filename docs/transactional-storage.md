# Transactional CBOR geometry storage

## Implementation and scope

[`ergo-transactions`](../crates/transactions/src/lib.rs) is a reusable Rust library
for concurrent edits to immutable, content-addressed CBOR geometry blocks. It uses
the same `object_store` 0.12 backend interface as Ergo's server, including S3 and
S3-compatible endpoints. A persistent octree, optimistic dependency validation,
and a conditional head replacement provide **serializable transactions within
one branch**, assuming a backend with linearizable conditional writes and strong
reads. Read-only transactions observe their immutable starting snapshot.

The library also backs the implemented authenticated
[shared-world REST service](concurrent-global-editing.md). That service adds stable
creation-record IDs, grants, reference-only spatial blocks, on-demand dependency
closure discovery, and static validation. Persistent worlds have no aggregate size
cap; only individual working sets are bounded. Private snapshots still use
`crates/server/src/store.rs`, and the local MCP working copy remains a separate
workflow. The library itself does not authenticate callers or interpret geometry.

## Minimal use

Add `ergo-transactions = { path = "../ergo/crates/transactions" }` to another Rust
project, adjusting the path. Inside this workspace use
`ergo-transactions.workspace = true`. Supply an
`Arc<dyn object_store::ObjectStore>` to `Repository::new`.

```rust
use ergo_transactions::{CommitOptions, OctreePath, Repository};
use object_store::memory::InMemory;
use std::sync::Arc;

async fn example() -> ergo_transactions::Result<()> {
    let repo = Repository::new(Arc::new(InMemory::new()), "geometry-transactions")?;
    repo.create("shard-17").await?;
    let region: OctreePath = "2/6".parse()?;

    let mut tx = repo.begin("shard-17").await?;
    let mut vertices: Vec<[f32; 3]> = tx.get(&region).await?.unwrap_or_default();
    vertices.push([1.0, 2.0, 3.0]);
    tx.put(&region, &vertices).await?;
    let accepted = tx.commit("alice", "add vertex", CommitOptions::default()).await?;
    let saved: Vec<[f32; 3]> = accepted.snapshot.get(&region).await?.unwrap();
    assert_eq!(saved.len(), 1);
    Ok(())
}
```

The crate-level rustdoc also contains a compiled, runnable example. Payloads can
be any Serde-serializable type, including `ergo_world::Block`, meshes, or
application-specific geometry chunks. The crate itself has no dependency on the
viewer, physics, or world schema.

### S3 / Scaleway

[`examples/s3_edits.rs`](../crates/transactions/examples/s3_edits.rs) constructs an
`AmazonS3Builder` with `S3ConditionalPut::ETagMatch` explicitly enabled. Export
`S3_BUCKET`, `AWS_ACCESS_KEY_ID`, and `AWS_SECRET_ACCESS_KEY`; optionally set
`S3_REGION`, `S3_ENDPOINT`, and `S3_PREFIX`. Its defaults are Scaleway `fr-par`,
`https://s3.fr-par.scw.cloud`, and prefix `ergo-transactions`. It does not load
`.env` automatically. Each run creates an isolated example branch and retains its
objects for inspection:

```sh
cargo run -p ergo-transactions --example s3_edits
```

ETags are opaque CAS tokens; the library neither assumes MD5 nor derives them
from content hashes. Both returned ETag and version are retained. Immutable PUTs
use `PutMode::Create`; head updates use `PutMode::Update`. There is no unconditional
write fallback. In-memory storage supports the protocol for tests. The
plain `LocalFileSystem` backend in object_store 0.12 does **not** support conditional
updates. `ergo_transactions::local::LocalCasStore` adds cross-process advisory-lock
CAS and atomic file replacement for local development. All writers to its
transaction namespace must use the adapter on a filesystem supporting those
primitives. It is not a distributed/network-filesystem implementation; multipart
upload is unsupported by the adapter.

## State and object layout

Use a dedicated namespace, separate from the existing world's flat-manifest layout:

```text
<transaction-prefix>/
  refs/<branch-or-shard>.cbor     mutable { format: 3, commit: CID, history: {root, retained_from} }
  objects/<sha256>.cbor          immutable commits, octree nodes, geometry payloads
  maintenance.cbor              opt-in durable admission pins / exclusive barrier
```

Branch names contain 1–128 ASCII letters, digits, hyphens, or underscores.
`create` initializes an empty branch using create-if-absent and returns
`BranchExists` for an existing branch. A branch is one independent serialization
domain; it is not automatically a fork of another branch. Branches share the
immutable object namespace and therefore deduplicate identical payload bytes.

The head references a parent-linked immutable `Commit` containing:

- Format version, unique transaction ID, and consecutive sequence number.
- Actual accepted parent, original execution base, and materialized octree root.
- Ordered subtree substitutions (`Edit`: path, generation, payload ID or deletion).
- Caller-supplied actor and message, which are **not** identity verification.
- Optional immutable metadata/audit CID and explicit blob attachments for retention.

Octree nodes are branches with eight optional children, packed unary extensions,
geometry leaves holding payload IDs, or deletion tombstones. Unfenced unary paths
collapse into one extension; fenced branches remain explicit. Current format 3
requires an atomic acceptance index. Other persisted versions are rejected.
A geometry leaf occupies its entire region.
`put` replaces the **whole subtree**, not individual fields inside a geometry;
`remove` logically deletes that region. Remove a covering leaf before subdividing
it into children. `get` returns `NotLeaf` for a branch and `CoveredByLeaf` if a
requested descendant lies inside an unsplit geometry leaf. `scan` enumerates all
geometry leaves under a region in octant order and skips tombstones.

Every accepted root is a complete materialized checkpoint. `snapshot(commit_id)`
opens historical state directly; neither current reads nor time travel replay a
delta chain. The substitution log supplements the tree; it is not a semantic
modeling-operation log. Even payloads superseded by later edits in the same
transaction are uploaded, because the ordered history record references them.

## What counts as independent or commutative?

An octant is $q=b_x+2b_y+4b_z$, using public X east, Y north, Z up. The empty path
(`/` when displayed) denotes the whole shard. `2/1` and `2/6` are disjoint;
`2` and `2/1` overlap. `OctreePath::is_disjoint_from` exposes this structural test.
Depth is at most 192. `OctreePath::for_block([x,y,z])` maps signed i64 coordinates
to 64 octants by flipping each coordinate's sign bit and interleaving bits from
most to least significant. Append octants for a block-local hierarchy. Convert
`ergo_world::BlockCoord` to its public `[i64; 3]` representation first.

For paths $p,q$, define:

$$
\operatorname{overlap}(p,q)\iff p\preceq q\lor q\preceq p,
$$

where $\preceq$ is the prefix relation. Let $W_T$ be a transaction's replacement
regions and $R_T$ its observed regions. Two transactions prepared against the
same snapshot have an application-independent sufficient condition for safe
reordering:

$$
\begin{aligned}
\forall p\in W_A,\ q\in(W_B\cup R_B):&\quad\neg\operatorname{overlap}(p,q),\\
\forall p\in W_B,\ q\in R_A:&\quad\neg\operatorname{overlap}(p,q).
\end{aligned}
$$

Disjoint subtree substitutions then commute at the **geometry-state level**:
applying A then B produces the same geometry as B then A. Commit IDs and history
order intentionally differ. The implementation tests this in both orders and
does not infer commutativity of arbitrary operations inside one leaf. Two edits
to the same leaf conflict even if their CBOR payloads happen to be identical.
Split independently editable meshlets, cells, materials, or LOD chunks into
separate leaves if that finer concurrency is needed.

### Replacement generations and tombstones

Hash comparison at the exact target alone misses an important overlap:

1. A plans to delete all of `2`.
2. B plans to insert `2/5`, which was absent in their common base.
3. After A commits, `2/5` is still absent, but B must detect A's ancestor edit.

Every explicit replacement therefore gets a stable `transaction-UUID/edit-index`
generation. Path-copying ordinary ancestors preserves their replacement generation.
Deletion leaves a tombstone carrying the new generation, and later subdivision
preserves that generation on the new branch. A dependency records the target
subtree hash/absence/covering leaf **plus replacement generations on ancestors**.
This catches ancestor deletion, ancestor replacement, and ABA changes while
allowing unrelated child edits to rebase. These generations are optimistic version
markers, not distributed leases or authorization fences.

Do not prune tombstones or reset head history while transactions can still use
older bases. `Maintenance::compact_region` prunes current markers only behind an
exclusive coordinated barrier, with a visited-node budget. Historical roots remain
unchanged. Dependencies ending inside a packed extension use a canonical virtual
suffix hash, preserving the same logical prefix conflict rules.

## Reads, validation, and commit

1. `begin` reads one head with its ETag/version and opens its immutable checkpoint.
2. `get`, `scan`, and `watch` record dependencies against that starting snapshot.
   Reads see the transaction's own pending edits. Missing reads are dependencies;
   `scan` protects its whole subtree against phantom insertions/deletions.
3. `put` and `remove` also record dependencies, stage deterministic CBOR payloads,
   and path-copy only affected ancestors in memory. No per-object locks are taken.
4. `commit` uploads reachable staged nodes, payloads referenced by history, and an
   immutable candidate commit. Uploads have bounded fanout.
5. One conditional replacement of the branch head publishes everything atomically.
   This is the transaction's linearization point.
6. On a conditional conflict, re-read the head and check whether the candidate
   actually committed (a retried request may return 412 after losing a success
   response). Otherwise validate all observed versions against the newer root.
7. Changed dependencies produce `Error::Conflict { paths }`. The caller must
   re-read and re-execute its geometry validation on a new transaction. If all
   dependencies are unchanged, replay the exact substitutions on the new root
   and retry its CAS. Caller code and external side effects are not replayed.

Successful transactions are serializable because their reads still match the
state immediately preceding their successful root CAS; all writes become visible
together. Readers see an immutable snapshot plus their own edits. Abandoned
uploads cannot make a partial multi-block move visible. This is a storage
guarantee, not automatic verification that arbitrary caller-produced geometry is
valid. All shared state used in the decision must be accessed through that
transaction, or be immutable data discovered through tracked references.

`watch(region)` registers a predicate without downloading its geometry payloads.
For a move, read/watch the original and destination references, swept collision
volume, neighboring candidates, changed connector records, and complete support
dependencies used by the validator. A mechanical support chain may extend far
beyond neighboring octants. Two disjoint branches can still depend on the same
brick, support, material, or constraint. Store authoritative identity records and
their replica references consistently and include them in the same transaction.
The library does not derive those dependencies from arbitrary CBOR.

A read-only `commit` returns its starting or explicitly refreshed checkpoint and performs no publication
or revalidation; it is a snapshot read, not a reservation of current state.

`begin_at(branch, accepted_revision)` preserves an older base's preconditions;
it cannot overwrite unrelated newer state. `Transaction::refresh` explicitly
validates/rebases before application checks and returns the underlying refreshed
tip before pending edits. Subsequent reads use that refreshed immutable base.
`set_metadata` attaches a certificate without turning it into geometry state.

`Snapshot::scan_page` and `Transaction::scan_page` return at most 10,000 leaves
plus an exclusive `next` path. Resume on the same revision/region. Transactional
pages watch the entire queried region, including an empty result. `get_limited`
and `blob_limited` reject oversized objects using response metadata before reading
the response body; limits also apply on cache hits.

### Failure outcomes

Defaults are 32 publication attempts, 32 concurrent immutable uploads, and
exponential jittered backoff with a 2 ms initial ceiling and 100 ms maximum.
`CommitOptions` controls these values. `Contention` means this invocation exhausted
its CAS budget without publishing; `Conflict` identifies invalidated paths.
These transactions are consumed; begin a new one to recompute.

An I/O error during immutable staging leaves the head unchanged. An unexpected
error while replacing a head returns `PublicationUnknown { branch, candidate,
source }`, because the request could have committed even if its response was lost.
`was_committed(branch, candidate)` follows accepted parent history and proves
success even after later commits; the mere existence of the staged commit object
does not prove acceptance. A false result is only absence from the currently
observed history: a timed-out request may still be in flight. Do not blindly
repeat a non-idempotent operation on that basis. Positive reconciliation, a
settled request, or a later head advancement fencing out the old CAS is needed
before deciding to re-execute. The library does not provide durable cross-process
operation deduplication. In coordinated GC mode, unknown/cancelled head writes
retain their durable pin so collection cannot delete objects they might publish.
If a conditional conflict neither reconciles the candidate nor demonstrates a
newer head version fencing the request, the result is conservatively unknown;
the implementation does not blindly continue after an unfenced attempt.

## Geometry encoding and sharing

`canonical_cbor` materializes a Serde value, recursively orders maps by the
lexicographic bytes of their deterministic keys (RFC 8949 core order), rejects
duplicate canonical keys, emits definite lengths and shortest exact numeric
encodings, rejects non-finite CBOR floats, and normalizes float negative zero.
The profile is implemented in [`codec.rs`](../crates/transactions/src/codec.rs).
It differs from RFC 7049's length-first map ordering. Opaque numeric byte strings
are unchanged: normalize their endianness, NaNs, and signed zeros in the geometry
codec if byte-level deduplication requires it. Typed arrays can be represented
with CBOR tags and byte strings; the library does not impose a mesh schema.

`ContentId` is lowercase hexadecimal SHA-256 of those uncompressed CBOR bytes.
Downloads verify hashes before decoding. Identical geometry deduplicates;
replacement generations intentionally distinguish edits to that geometry.
`Repository::put_blob`/`blob` support shared immutable meshes referenced by blocks.
Prefer `Transaction::stage_blob` for new meshes and `retain_blob` when reusing an
older standalone blob. Attachments are recorded in the commit and retained by GC.
`retain_blob` verifies remote existence while the writer is pinned, bypassing a
possibly stale orphan cache entry. Arbitrary application-defined links still
require the collector's link callback; CBOR IDs are not automatically interpreted
as links. Hash verification is not authentication or authorization.

Use per-chunk local vertex indices and explicit seams to avoid changing every
index chunk after a local edit. Spatial/semantic chunks around 256 KiB–4 MiB are a
starting point to benchmark, not an enforced limit. There is no binary delta
chain or compression layer in this implementation. Old payloads are never
overwritten or deleted by an edit.

## Scaling and current limits

- Each branch/shard has one serialized publication point. Parallel construction,
  upload, and validation do not eliminate same-head CAS contention. There is no
  claim of unlimited commits/sec or a measured S3 throughput target.
- Choose locality-based shard roots and short shard-local paths. Multi-region
  edits inside a shard are atomic; **separate shard heads are not one atomic
  transaction**. Partition boundary-spanning edits into a shared serialization
  domain or add a separate cross-shard protocol.
- Many edits can share one `Transaction` and therefore one publication. Automatic
  group-commit scheduling is opt-in in the shared REST service, using library
  `commit_batch`. A distributed sequencer, cross-shard consensus, CRDTs, and
  semantic same-leaf merge are not implemented.
- Unary levels are packed into extensions. Cold traversal follows actual branches
  and packed edges instead of fetching one object per address octant. Worst-case
  depth is still bounded by 192. Spatial locality and shard design remain important.
- `with_cache_capacity(bytes)` enables a verified-object byte-budgeted LRU shared
  by repository clones and child namespaces. Cache keys include the namespace;
  heads and coordination state are never cached. `cache_stats` reports residency,
  hits, misses, and evictions. Each server repository cache uses a 256 MiB budget.
  Concurrent immutable reads of the same namespace/object share one verified
  download; failures remain retryable. Acknowledged uploads populate the LRU,
  but an LRU hit never skips a subsequent upload, preserving orphan/GC safety.
  Explicit uncached verification still bypasses the cache and read-coalescing gates.
  `Snapshot::scan_page_prefetched` optionally looks ahead four immutable-node levels
  with 64 concurrent GETs, a 64 KiB node bound and two-second speculative
  timeout. Validated decoded nodes are reused within the page rather than fetched
  and decoded by overlapping lookahead repeatedly. It never follows application-value
  links; invalid speculative nodes fall back to required traversal and its ordinary
  errors. Transaction predicate registration is unchanged.
- Nodes are also cached within a transaction; staged values and serialization
  buffers remain in memory. There is no whole-process heap quota, streaming CBOR
  encoder, or automatic chunker. `scan` is an unbounded compatibility convenience;
  use `scan_page` for large trees. The shared-world service never uses a world scan.
- An edit path-copies O(depth) nodes; sequential edits share cached ancestors and
  only final reachable staged nodes plus history payloads are uploaded per attempt.
  Failed CAS attempts can leave orphaned candidates/ancestors. Failure handling
  may traverse commits since an earlier attempt to reconcile ambiguous success.

## Coordinated retention and bounded maintenance

`with_coordinated_gc()` opts a namespace into durable admission pins. **All clients
in a collected namespace must use it.** Uncoordinated mode remains useful when
collection is disabled. Managed snapshots/transactions pin the namespace before
reading their bases; clones share a pin. `Snapshot::close` awaits final unpinning;
drop attempts it asynchronously. Runtime shutdown or release failure can leave a
safe leaked pin. Unknown/cancelled root publications deliberately keep their pin.
Pin registration/release adds coordination CAS traffic and is not free scalability.

`Repository::maintenance` atomically checks that there are no pins and fences new
readers/writers. Pins and barriers do not expire. `active_pins` and
`maintenance_status` support inspection. `recover_abandoned_pin` and
`recover_maintenance` are operator operations requiring the old holder **and all
remote mutation requests** to be stopped/settled. Timeouts alone are insufficient.

Behind that barrier, `Maintenance::collect` marks accepted heads, every retained
parent/base commit, current/historical roots, ordered edit values, audit metadata,
and explicit attachments. A callback traces other application-defined blob links.
It completes marking before sweeping unmarked keys. All history is retained by
default; explicit `prune_history(keep_last)` changes accepted-history roots. For
indexed branches, GC follows retained index entries rather than retaining every
parent/base forever. Application links must be traced after their original import
commits are pruned. The shared collector supplies the schema-aware callback.

The collector streams object inventory and uses a temporary sharded on-disk mark
set and disk-backed frontier, avoiding a world-sized in-memory graph or inventory.
`GcOptions` configures scratch directory, maximum individual object bytes (default
128 MiB), and delete fanout (32). Scratch metadata needs disk space proportional
to reachable IDs; RAM is bounded by an individual decoded object, SDK paging,
cache budget, and delete fanout, rather than total world size. The worker uses a
blocking pool for local scratch I/O. The temporary metadata is not a persistent
application database.

`compact_region(branch, region, max_nodes)` path-copies a bounded current subtree,
removes tombstones/redundant fences, and publishes a maintenance checkpoint.
`compact(branch)` is a convenience with a 10,000-node root budget. Larger regions
fail explicitly before publication; choose a smaller region. Historical roots are
not rewritten or deleted.

`finish` explicitly releases successful maintenance. Cancellation/crashes and
failed/uncertain collection leave the barrier in place, avoiding late-delete races
with new CID reuse. A grace period alone cannot provide that guarantee, and bucket
age expiry on `objects/` remains unsafe.

## Repeatable workload benchmark

```sh
cargo run -p ergo-transactions --example concurrent_bench -- \
  --writers 32 --shards 4 --edits 2 --payload-bytes 4096
```

The synthetic benchmark checks final leaf counts and emits JSON latency
percentiles, throughput, attempts/rebases, and cache statistics. Default storage is
in-memory. Add `--coordinated-gc` to measure admission overhead; add `--s3` to use
exported S3 settings and an isolated retained `ergo-transaction-bench/<uuid>/`
prefix. It does not load `.env` automatically. Local runs validate the harness;
they are not evidence of a live S3 throughput guarantee.

`--group-size 16` stages waves and exercises one-CAS multi-member publication;
the arrival schedule differs from independent-writer mode. Each member retains
its own commit/root/audit, and incompatible members fail independently. The JSON
separates actual head-CAS attempts from summed member attempts. Read-only snapshots
use ordinary `commit`, rather than the write-oriented batch API.

## Acceptance, migration, and predicates

Format-3 heads publish the acceptance-index root in the same CAS as data. A commit
cannot contain its own CID without a hash cycle, so history indexes live in the
head and point to immutable revision entries. `accepted_snapshot`, `revision_entry`,
`revisions`, and `was_committed` use direct CID/sequence lookup. Missing pruned
entries are not accepted receipts; a negative result after retention is not proof
that an operation never happened. No historical-format traversal fallback is used.

`directory_exists` tracks structural presence and replacement generations rather
than every descendant version. A true directory can contain tombstones; use actual
row reads/predicates for business data. `scan_level` pages directory prefixes at a
logical depth without scanning all descendant leaves. A page traversal has a
100,000-node budget. Narrow the region or compact it when that operation budget is
exceeded; it is not a world-size limit.

`Maintenance::rewrite` consumes a stream of record edits, flushes bounded batches,
and publishes only the final derived-index root. A failed intermediate batch does
not alter the authoritative head. The source snapshot is valid only inside that
maintenance job. `Maintenance::replace_history` installs a pre-audited current
history under the same barrier and preserves its complete retained sequence range. See
[storage operations](storage-operations.md) for commands and the opt-in retention
API.

## Verification

```sh
cargo test -p ergo-transactions
cargo clippy -p ergo-transactions --all-targets -- -D warnings
cargo fmt -p ergo-transactions --check
```

Tests cover 64 concurrent independent transactions, both orders of commutative
edits, same-leaf and ancestor/descendant conflicts, tombstone generations, phantom
reads, write skew, support dependencies, multi-block moves, historical reads,
deterministic encoding, hash corruption, partial staging, bounded contention, and
lost/hidden publication acknowledgements. An unsupported local-filesystem CAS is
also tested to ensure there is no unconditional fallback.

Run the opt-in endpoint check against a disposable configured bucket:

```sh
cargo test -p ergo-transactions --test s3 -- --ignored --nocapture
```

It runs five 16-writer same-ETag races, checks immediate winning-value visibility,
then checks independent rebasing and old-snapshot reads. It prints and retains a
unique `ergo-transaction-tests/<uuid>/` prefix for inspection and manual cleanup.
It is an empirical endpoint smoke test, not a proof of all distributed failure
modes. Ordinary tests use the in-memory backend; live Scaleway behavior requires
those external resources and is not inferred from them.

## Source map

- [`lib.rs`](../crates/transactions/src/lib.rs): repository, immutable snapshots,
  transactions, optimistic publication, history reconciliation, errors/options.
- [`tree.rs`](../crates/transactions/src/tree.rs): persistent octree, dependency
  tokens, replacement fences, path-copying, bounded uploads.
- [`path.rs`](../crates/transactions/src/path.rs): validated octant paths and signed
  block-coordinate mapping.
- [`codec.rs`](../crates/transactions/src/codec.rs): deterministic CBOR profile.
- [`cache.rs`](../crates/transactions/src/cache.rs): namespace-scoped byte LRU.
- [`maintenance.rs`](../crates/transactions/src/maintenance.rs): durable pins,
  exclusive barriers, disk-backed streaming collection, bounded compaction.
- [`local.rs`](../crates/transactions/src/local.rs): advisory-lock local CAS.
- [`history.rs`](../crates/transactions/src/history.rs): atomic acceptance indexes,
  bounded history pages, current-history installation, and retention pruning.
- [`batch.rs`](../crates/transactions/src/batch.rs): compatible member publication
  with one head CAS and individual candidate/error reporting.

See [commit history](commit-history.md) for application-level audit requirements
and [persistence indexing](persistence-indexing.md) for existing world formats.
