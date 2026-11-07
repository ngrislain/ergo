# Paging, history and maintenance

All hosted operations target the one configured Sandbox. Current storage requires
transaction format 3, indexed accepted history and spatial layout 4 with 128-unit cells.

## Bounded reads

Shared APIs page assemblies, allocated blocks, spatial references, rules, pieces,
joints and history. Cursors are ordered/exclusive application paths or accepted
revision identifiers; callers pass returned cursors back unchanged. Each request
rechecks current authorization. Dense domains exceeding the byte/piece budgets
fail explicitly.

`GET /api/shared-worlds/{id}/assemblies/{assembly}/location` reads one current
membership row and Piece, returning metadata/revision and a public position hint.
It exports no isolated model. The browser loads the shared region and focuses
resident assembly bounds there.

## Accepted history

The head stores Commit CID and the acceptance index root/retained sequence floor.
The index maps both sequence and CID to `{revision,sequence}`. History pages are
newest-first. Staged-but-unaccepted objects are not valid publication receipts.
Each accepted Commit selects a materialized root; viewing does not replay history.

Owner-controlled `keep_last` is optional; null disables pruning and zero is invalid.
Pruning is an explicit maintenance operation. It changes retained acceptance
membership, not the current world geometry. Schema-linked collection retains live
births, meshes, models, policy, audits and selected history.

## Events and grouped publication

The SSE endpoint coalesces bounded notifications, supports resume/reset and
rechecks permissions. Subscribers do not hold a permanent snapshot pin; a process
shares head polling among subscribers. Slow/revoked streams terminate safely.

Opt-in bounded group publication batches compatible independent transactions into
one conditional head update while retaining individual Commit/audit/uncertain
outcome records. Functional behavior is tested; deployment throughput is measured
with the repository benchmark, not inferred from paging or batch size.

## Coordinated maintenance

Durable pins protect active readers/writers. An exclusive barrier requires no
active pins. Pins and barriers never expire merely with time. Cancellation or an
ambiguous mutation leaves fencing in place until the holder/remote requests are
settled and an operator explicitly recovers it.

`shared-part-inventory WORLD_ID` reads a pinned snapshot and reports, for the
installed [part library](part-library.md), stored alias part IDs, unknown parts,
assembly headers keyed by aliases and stored meshes whose vertex stream differs
from the library's. It never changes the head.

Supported operator commands include `shared-part-inventory`, `shared-maintenance-status`, `shared-gc`,
`shared-prune`, `shared-compact`, `shared-warm-view`, `shared-pack-world`, current-format namespace migration and current
history installation. They have no public HTTP mutation endpoint. See
[the runbook](operations.md).

