# Paging, history and maintenance

All hosted operations target the one configured Sandbox. Current storage requires
transaction format 3, indexed accepted history and spatial layout 2 or 3.

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

Supported operator commands include `shared-maintenance-status`, `shared-gc`,
`shared-prune`, `shared-compact`, `shared-warm-view`, current-format namespace migration and current
history installation. They have no public HTTP mutation endpoint. See
[the runbook](operations.md).

`shared-warm-view WORLD_ID X Y Z` materializes the existing accepted region through
the normal bounded reader and confirms its discardable cache packet. It does not
advance the world head or publish geometry. See [cache warm-up](loading-and-cache.md#operator-warm-up)
for the source-built command and required storage configuration.

### Packed-block backfill

Source-built servers after v0.5.11 support
`shared-pack-block WORLD_ID X Y Z`. This is an explicit storage-only commit, not
a read-cache warm-up: it packs one allocated block and, when necessary, promotes
Metadata from spatial layout 2 to layout 3. Use existing storage credentials and
the same read budgets as the viewer. A whole-root watch rejects concurrent changes.
No geometry, births, ownership, geographic reference or frozen physics is changed.
Stop old writers before promotion and keep all subsequent writers page-aware;
layout-2-only servers reject the new metadata instead of leaving stale projections.
Other legacy blocks retain the reference fallback until explicitly packed.

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
`server/src/shared/{authoring,events,paged,spatial,administration,retention}.rs`.
