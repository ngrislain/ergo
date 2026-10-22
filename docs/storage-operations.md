# Shared storage: paging, history, discovery, and operations

This page documents the implemented follow-ups to
[shared-world editing](concurrent-global-editing.md). The world remains limited by
storage capacity, not by an aggregate application piece counter. Working-set and
page budgets bound individual operations. Coordination stays optimistic; oversized
mechanical closures are rejected, without boundary-certificate shortcuts or leases.

## Indexed accepted history

Format-3 heads contain a separate immutable acceptance-index root alongside the
commit CID. It is a Merkle map with both CID → revision-entry and sequence →
revision-entry paths. The same conditional head replacement publishes data and
both indexes, so an uploaded but rejected candidate cannot acquire an acceptance
proof. The index is outside `Commit`, avoiding a self-referential content hash.

`was_committed`, `accepted_snapshot`, `revision_entry`, `begin_at`, and paged
history use that index rather than following every intervening parent. Every
download is content-hash verified. Retained materialized snapshots still open
without replay. The index selects accepted revisions; it is not a signature or
permission grant, and REST reads still check current access.

Format-1/2 heads remain readable. A legacy head without an index uses its old
ancestry check until explicitly upgraded:

```sh
cargo run -p ergo-server -- shared-index-history WORLD_UUID
```

This is a one-time streamed metadata job behind the durable namespace barrier.
It does not load world geometry. Interrupted jobs leave the old head intact and
the namespace fenced for explicit recovery.

## Opt-in retention

Retention defaults to **all history**. Only a world owner may configure it:

```text
PUT /api/shared-worlds/{world}/retention
{ "base": "ACCEPTED_CID", "keep_last": 1000 }
```

Use `null` to disable future pruning; zero is invalid. Changing the policy does
not immediately erase revisions. Run explicit maintenance:

```sh
cargo run -p ergo-server -- shared-prune WORLD_UUID
cargo run -p ergo-server -- shared-gc WORLD_UUID
```

`shared-prune` reads the current owner-configured policy while holding the barrier,
removes older index entries in bounded batches, and atomically publishes the new
retention boundary. It keeps at least the current revision. Increasing the count
later does not restore pruned history. Pinned readers, in-flight writers, and
uncertain publications prevent maintenance from acquiring the barrier.

Garbage collection then treats retained index entries as history roots. Old
`parent`/`base` IDs remain provenance values, not instructions to retain the pruned
past forever. The shared collector traces current/retained piece records, birth
records, meshes, model objects, assembly defaults, joint endpoints, and audit
policy/model/identity links. Removing the original import commit therefore does
not delete a still-live piece's birth, geometry, or physics model. Arbitrary hashes
inside messages are not interpreted as links. Generic library users must supply
their own schema link callback before pruning application history.

Pruned or unaccepted revision reads return HTTP 410 through the retained-history
API. `was_committed` returns false when the ID is absent from retained history;
after pruning it is not an eternal proof that an operation never happened. Resolve
uncertain operations before allowing their retained receipts to expire. Pruning
cannot bypass the existing stopped-holder/settled-request recovery requirements.

## Dense block-local spatial pages

New worlds use spatial layout 2. Root `5` is an allocated-block directory; root
`1` contains adaptive block-local octrees. At each local cell, `0` indexes the
cell's own references by piece CID, and `1/q` indexes a child octant. Up to ten
local spatial levels subdivide a 1024³ block. A piece is placed in the smallest
cell covering its clipped render/body bounds, with one reference per intersected
storage block. Large pieces do not expand into thousands of fine-cell replicas.

Each reference contains coordinate, stable piece ID, and versioned bounds.
Neighborhood validation visits intersecting local cells and their ancestor-owned
references. It checks bounds before loading authoritative geometry. A directory
presence predicate allows unrelated descendant updates without observing an entire
populated subtree, while missing directories and finer spatial scans still detect
phantoms. Piece versions and adjacency reads complete the dependency check.

Block listing returns small `{coord, paged}` summaries, not an unbounded ID array:

- `GET /api/shared-worlds/{world}/blocks?revision=CID&limit=64&after=PATH`
- `GET /api/shared-worlds/{world}/blocks/{x}/{y}/{z}?revision=CID`
- `GET /api/shared-worlds/{world}/blocks/{x}/{y}/{z}/pieces?revision=CID&limit=64&after=PATH`

The piece page returns spatial-reference rows and an exclusive `next` cursor.
Use the same revision for every page and subsequent piece/mesh load. Allocated
block headers can remain after their last piece moves away; such a block has an
empty piece page. This is conservative directory metadata, not phantom geometry.