`shared-warm-view WORLD_ID X Y Z [RADIUS]` materializes the existing accepted region through
the normal bounded reader and confirms its discardable cache packet. The optional radius
(1–32, default 1) must match what browsers request — the navigator derives it from the fog
distance (radius 21 at the default 2,200-stud cutoff) — otherwise the warmed packet's
region key never matches a browser read. It does not
advance the world head or publish geometry. See [cache warm-up](loading-and-cache.md#operator-warm-up)
for the source-built command and required storage configuration.

### Packed-block backfill

Source-built servers after v0.5.11 support
`shared-pack-block WORLD_ID X Y Z`. This is an explicit storage-only commit, not
a read-cache warm-up: it packs one allocated current-format cell. Use existing
storage credentials and the same read budgets as the viewer. A whole-root watch rejects concurrent changes.
No geometry, births, ownership, geographic reference or frozen physics is changed.
Only layout 4 and the 128-unit cell marker are accepted. Run
`shared-resize-cells WORLD_ID` for the [old-block migration](cells-and-migration.md)
before serving an old world; incompatible caches are deleted. Other current
unpacked cells use the reference reader until explicitly packed.

`shared-pack-region WORLD_ID X Y Z [RADIUS]` (since v0.5.25) enumerates the
allocated cells in the bounded cube — the same sparse directory scan as the
region reader — and packs each unpacked one, committing one storage-only pack
per cell. It reports packed/already-packed counts and per-cell revisions. Run it
before `shared-warm-view`: every cell pack advances the head, invalidating
revision-keyed view packets and loading checkpoints, so warm the packet once
after the backfill completes. Concurrent external edits reject the whole-root
watch and abort the run; re-run from the report.

### Whole-world packing

Servers since v0.5.25 additionally support:

```sh
target/release/ergo-server shared-pack-world "$ERGO_COMMON_WORLD_ID" --check
target/release/ergo-server shared-pack-world "$ERGO_COMMON_WORLD_ID"
target/release/ergo-server shared-pack-world "$ERGO_COMMON_WORLD_ID" --check
```

Use the existing production namespace, credentials, frozen physics and view budgets.
`--check` inventories one pinned accepted snapshot without publishing. The migration
scans **all** allocated-cell headers in ordered 64-row pages, rather than one camera
radius, and packs every unpacked cell. Geometry loading remains bounded per cell
by the configured view piece/byte budgets; already-packed cells are skipped.
Each completed cell logs its coordinate and accepted revision. Re-running a
completed migration is a no-op. A changed head between cells or a conflicting
whole-root watch stops the operation; reconcile an uncertain publication before
explicitly restarting. Earlier confirmed cell packs remain accepted.

The final report compares Merkle subtrees for policy, the canonical spatial index,
metadata/frozen physics, both canonical piece branches, assembly rules, joints and
birth/ownership records. Only `/3/2` read projections and `/5` cell headers may
change. This verifies all canonical model state without downloading every payload
again. It also checks that no external head change intervened. Normal read pins
are closed on successful completion and on already-packed skips.

Storage-only commits change the accepted revision/root. Existing view/workspace
ETags therefore miss and local current-state caches are refreshed through the
normal authorized read path; no origin-wide deletion or asset-cache flush is
needed. A retained historical snapshot remains valid for its own revision and
never aliases the new current root. Warm frequently visited regions **after** the
last pack to avoid invalidating each warm-up with the next storage commit.

#### Verified common-world backfill

The 2026-11-06 production run inventoried 113 allocated cells: 66 were already
packed and all 47 remaining cells were packed in 1,074.6 seconds. The final
accepted revision was
`7555ca403e8934b3dabea0978e079d53ce9be837f0302ee688a6625fc19762d1`.
The before/after canonical-subtree fingerprint was identical:
`2b7e909301183778d64b7a57e44f8d07881268d98fe7453b848a546d65f9130e`.
An independent `--check` confirmed all 113 cells packed; current-view HEAD with
the pre-migration token returned 200/new ETag and the new token returned 304.
Policy, frozen physics and canonical model state were unchanged. Historical
accepted snapshots were retained, rather than rewritten or pruned.

The same 73,660-piece district view at centre `[156,0,0]`, radius 21 was verified
in the persistent read cache after packing. See the
[measured loading breakdown](loading-and-cache.md#measured-whole-world-backfill).
Private operational evidence is under `target/packed-world-migration/` and the
before/after transfer reports under `target/loading-{before,after}-pack.json`.

### Current history installation

`shared-install-history WORLD_ID MANIFEST.json` accepts an explicitly staged,
semantically audited native history. The manifest is:

```json
{
  "format":"ergo-history-install-1",
  "expected_head":"64-lowercase-hex",
  "revisions":["oldest-through-newest-current-commit-CIDs"],
  "resume_file":"/absolute/private/path/resume"
}
```

The strings above are placeholders. The command acquires a barrier, checks the
live expected head and writes an adjacent ready receipt. It waits for the resume
file to contain the actual target tip. Close and verify the hosting gateway before
signaling it: server startup reads the world and cannot become healthy while the
barrier is held. Keep traffic closed until installation finishes and the matching
runtime is ready; the old runtime must not receive writes against the new format.
The library verifies consecutive parents and the complete retained sequence range,
builds a new acceptance index, and conditionally installs the head. It then runs
schema-aware GC and finishes the barrier. Application conversion must verify
poses, identities, authors, geometry and references before this installation.
Failure does not unlock and blindly repeat an uncertain operation.

Source: `transactions/src/{history,maintenance}.rs` and
`server/src/shared/{authoring,events,paged,spatial,bulk,world_pack,administration,retention}.rs`.
