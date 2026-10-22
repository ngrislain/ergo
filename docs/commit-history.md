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
index with CID and sequence lookup, avoiding ancestry scans. Legacy heads can be
explicitly upgraded. SSE coalesces revision notifications and reports resets; it
does not retain an unbounded queue or pin every old snapshot. See
[storage operations](storage-operations.md).

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
[group publication](storage-operations.md#optional-group-publication-and-benchmarks).

Cache TTLs, ground imagery retention, and source-asset cache expiry do not dictate
accepted shared-world history. Bucket-wide age expiry on content-addressed objects
is unsafe. See [maintenance operations](concurrent-global-editing.md#retention-and-maintenance).

## Related systems

Git is an analogy for immutable snapshots, parent-linked commits, shared objects,
and branches; physical delta compression is a separate optimization. Ergo does not
currently implement binary delta chains or packfiles.
[Apache Iceberg](https://iceberg.apache.org/spec/) is useful related reading on
snapshots and immutable-file references, but is not an implementation dependency.