Legacy layout-1 worlds remain readable and editable through their bounded legacy
path. Upgrade an existing world in a single final root publication:

```sh
cargo run -p ergo-server -- shared-upgrade-spatial WORLD_UUID
```

The migration streams authoritative records in bounded batches. It stages the
derived index and updates the layout marker only at the final CAS. A failed or
interrupted batch never exposes a half-converted world; retained older roots keep
their original layout.

## Assembly metadata pages

New assembly records are directories containing a small `{id,name,meshes,paged}`
header plus independently indexed membership, adjacency, per-piece joint, anchor,
group-header/member, and joint-definition rows. An ordinary pose edit reads its
member/adjacency rows instead of decoding every member in a large assembly.

`GET /api/shared-worlds/{world}/assemblies/{assembly}` returns the paged header.
Fetch rows with:

```text
GET /api/shared-worlds/{world}/assemblies/{assembly}/pages
    ?revision=CID&kind=1&limit=64&after=PATH
```

Kinds are `1` membership, `2` adjacency, `3` per-piece joint bindings, `4` fixed
anchors, `5` group headers/members, and `6` joint definitions. `group=LOCAL_ID`
narrows kind 5 to one group. Pages include row paths, values, and `next`.

A whole-assembly `RulesEdit` remains a bounded replacement operation: it validates
the complete requested rule set and rejects oversized metadata rather than
silently dropping references. Legacy rule blobs are supported; upgrade them with:

```sh
cargo run -p ergo-server -- shared-upgrade-rules WORLD_UUID
```

The library allows paths up to 192 octants for composed coordinate/hash/member
indexes, packs unary paths, and bounds a single page walk to 100,000 visited nodes.
These are operation/index-address bounds, not world-size counters. Narrow a query
or compact its region when a traversal budget is exceeded.

## Cross-assembly joints

`POST /api/shared-worlds/{world}/commits` accepts an optional `joints` array in
addition to piece and assembly-rule edits. A new joint has `id: null`; an update
or removal names the stable joint ID. Example value:

```json
{
  "id": null,
  "value": {
    "a": "PIECE_A_CID",
    "b": "PIECE_B_CID",
    "anchor": [1.0, 2.0, 3.0],
    "kind": "fixed"
  }
}
```

Use `value: null` to remove a named joint. New IDs hash immutable joint-birth
records, and `Accepted.joint_ids` reports the created/changed IDs. World root `6`
stores joint versions and root `7` stores paged per-piece incident references.
Endpoints, references, and geometry publish together. Both old/new endpoint
footprints require write permission; both ends enter the complete dependency
closure. The frozen server-owned connector measurements are still mandatory.
There is no invented strength for an unmeasured connection.

- `GET /api/shared-worlds/{world}/joints/{joint}?revision=CID`
- `GET /api/shared-worlds/{world}/pieces/{piece}/joints?revision=CID&limit=64&after=PATH`

Deleting a piece requires removing its incident joints in the same or an earlier
transaction. A new joint invalidates a prepared geometry edit's incident-directory
predicate, even if the directory was empty before. Cross-world endpoints are not
accepted. Model migration and unloaded-region certificates remain future work.

## Discovery catalogs

Shared discovery is paginated:

```text
GET /api/shared-worlds?limit=32&cursor=OPAQUE_CURSOR
GET /api/shared-worlds?quad=0123&limit=32&cursor=OPAQUE_CURSOR
```

Without `quad`, the catalog selects the signed-in user's owned/granted worlds.
With it, a hierarchical Web-Mercator quad-prefix directory selects allocated
geographic blocks. Quadkeys have up to 12 digits `0..3`; an empty prefix covers
the geographic directory. Block-footprint corners advertise intersected tiles;
latitude is clamped to the Web-Mercator projection domain. Entries describe
allocated regions, not a claim that a nonempty mesh currently occupies each cell.

Indexes are pre-advertised **before** world, grant, or first-block publication.
Thus a failed write can leave a harmless stale entry, but a successful write does
not depend on an unrecorded post-commit advertisement. Every result checks current
world authorization and allocated-block existence. Revoked users do not keep access
through an old catalog entry. Pages bound scanned candidates/regions, so a page
may be empty with a non-null `next`; continue until it is null. User/region cursor
scopes are checked. These are component snapshots of a derived discovery index,
not a multi-world transactional snapshot.

Private snapshot discovery supports `GET /api/worlds?limit=32&cursor=CURSOR`,
returning bounded summaries rather than entire manifests. The no-query legacy
route remains compatible with the current private-world UI. Index pre-advertising
is gated by manifest existence before a summary is returned.

