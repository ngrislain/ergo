# Snapshots, committed substitutions, and history

## Implemented records

The [transaction library](transactional-storage.md) stores immutable parent-linked
commits alongside a materialized current octree. The [shared-world REST gate](concurrent-global-editing.md)
adds verified actor identity, policy/model references, dependency footprints, and
static validation certificates. Private `/api/worlds` snapshots are still separate
imports/saves without a mutable shared branch.

There are three distinct records:

- **Working-copy operations:** local authoring, experiments, drag samples, and
  simulation output. They are not accepted shared-world changes.
- **Committed substitutions:** ordered exact subtree replacements/removals with
  generation fences, relative to an accepted parent. `base` preserves the original
  execution revision when independent edits were rebased onto a newer parent.
- **Snapshot/checkpoint:** the materialized root referenced by each accepted commit.
  Unchanged geometry/index objects are shared by content hash.

An explicit application-level semantic modeling log is not implemented. In
particular, the library does not infer how arbitrary transformations or boolean
modeling commands should commute inside the same leaf.

## Reads and reconstruction

If $D_i$ is the accepted exact substitution sequence, conceptually:

$$
S_n=\operatorname{apply}(D_n,\ldots\operatorname{apply}(D_1,S_0)\ldots).
$$

Normal readers never perform that replay. They use:

$$
\text{head}\rightarrow\text{commit}\rightarrow\text{materialized root}
\rightarrow\text{spatial/identity pages}\rightarrow\text{objects}.
$$

`snapshot(commit_id)` directly opens a historical root. Packed unary edges avoid
one object per address digit. The world need not fit in RAM; spatial pages and
individual global piece records are loaded on demand. Shared-world point reads
and pages accept a revision so a view can retain one consistent snapshot.

Piece **identity** is a hash of its immutable creation record, not its current
pose. Current piece versions and meshes remain independently content-addressed.
An undo/revert service could publish selected prior values as a new commit;
rewriting the branch head or old audit history is not an implemented undo API.

## Audit metadata

A library commit contains format, transaction ID, sequence, parent, execution
base, root, substitutions, optional audit metadata CID, explicit blob attachments,
actor, and message. Library actor/message values are untrusted caller metadata.

The shared REST service fills audit records from authenticated server execution:

- Verified actor and accepted execution base.
- Policy content hash and frozen physics-model CID.
- Changed and affected global piece identities.
- Watched spatial predicates and charged working-set sizes.
- Static certificate, tolerances, and calibration description when geometry remains
  in the affected domain.

Creation records, server-normalized meshes, and model objects are explicit commit
attachments. Even values superseded by a later substitution in the same commit
remain uploaded because the ordered record references them. Acceptance never
requires replaying a nondeterministic dynamics simulation.

`GET /api/shared-worlds/{id}/history` returns bounded commit/audit pages after
checking current access. Format-3 heads atomically publish a Merkle acceptance
index with CID and sequence lookup, avoiding ancestry scans. Current-format heads
missing their acceptance index require explicit fenced history installation. SSE coalesces revision notifications and reports resets; it
does not retain an unbounded queue or pin every old snapshot. See
[storage operations](storage-operations.md).

## Bounded native commit records

Implemented in **unreleased source after v0.5.34**. Canonical format-3 bytes,
commit IDs, list order, roots, base/parent relationships and acceptance semantics
are unchanged. A byte-for-byte reference test compares the streamed envelope to
the ordinary canonical encoder. No accepted-world data rewrite is required.

Native commit assembly consumes the disk edit/attachment journals one record at
a time into a private sealed file, with definite array lengths and the same
canonical map-key order. Snapshots retain small headers and file-range list views,
not complete decoded arrays. Downloads stream to disk, verify remote size/hash,
then verify the file and validate its typed records before exposing a snapshot.
A bounded CBOR size/depth scan precedes typed decoding, so malicious length hints
cannot cause a history-sized allocation. Defaults are a 128 MiB encoded record,
1 MiB per scalar header/list item, 64 header fields/bytes per field name, and
nesting depth 64. See [resource configuration](native-working-state.md).

List iterators have independent file cursors, preserve exact order, and return
I/O/decode/length/checksum errors rather than silently ending early. GC consumes
these same fallible lists during its complete marking pass; any error stops before
sweeping. Snapshot clones and namespace-qualified metadata cache hits share the
verified file. With repository caching enabled, a separate file cache retains at
most 32 records, at most the lesser of the ordinary cache capacity and 128 MiB of
encoded files, and approximately 1 MiB of retained-header accounting. This avoids
downloading a large edit log again for every sequential piece page. Up to 1,024
weak entries support active concurrent readers; disabling ordinary caching also
disables strong file retention. This cache never caches mutable heads, pins remote
history, supplies publication proof or bypasses tighter read budgets. Active
snapshots/iterators can retain files outside the cache's own limits.

