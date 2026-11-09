# Paged construction authoring

This is implemented in **v0.5.35 source**. Deploy matching rebuilt server,
MCP and viewer assets together. Old whole-workspace `PUT` requests are rejected;
there is no compatibility save path.
`/api/config` advertises `authoring_format:"ergo-workspace-2"` and
`edit_format:"ergo-edit-1"`; a mismatched MCP client stops before opening caches.

## Storage, pages and physics

The existing canonical octree stores piece identities, poses, spatial references
and assembly/rule records. A page is a bounded response from an accepted snapshot,
not necessarily one octree node. A mechanical island is a contact/joint-connected
domain: neither pages nor storage cells are artificial ground supports.

`GET /api/shared-worlds/{world}/assemblies/{assembly}/workspace` returns an
`ergo-workspace-2` **manifest**, not a scene. It contains the accepted revision,
name, geographic origin, stored piece-count summary, next available assembly-local
ID and limits. Shared mesh identities arrive with the selected page, not a
whole-assembly mesh map. `HEAD`/ETag checks authenticate first;
tokens bind manifest, actor, configured budgets and reader version.

## Reading bounded views

`GET .../workspace/pages?revision=CID&limit=128` enumerates membership order.
Optional `bounds` is JSON `[minimum,maximum]` in public east/north/up units;
it queries allocated cells and their canonical spatial-reference octrees.
Alternatively, `ids` is a JSON array of 1–128 distinct local IDs per request. Choose IDs
or bounds, not both. A `cursor` continues the same actor/world/assembly/revision
and selection; a changed revision/query is rejected rather than mixed.

Pages contain posed `bricks`, transport `versions`, shared `mesh_ids`, count
summary, `next`, `partial:true` and `external_dependencies:true`. Each has at most
512 pieces and 2 MiB encoded JSON. At most 2,048 reference/member/directory records
are examined per request: an empty page can still have a continuation. Boundary
replicas are deduplicated by the first query-intersecting replica in canonical
block order. Pieces remain whole with their existing identities.

Reads are revision-pinned, not durable long-lived sessions: current authorization
and accepted-history availability are rechecked on every request. If retention
removes the revision, stop and reopen; never silently upgrade the page's base.
Cursors are read continuations, not physical-completeness certificates.

`GET .../workspace/meshes/{mesh}?revision=CID&piece=LOCAL_ID` retrieves shared
geometry after checking permission, historical acceptance and that exact member's
mesh reference, without reconstructing an assembly-wide mesh dictionary.
The MCP working view retains at most 2,048 pieces and 32 MiB materialized JSON,
including meshes. It does not automatically load later pages.

### MCP workflow

1. `open_shared_construction(assembly_id=...)` opens only a manifest and clears
   resident geometry. Read `total_pieces`, not the resident `brick_count`, for
   the assembly's size summary.
2. `read_construction_page(bounds=[[...],[...]], limit=128)` loads a spatial view,
   or select `ids=[...]` for the exact existing members an atomic edit replaces.
3. Continue with `cursor`; `append:true` retains prior pages within the resident
   budget. Otherwise the new page replaces the working view.
4. Inspect/edit loaded pieces with existing tools. Shared edits submit sparse
   deltas and preserve unloaded members. New local IDs start above the highest
   live member at the manifest revision; concurrent collisions reject.
5. Reopen/reload after conflicts or uncertain outcomes. A cursor for a preceding
   accepted revision cannot be appended to a new revision's working view.

Local inspection, rendering and building feedback describe the **loaded view**;
replies disclose that scope. Full local validation/dynamics are refused for bound
partial views: omitted neighbours and unknown boundary loads cannot be assumed
absent. `move_brick` and `rotate_about_pivot` are also refused: their implicit
connected-side selection cannot silently omit unloaded neighbours or load-sharing
loops. Explicit complete loaded-group edits remain available, with server checks.
The server still validates the complete affected old/new mechanical domain.

Posed-piece pages alone do not populate all group/joint metadata. In shared mode,
`list_groups(cursor,limit)` pages authoritative group headers without geometry;
`read_construction_group(group_id)` explicitly replaces the view with a complete
descendant group closure, its pieces, internal explicit joints and anchors. This
is authoring completeness, not physical isolation. Unloaded/global constraints
fence transforms; reparenting to an external hierarchy requires loading a common
ancestor. Empty loaded geometry does not mean the assembly has no groups.

Group closure reads are limited to 512 groups, 4,096 membership records, 16,384
membership/constraint records total, 2 MiB metadata and the existing piece/geometry
residency limits. Oversize/cyclic closures fail without publishing or marking a
partial closure complete. The manifest supplies a separate `next_group_id` floor,
so creating a group cannot collide with an unseen high-numbered group.
Existing group changes require a full old-definition hash with set-ordered
members. Dissolving a loaded child also splices its members into any unloaded
authoritative parent; concurrent rule changes still conflict.
Removal writes one group-directory tombstone, retaining its transaction conflict
fence without a headerless directory entry. Record-level tombstones already in
the current tree format are absent groups only after a bounded scan proves no
live records remain. Orphaned live membership without a header remains an error,
not a silently skipped group. A header page can therefore be empty with a cursor
when its directory candidates contain only verified deletions.