Backfill pre-existing data explicitly:

```sh
cargo run -p ergo-server -- shared-rebuild-catalog WORLD_UUID
cargo run -p ergo-server -- private-rebuild-catalog OWNER_ID
```

Backfills stream metadata/directories, not a world-sized geometry export. Primary
worlds/policies remain authoritative if an index entry is stale. Catalog namespaces
are independent of world GC; accepted-history pruning does not erase discovery.

## Revision subscriptions

`GET /api/shared-worlds/{world}/events` is an SSE stream. `after=CID` or
`Last-Event-ID` supplies the last observed revision. Notifications include revision,
sequence, retention floor, and a `coalesced` flag. Slow subscribers receive the
latest state, not an unbounded event queue. A pruned/unknown resume point produces
`reset`; use paged history for complete retained details.

One poll per actively watched world **per server process** feeds a shared bounded
watch channel. `ERGO_SHARED_EVENT_POLL_MS` defaults to 1000 (range 50–60000).
Snapshots close after each poll, so open SSE clients do not permanently pin GC.
Connections recheck session expiry and observed policy; revocation ends the stream.
Storage/maintenance outages emit `unavailable`, rather than leaking stale geometry.
Notifications contain no geometry. Authoritative payload reads always perform
their own current-access check.

## Optional group publication and benchmarks

Set `ERGO_SHARED_GROUP_COMMIT=1` after evaluating the actual workload. Default is
off. Per-world queues hold at most 32 prepared requests, coalesce for 2 ms, and
publish up to 16 compatible members per batch; overflow returns HTTP 429. Idle
workers retire. Validation remains per request. The library serially checks each
member's reads against the accumulated candidate state, rejects conflicting
members, uploads their separate roots/commits/audits, and publishes compatible
members and their acceptance entries with one head CAS.

Every successful member retains its own revision and audit. Intermediate member
states are historical snapshots; live head readers observe the whole compatible
group at once. Lost acknowledgements report each member's own candidate CID, not
some other request's success. Generic `commit_batch` submissions must be writes
or contain metadata/attachments; use ordinary `commit` for read-only snapshots.

```sh
cargo run -p ergo-transactions --example concurrent_bench -- \
  --writers 32 --shards 1 --edits 2 --payload-bytes 4096 --group-size 16
```

Group mode stages waves before publication; its arrival schedule differs from the
independent-writer benchmark. JSON separates logical head-CAS calls from summed
member attempts; provider-internal HTTP retries are not counted separately.
In-memory runs validate the harness and grouping,
not Scaleway latency or throughput. Live verification still requires exported S3
bucket, endpoint, and credentials, using the existing isolated test/benchmark prefixes.

## Live Scaleway verification — 2026-10-22

The unmodified `v0.2.0` transaction implementation was exercised against the
regional `https://s3.fr-par.scw.cloud` endpoint with private local credentials:

- Bucket HEAD and isolated-prefix LIST returned HTTP 200.
- The opt-in live test passed five 16-writer conditional-write races, each with
  exactly one winner and immediate winning-value visibility. Independent-edit
  rebasing and old materialized snapshot reads also passed.
- An independent benchmark with four writers, two shards, two edits per writer,
  4 KiB payloads, and coordinated GC admission verified the final eight-leaf count.
- A grouped benchmark with four writers, one shard, groups of four, the same edit
  count/payload size, and coordinated admission also verified eight final leaves.
  Group publication used two logical head-CAS calls.

The benchmark checks leaf counts rather than decoding every stored payload; the
live transaction smoke test separately reads and verifies its scalar CBOR values.
These are bounded functional smoke runs, not scale qualification. Observed commit
latency was several seconds; the tiny sample and differing arrival/shard profiles
do not establish maximum throughput or a grouping speedup. Live physical deletion,
retention pruning, garbage-collection recovery, fault injection, and bucket
versioning/delete-marker behavior were not exercised by these runs.

Test objects were retained for inspection under:

```text
ergo-transaction-tests/92605b1a-ab18-42d4-8be9-291642284a5b/
ergo-transaction-bench/5216338c-4f4e-4044-9bcd-f9fd0cfad09c/
ergo-transaction-bench/addf681b-7258-464a-a6d3-16bf69f13e1e/
```

The earlier denied attempt at
`ergo-transaction-tests/d1d54813-d48a-4bdd-ba3e-93838bad399c/` stopped at its first
PUT with HTTP 403; it did not reach a transaction. A bucket-specific endpoint was
corrected in the ignored local configuration before testing. No credential values
are recorded here, and the `v0.2.0` tag is not changed by this verification record.
