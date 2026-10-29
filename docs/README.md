# Implemented Ergo behavior

These guides describe the current code and supported operations. Unimplemented
designs live in `todo/` and are indexed by [TODO.md](../TODO.md).

## Use Ergo

- [Agent editing and placement](agent-shared-editing.md)
- [MCP tools and installation](local-construction-mcp.md)
- [Official/custom model search and reuse](model-library.md)
- [Sourced building-technique library](local-construction-mcp.md#sourced-building-techniques)
- [Walkable pyramid and ten-level tower recipes](model-recipes.md)
- [Camera navigation, GPS and construction list](navigation.md)
- [Loading progress, timing diagnosis and revision-safe caches](loading-and-cache.md)
- [Sign-in and access](access-and-onboarding.md)
- [Nested groups](groups.md)
- [Collision-aware positioning](interactive-constraints.md)

## Formats and storage

- [Native construction/model format](construction-format.md)
- [Native part descriptions](part-catalog.md)
- [S3 organization](object-store-layout.md)
- [Every persisted record family](object-store-formats.md)
- [Spatial persistence indexing](persistence-indexing.md)
  includes packed block reads and the layout-2 backfill/writer-fence workflow.
- [Shared-world protocol](concurrent-global-editing.md)
- [Transactional CBOR library](transactional-storage.md)
- [Paging, history and maintenance](storage-operations.md)
- [Commit history](commit-history.md)
- [Identity and private account state](identity-and-ownership.md)

## Engine and validation

- [Architecture and execution boundaries](architecture.md)
- [Block display pipeline, greedy surface LOD and rendering benchmarks](3d-display-indexing.md)
- [Lighting and shadows](lighting.md)
- [Stud-specific LOD](stud-lod.md)
- [Ground, coordinates and placement snapping](ground-and-coordinates.md)
- [Rigid-body and static physics](rigid-body-physics.md)
- [Server acceptance gates](validation-gates.md)
- [Sourced mass and connector calibration](physical-calibration.md)
  includes a read-only construction measurement-requirements inventory helper.
- [Gravity, earthquake and impact probes](solidity-testing.md)

## Maintain and deploy

- [Operations](operations.md)
- [Accepted-region cache warm-up](loading-and-cache.md#operator-warm-up)
- [Scaleway deployment](scaleway-deployment.md)
- [GitHub client/image distributions](distribution.md)
- [Public website](static-site.md)
- [Implementation references](references.md)

## Boundaries

The hosted service has one public world and no aggregate world piece-count cap.
Individual operations are bounded: shared edits normally load at most 5,000 pieces
and 128 MiB of CBOR; god-mode authoring has no piece-count cap. Browser regions
have independent budgets, by default 50,000 pieces/64 MiB of charged reads; local
simulation uses a complete working set of at most 5,000 pieces. Imports have a 128 MiB upload
and 200,000-part expansion limit. Rendering never substitutes for server acceptance.

The renderer, importer, persistence address space and physics model are separate.
In particular, a storage cell is not a mechanical joint or an extra floor. Geometry
and transforms use public east/north/up coordinates; actual simulation uses SI units.
