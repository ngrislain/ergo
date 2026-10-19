# Concurrent editing of the global shared model

## Status and invariants

This document specifies the **next shared-world collaboration layer**. The current
application publishes new immutable per-owner snapshots and shared built-in
examples. It does not yet implement a mutable global branch, global commit
permissions, distributed leases, or multi-user merging. Do not infer those
guarantees from the presence of S3 persistence.

The collaboration design must preserve four invariants:

1. A committed scene has no unauthorized edits, missing objects, or mixed revisions.
2. Body boxes are overlap-free and the affected mechanical domain is static under
   the declared physics model.
3. Disjoint local changes can proceed concurrently; ancestor/descendant or
   boundary-dependent changes cannot evade conflict detection.
4. A stale, expired, or superseded lock holder cannot publish as the current holder.

The server, including requests originating from an MCP client, enforces these
invariants. A browser or agent cannot grant itself global publication permission.

![Octree-region leases and conditional global publication](figures/concurrent-editing.svg)

## Local working copies versus global commits

An authenticated user may prepare a local construction without being a global
committer. A local working copy may temporarily float or collapse while the user
or agent is building it. The MCP feedback loop should make that visible, not
pretend every intermediate operation is publishable.

A global commit requires an explicit role or region grant, such as a server-side
policy containing `can_commit` and allowed world/region prefixes. Store policies
as private, versioned S3 objects. Google proves identity; it does **not** confer
editing authority. The commit validates the current policy revision, not a role
asserted by the client or cached indefinitely in a session cookie.

Grant/revoke authority should belong to an administrator or explicit owner.
Record the actor, base revision, touched regions, validation report, policy
revision, and commit message in an immutable commit object.

## Address a small edit region with an octree path

Each 1024³ storage block is the root of an independent logical editing octree.
At depth $d$, a cell has side length:

$$
s_d=\frac{1024}{2^d}.
$$

Depth 5 represents a 32³-unit region; depth 10 represents a 1³-unit region. Do not
automatically descend to depth 10 for every edit: a brick or attached group may
span many such cells and produce excessive lock/index overhead.

With child octant $q\in\{0,\ldots,7\}$ encoded as
$q=b_x+2b_y+4b_z$, a path such as `1/6/2` is a stable spatial address inside the
block. The actual geometry can be paged independently of the lock granularity.

The entire brick is an atomic semantic object. Two users editing different
replicas of a boundary-crossing brick are still editing the same ID. Maintain
an authoritative brick version plus replica references; do not allow separate
cell copies to become conflicting sources of truth.

## What must be locked or version-observed?

For a moving set $S$, compute more than the final destination box. The write
footprint includes original bounds, destination bounds, changed constraints, and
every cell reference created/deleted by the move. The collision read footprint
includes the **swept** bounds and nearby candidates.

$$
W\supseteq\operatorname{cells}(B_{start}\cup B_{end}),
\qquad
R\supseteq\operatorname{cells}(B_{swept}\oplus H),
$$

where $H$ is a chosen collision/support halo. Rotational sweeps can be larger
than the union of the start and end AABBs.

A mechanical support chain can cross an arbitrarily large distance; a fixed
spatial halo alone cannot establish gravity stability. Record the versions of
all supporting bodies/constraint edges used during validation, or lock/revalidate
the relevant mechanical island. Spatial edit granularity and physical validation
granularity need not match.

For large object moves, acquire a minimal covering set of octree nodes. Normalize
the set: remove descendants of an already-covered ancestor, and optionally
replace eight fully covered siblings by their parent.

## Hierarchical lock conflicts

Two octree paths conflict when either is a prefix of the other. `2/4` conflicts
with `2/4/1`, not only with another exact `2/4` key. Storing independent S3 lock
objects and checking only exact names is therefore incorrect.

There are two practical approaches:

- A hierarchical intent-lock scheme with shared/exclusive intent along ancestors.
- An authoritative region lock directory that atomically validates all covering
  ranges, including ancestors and descendants, before granting leases.

Exclusive writes conflict with other writes and protected reads on their
overlapping footprints. A lighter alternative keeps only write leases and
records versioned read sets: speculative reads may proceed, but the commit is
rejected/revalidated if an observed dependency changes.

Acquire a multi-region lock set all-or-nothing where possible. Otherwise use a
canonical global ordering `(world, block coordinate, octree path)` and release
everything on partial failure. Do not wait indefinitely holding the first half
of a lock set; it creates avoidable deadlocks and starvation.

