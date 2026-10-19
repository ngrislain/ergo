# Snapshots, committed deltas, and history

## Recommendation and current status

Retain immutable committed deltas alongside snapshots, with a materialized current
root for fast reads. A viewer must not replay an unbounded event history to draw
the current world.

This document specifies the **planned global history layer**. The current server
creates immutable private snapshots whose blocks/meshes share content-addressed
objects. It does not yet record parent-linked semantic deltas or expose a mutable
global branch head. See [concurrent editing](concurrent-global-editing.md).

## Three different records

- **Working-copy edits:** transient or locally persisted authoring operations,
  including temporarily unstable arrangements. They support interactive feedback.
- **Committed delta:** an immutable, authenticated record of an accepted change
  relative to a particular base revision.
- **Snapshot/checkpoint:** a materialized revision selecting complete current
  leaf, geometry, group, and constraint state.

Do not write every drag sample, physics timestep, earthquake frame, or rendered
frame to global history. A simulation produces an experimental working result;
only an explicitly accepted, revalidated static result becomes a commit.

## State and replay

For checkpoint state $S_c$ and deltas $D_{c+1},\ldots,D_n$:

$$
S_n=\operatorname{apply}(D_n,\ldots\operatorname{apply}(D_{c+1},S_c)\ldots).
$$

This defines recovery and historical reconstruction. Hot reads should instead
follow the current materialized tree root directly:

$$
\text{branch head}\rightarrow\text{current root hash}
\rightarrow\text{visible leaf hashes}\rightarrow\text{objects}.
$$

A commit can produce new immutable versions of only changed octree leaves and
their ancestors. Unchanged subtrees and geometry remain shared by hash. Therefore
maintaining a materialized current root does not require copying the whole world.

## What a committed delta should contain

- Commit ID/hash, schema version, parent/base revision, and optional merge parents.
- Verified actor ID, timestamp, message, policy revision, and lease/fencing data.
- Stable affected piece/group/constraint IDs and the operation's spatial footprint.
- Preconditions: expected prior object versions or leaf hashes.
- Semantic authoring operations where useful: create/remove a piece, change group
  membership, rigidly transform a group, or edit a connector.
- Authoritative before/after leaf hashes and metadata changes, so replay does not
  depend on reproducing a nondeterministic physics simulation.
- The server validation report, tolerances, and physical-data references.

Semantic commands help human review, undo, and rebasing. Materialized before/after
state is still important: replaying a spring solver or GPU simulation years later
must not be required to recover the accepted poses.

## Atomic publication

Upload changed data, the immutable delta, and materialized metadata first. Then
conditionally update the authoritative coordination root against its observed
ETag/version, with permission, dependency, and fencing checks against that same
state. Readers either see the old revision or the complete new revision.

An uploaded delta that was never published is staged garbage, not a committed
change. Neither a delta file nor several independently replaced leaf heads
provides an S3 multi-object transaction. The exact endpoint CAS behavior must be
verified before claiming the distributed guarantees.

## Checkpoints and retention

Create checkpoints based on measured replay work—operation count, changed bytes,
and recovery time—rather than a hard-coded number chosen without data. Current
viewing remains independent of checkpoint frequency because it uses the latest
materialized root.

Keep committed history initially, but define retention separately from
checkpointing. A checkpoint is not permission to erase the audit trail or objects
needed by older reachable revisions. Garbage collection must trace retained
branches, tags, checkpoints, commits, and their referenced blobs, with a grace
period for in-flight publication.

Map-source caches and failed staged uploads have different lifecycles from user
commit history. Cache TTLs should not dictate the retention of accepted user work.

## Related systems and distinctions

Git is useful as an analogy for content-addressed snapshots, parent-linked
commits, shared immutable objects, branches, and tags. Conceptually Git commits
reference snapshots; packfile delta compression is a separate storage optimization.
Ergo should likewise avoid confusing an audit delta log with physical compression.

[Apache Iceberg](https://iceberg.apache.org/spec/) provides related reading on
snapshots, manifests, and immutable data-file references. Its catalog and table
semantics are not an implementation dependency or a claim that Ergo is a data
lake. The intended collaboration protocol remains an object-store-only design
with a deliberately small authoritative coordination operation.

Optimistic read/write validation and leases are discussed in the annotated
[concurrency references](references.md#concurrent-object-store-editing). They
explain why saved deltas alone do not solve concurrent conflicting edits.
