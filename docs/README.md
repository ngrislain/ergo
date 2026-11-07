# Implemented Ergo behavior

These guides describe the current code and supported operations. Unimplemented
designs live in `todo/` and are indexed by [TODO.md](../TODO.md).

## Use Ergo

- [Agent editing and placement](agent-shared-editing.md)
- [MCP tools and installation](local-construction-mcp.md)
- [Codex and Claude Code plugin](mcp-plugins.md)
- [Official MCP Inspector and unlisted 3D catalogue](mcp-inspector.md)
- [Official/custom model search and reuse](model-library.md)
- [Sourced building-technique library](local-construction-mcp.md#sourced-building-techniques)
- [Walkable monument, genuine-tree park and castle mountain recipes](model-recipes.md)
- [Paris landmarks: accepted models, closed-roof revision and articulated prototype](paris-landmarks.md)
- [Camera navigation, GPS and construction list](navigation.md)
- [Resumable loading, fast Merkle freshness checks and local cache diagnostics](loading-and-cache.md)
- [Sign-in and access](access-and-onboarding.md)
- [Nested groups](groups.md)
- [Collision-aware positioning](interactive-constraints.md)

## Formats and storage

- [Native construction/model format](construction-format.md)
- [Part library entries, derived profiles and the unlisted HTML catalogue](part-catalog.md)
- [The synthesized part library (LDraw + BrickLink), parts-sync and common constructs](part-library.md)
  includes common-part usage audits, hand/wheel/Technic attachments, plate-free figures/cars,
  printed Lion Knights torsos and supported pennant standards.
- [S3 organization](object-store-layout.md)
- [Every persisted record family](object-store-formats.md)
- [Spatial persistence indexing](persistence-indexing.md)
  includes packed block reads and projection backfill.
- [128-stud cells, atomic migration and incompatible-cache deletion](cells-and-migration.md)
- [Shared-world protocol](concurrent-global-editing.md)
- [Transactional CBOR library](transactional-storage.md)
- [Paging, history and maintenance](storage-operations.md)
  includes whole-world packed-read migration and Merkle-based cache invalidation.
- [Commit history](commit-history.md)
- [Identity and private account state](identity-and-ownership.md)

## Engine and validation

- [Architecture and execution boundaries](architecture.md)
- [Block display pipeline, buried-part rejection, LOD and walking benchmarks](3d-display-indexing.md)
- [Lighting and shadows](lighting.md)
- [Stud-specific LOD](stud-lod.md)
- [Clean OSM ground, cached floor patterns and coordinate/placement rules](ground-and-coordinates.md)
- [Rigid-body and static physics](rigid-body-physics.md)
- [Server acceptance gates](validation-gates.md)
- [Sourced mass and connector calibration](physical-calibration.md)
  includes a read-only construction measurement-requirements inventory helper.
- [Gravity, earthquake and impact probes](solidity-testing.md)

## Maintain and deploy

- [Operations](operations.md)
- [Accepted-region cache warm-up](loading-and-cache.md#operator-warm-up)
- [Scaleway deployment](scaleway-deployment.md)
- [npm/GitHub client and image distributions](distribution.md)
- [Public website](static-site.md)
- [Implementation references](references.md)

## Boundaries

The hosted service has one public world and no aggregate world piece-count cap.
Individual operations are bounded: shared edits normally load at most 10,000 pieces
and 128 MiB of CBOR; god-mode authoring has no piece-count cap. Browser regions
have independent budgets, by default 50,000 pieces/64 MiB of charged reads; local
simulation uses a complete working set of at most 5,000 pieces. Imports have a 128 MiB upload
and 200,000-part expansion limit. Rendering never substitutes for server acceptance.

The renderer, importer, persistence address space and physics model are separate.
In particular, a storage cell is not a mechanical joint or an extra floor. Geometry
and transforms use public east/north/up coordinates; actual simulation uses SI units.