## Sparse writes and atomicity

`PATCH .../workspace` accepts `{base,delta}`. The delta has explicit
`format:"ergo-edit-1"`, `schema:2` and these operations:

- `upsert`: new/changed materialized bricks only;
- `remove`: explicitly removed local IDs, never inferred from an absent page;
- `expected`: returned versions for every edited/deleted existing piece;
- `meshes`: new/changed geometry only; stored assembly meshes resolve server-side;
- optional `name`, joint upserts/removals, anchor additions/removals and group
  upserts/removals; existing group edits require `expected_groups`.

Piece versions are SHA-256 of the ordered JSON `Value` representation of a posed piece
record (`ergo_world::authoring::version`), distinct from birth IDs and canonical
CBOR storage hashes. `Delta::between` is shared by native/browser clients.
The server independently checks poses, genuine geometry, frozen physics,
permissions and quota. A client-provided hash is never authorization.

`PATCH /api/shared-worlds/{world}/assemblies/workspaces` accepts
`{base,assemblies:[{assembly_id,delta},...]}` for 1–32 assemblies. Discover old
dependencies before staging final state, validate all members, then publish once.
Budgets are aggregate. Receipts map **upserted** local IDs to global births, not
every unchanged member. Removed pieces lose internal joint/anchor/group references;
external joint references cannot dangle. Stale full-scene model hints are cleared;
native export reconstructs a model from canonical pieces.

Bound MCP files use an explicit `ergo-authoring-cache-1` envelope with
`partial:true`, containing only the resident native construction. They are not
standalone complete-construction files and cannot accidentally be decoded as
such. The sidecar contains `ergo-workspace-binding-2` binding/version metadata.

`export_construction` on a bound MCP workspace explicitly exports the complete
accepted assembly to a separate `.export.ergo.json` path returned in its result.
It does not overwrite the resident cache or publish. Recipes must use that path,
not treat the page cache as the whole model.

## Forward migration

Existing canonical construction files, common constructs, accepted identities,
world roots and geometry need **no rewrite**: only authoring transport and caches
change. This is not a new part/model/construction geometry storage format.

1. Stop MCP clients and reconcile all pending publication outcomes.
2. Deploy matching server/native/WASM in a coordinated maintenance window. Update
   REST consumers from full `PUT {base,scene}` to sparse `PATCH`.
3. Dry-run each old bound cache against the upgraded service:

   ```sh
   python3 scripts/migrate-authoring-cache.py /absolute/path/agent.cache.json \
     --token-file /absolute/private/token
   ```

4. Inspect the revision/manifest and repeat with `--apply`. The script takes the
   normal workspace lock, refuses pending/unbound proposals, retains an archival
   backup, writes an empty working view and commits the new
   `ergo-workspace-binding-2` sidecar last. It sends no shared mutation. Normal
   startup rejects old sidecars; there is no fallback reader.
5. Restart and explicitly load bounds/IDs. Repository recipe helpers
   `Mcp.load_piece_ids` and `Mcp.export_path` make this separation explicit.
   Incremental district/park builders release resident stages after acceptance;
   district batches default to one storey. Complete render/walking review uses
   an explicit accepted export in a separate local inspection client, never a
   whole-model authoring fallback or an alternative publication path.

Never migrate a standalone proposal as an accepted cache. Against an old server
the script refuses to write because the manifest format is absent. The backup
is an archive, not a second active cache or authoring path.

## Limits of this migration

Paging, sparse upload and bounded complete group loading are implemented here.
The [coupled interface-force solver](partitioned-stability.md) now bounds local
SOCP factorization and independently checks one assembled witness body-by-body.
[Native working-state spilling](native-working-state.md) now externalizes staged
objects, predicates, edit journals, dependency indexes/frontiers and rule-diff rows.
Durable authoring sessions and asynchronous publication remain unimplemented.
Shared-native geometry uses disk-backed normalized bodies and paged spatial
candidates, not a complete affected pose vector. Constraint collections, component
membership, compression state, canonical solver inputs and planner/verifier indexes
spill too. Assembly rules, cached geometry and affected-ID/audit sets can remain
resident under their existing budgets; local/full exports explicitly materialize.
Native solver plan/interface records, reaction state and the piece cache now
spill through [the paged workspace](partitioned-stability.md#paged-coordinator-state).
Commit
records now stream through verified private files and fallible lists; conditional
upload and explicit HTTP JSON retain bounded output buffers. Do not advertise
bounded end-to-end process memory from these local
solver/scratch budgets.

Sources: `crates/world/src/authoring.rs`,
`crates/server/src/shared/{authoring_pages,edits}.rs`, `crates/mcp/src/shared.rs`,
and `scripts/migrate-authoring-cache.py`.