Rust API migration in this unreleased source:

- `Commit.edits` and `Commit.blobs` are `CommitList<T>`, with `len`, `is_empty`,
  and fallible `iter`/items. Handle both iterator creation and each item error.
- Use `Repository::read_commit` or `snapshot`, not generic `download::<Commit>` or
  standalone `Deserialize`. `read_commit` is metadata access, not acceptance or
  retention proof; use a pinned snapshot for retained geometry reads.
- Construct metadata with `CommitHeader` and `Commit::from_records`, supplying
  exact counts and fallible record iterators. Constructor count/resource errors
  perform no publication. `encoded_len` and `content_id` describe the original
  sealed bytes, not a commit receipt.

All repository publication, batching, maintenance and GC callers use this one
reader/writer pipeline. Existing public JSON history arrays remain arrays; their
bounded JSON response buffer is not an out-of-core transport guarantee.

The object-store SDK has no conditional multipart-create API. Publication therefore
retains **one capped encoded buffer** for `PutMode::Create`, instead of switching
to unconditional overwrite. A batch uploads its commit files sequentially and
limits their aggregate encoded size. Commit bytes are not retained in the ordinary
verified byte LRU. Immutable-upload existence/content reconciliation now hashes
uncached remote streams without collecting another complete proof buffer. All
referenced objects and per-member commits still upload before the existing single
head CAS; cancellation/lost-acknowledgement handling and durable pins are unchanged.

Temporary files follow `TMPDIR`, are private/disposable, and live until the final
metadata/list-iterator handle drops. They are not durable authoring checkpoints,
and file existence never proves publication. Sources:
`crates/transactions/src/{commit,lib,batch,maintenance,encoding}.rs`.

## Publication and uncertain outcomes

Upload all referenced immutable objects first, then conditionally replace one head
using its observed ETag/version. The shared gate's policy, identity, spatial, and
rule reads participate in the same transaction. A conflicting revocation or
dependency change prevents publication.

An uploaded commit is not proof of acceptance. A lost acknowledgement can leave
an unknown outcome; `was_committed` checks accepted parent history. SDK retries
that surface a 412 after hidden success are reconciled before another publication
attempt. A negative history check is not proof that a timed-out request has settled.
In coordinated mode, cancelled/uncertain root mutations retain their durable pin.

## Retention and maintenance

Every accepted revision is already a checkpoint, so viewing does not depend on
periodic event replay. History defaults to keeping all revisions. An owner can
configure keep-last-N; explicit maintenance prunes older acceptance entries before
collection. At least the current revision remains accepted. Pruned historical
access returns HTTP 410, and increasing retention does not restore deleted history.

The implemented collector removes unreachable staged data behind an exclusive
namespace maintenance barrier. Durable reader/writer pins prevent collecting
active bases or in-flight publication objects. No pin or barrier expires by age.
The collector traces heads, retained acceptance entries, roots, edit values,
audit metadata, attachments, and schema links. Legacy unindexed heads still trace
parent/base history; indexed branches do not retain pruned provenance forever.
Current birth/mesh/model links survive removal of their import commit.
Inventory is streamed; the mark set and frontier live in temporary
sharded disk files rather than a world-sized RAM collection.

Regional tombstone compaction is separately budgeted. It publishes a new root
without changing retained historical roots. A checkpoint is never permission to
delete history or the blobs needed by a historical reader. Failed/cancelled
maintenance remains fenced until the holder and outstanding requests are settled
and an operator explicitly recovers it.

Group publication retains a separate commit/root/audit per compatible member,
with one final head CAS. Live readers see the final group state; intermediate
member states remain accessible through retained indexed history. Conflicting
members fail independently. Each uncertain member receives its own candidate CID,
not some other request's success. See
[group publication](storage-operations.md#events-and-grouped-publication).

Cache TTLs, ground imagery retention, and source-asset cache expiry do not dictate
accepted shared-world history. Bucket-wide age expiry on content-addressed objects
is unsafe. See [maintenance operations](concurrent-global-editing.md#retention-and-maintenance).

## Related systems

Git is an analogy for immutable snapshots, parent-linked commits, shared objects,
and branches; physical delta compression is a separate optimization. Ergo does not
currently implement binary delta chains or packfiles.
[Apache Iceberg](https://iceberg.apache.org/spec/) is useful related reading on
snapshots and immutable-file references, but is not an implementation dependency.