## Leases and fencing

A lease contains owner, covered ranges, expiry, base revision, and a monotonically
increasing fencing generation. Renewal happens through the authoritative server.
Use server time, not the browser's clock, for expiration decisions.

Expiry alone is not sufficient. Consider:

1. Editor A's lease expires while its network stalls.
2. Editor B obtains the same zone and starts working.
3. A's old upload resumes and attempts publication.

A must fail even if it still holds old cookie/token material. The generation
must be checked **atomically with publication**, or by a single authoritative
commit coordinator whose lease validation and publication critical section cannot
race a new grant.

Checking a lease object and then separately overwriting the scene head has a
time-of-check/time-of-use race. Likewise, independently CAS-updating several leaf
heads is not an atomic multi-region transaction.

## An object-store-only atomic root design

One correct initial distributed design uses a small conditional coordination root:

```text
global/<world>/coordination.json
  scene_root_hash
  lease_directory_hash
  policy_root_hash
  generation
```

Scene trees, lease directories, policies, pages, and commit records are immutable
objects referenced by hashes. A lease grant/renewal, policy change, or publication
creates new metadata objects and conditionally replaces the coordination root
using the previously observed ETag/version.

During publication the server reads one root, verifies leases/fencing, policies,
and read/write versions against it, prepares the new scene tree, and CAS-writes
a new root derived from that **same** observed state. A conflicting grant,
revocation, or commit changes the root ETag and makes this attempt fail. On retry,
the server rechecks the invariants; it does not blindly resend the old write.

This serializes a short metadata publication operation, **not** the duration of
every edit. Users can hold disjoint zone leases, build, render, and simulate in
parallel. The root CAS is still a throughput/availability bottleneck and should
be measured honestly. Immutable persistent trees avoid rewriting the full model
when one leaf changes.

Verify Scaleway's actual conditional-write semantics before implementing this
design. If the endpoint cannot provide the needed strong CAS behavior, use a
single authoritative writer/coordinator. Do not substitute unconditional PUT
and claim distributed mutual exclusion. S3 does not provide a general
multi-object transaction or atomic rename.

## Optimistic conflict detection and rebasing

Let transaction $T$ have read versions $R_T$ and write versions $W_T$. Publication
must ensure every semantically relevant read is still valid:

$$
\forall (id,v)\in R_T\cup W_T,\quad
\operatorname{version}_{current}(id)=v,
$$

unless an explicit safe rebase has recomputed that dependency. A global root
revision change is not necessarily a semantic conflict: an unrelated distant
edit can be merged onto the new root if the transaction's read/write versions
and permissions remain valid.

Changes to the same brick, a connector shared by both edits, a support dependency,
or overlapping swept collision regions must trigger conflict handling. Two
collision-free changes tested independently against the same base can overlap
one another when combined; revalidate their combined destination state.

Do not merge arbitrary transform matrices by numeric averaging. Preserve semantic
commands such as “translate this fixed-connected group,” then recompute the group,
constraints, collision checks, and static validity on the new base.

## Commit protocol

Keep parent-linked immutable committed deltas alongside materialized revisions;
see [snapshots, deltas, and history](commit-history.md). The viewer reads the
current tree directly rather than replaying the entire commit log.

1. **Checkout:** fetch an immutable base revision and relevant octree pages.
2. **Prepare:** obtain region grants/leases and record all read dependencies.
3. **Edit:** make local changes; receive collision and stability feedback.
4. **Stage:** upload new immutable pages/meshes/constraint records. These are not
   globally visible yet.
5. **Validate:** verify identity, current commit grant, fences, versions, whole
   brick ownership, collisions, and the affected complete support domain.
6. **Publish:** CAS the atomic coordination root and reference the immutable commit.
7. **Release:** remove/expire leases and notify subscribers of changed leaf hashes.

If publication fails, retain the local working copy and explain which brick,
region, support, grant, or lease changed. A stale lease is not permission to erase
the newer model. Unpublished staged blobs are later garbage-collected.

## Destruction and collaboration

Real-time sandbox destruction can update many neighboring bodies per frame. It
should not acquire and publish thousands of global leaf changes at frame rate.
Keep it local or in an explicitly hosted simulation session. Once settled,
prepare a batched semantic commit against the relevant base revision and rerun
the static save gate.

MCP tools receive only local editing authority by default. A future `commit_global`
tool must present its proposed changes and invoke the same authenticated commit
service; it must not write manifests or branch heads directly to S3.
