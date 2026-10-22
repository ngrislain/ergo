# Implementation backlog

Keep this file synchronized with the code and [technical documentation](docs/README.md).
An item is complete only when its implementation, verification, and documentation
are complete. Future designs must not be described as existing guarantees.

## Physical fidelity and validation

- [ ] Obtain and version measured connection-capacity datasets for supported part
  pairs, engagement footprints, wear conditions, and load directions. Do not use
  invented universal clutch-force defaults.
- [ ] Add directional pull-off, shear, bending, and torsion failure envelopes,
  with uncertainty ranges and independent validation against physical experiments.
- [ ] Measure/refine center-of-mass locations and inertia tensors for hollow parts;
  retain sourced per-part masses rather than treating body boxes as solid ABS.
- [ ] Add exact mold-variant dimensions and masses where catalog averages differ.
- [ ] Measure surface friction and restitution rather than treating current solver
  coefficients as calibrated material data.
- [ ] Extend the constructive static wrench certificate with a general sparse
  equilibrium search that can find alternative feasible reaction distributions.
- [ ] Add perturbation tests, longer settling tests, and numerical convergence
  studies; distinguish acceptance tests from indefinite stability proofs.
- [ ] Add variable physical terrain/support domains if needed; the current model
  explicitly uses one global Z=0 floor, independent of vertical storage layers.

## Rendering and client memory

- [x] Frustum-cull octree nodes and upload visible instances within a finite fog horizon.
- [ ] Introduce per-page octrees so loading or moving one region does not rebuild
  the entire resident index.
- [ ] Subdivide dense 1024³ storage cells into independently streamed render pages.
- [ ] Add view-directed prefetch, eviction hysteresis, request cancellation, and
  byte-based decoded-memory limits.
- [ ] Move decoding/index construction and physics to workers.
- [x] Add stud/detail LODs and distant aggregate geometry.
- [ ] Profile optional WebGPU compute/rendering for large dynamic scenes after
  sleeping/rigid aggregation; keep GPU data resident and retain a WASM CPU baseline.
- [ ] Measure cache hit rates and add true byte-budgeted LRU behavior; the initial
  persistent cache uses insertion-order eviction.
- [ ] Share identical geometry-cache entries across authorized snapshots without
  weakening world ownership checks.
- [ ] Add robust WebGL context-loss recovery and performance benchmarks on mobile.

## Imports, parts, and assets

- [ ] Expand the deliberately restricted common-part/color catalog using an
  explicit selection policy and sourced popularity metrics if required.
- [ ] Add wheel, axle, pin, and hinge geometries plus connector metadata; joint
  support alone does not imply these parts exist in the starter catalog.
- [ ] Exercise Studio import against representative current and legacy `.io`
  archives, assemblies, custom parts, and alternate ZIP encryption variants.
- [ ] Add LDraw texture/BFC/material support and preserve construction steps.
- [x] Add legal/attribution metadata and checksum/version tracking to imported
  third-party geometry assets.
- [ ] Consider a packed, compressed, dictionary-based binary page format after
  profiling the current CBOR representation.

## Interaction and constraints

- [x] Persist nested authoring groups and rigid transforms, independent of physical bonds.
- [x] Add orbit, fly, and walk camera modes with keyboard/touch controls.

- [ ] Replace sampled translation checks with continuous shape casts, including
  rotational sweeps and constrained sliding along obstacles.
- [ ] Add translation/rotation gizmos, snapping, and typed metric coordinates.
- [ ] Expose exact local anchors, axis frames, angle limits, and motor parameters
  in the UI, rather than requiring JSON for advanced joint configuration.
- [ ] Add a quasi-static sparse inverse-kinematics/positioning solver with clear
  unreachable-target feedback.
- [ ] Add multi-step undo/redo and semantic edit commands; the initial reset is a
  working-copy restore, not a complete history.
- [ ] Discover complete mechanical islands and support dependencies without
  requiring the complete construction to fit in the interactive sandbox.

## Global shared-world commits

See [concurrent global editing](docs/concurrent-global-editing.md) for the
implemented optimistic REST protocol. The world has no aggregate size/count cap;
the limits and paging work below concern individual operations and indexes.

- [x] Add a standalone immutable CBOR octree transaction library with root CAS,
  read/predicate validation, replacement fences, disjoint-edit rebase, sharded
  heads, materialized checkpoints, and parent-linked substitution history.
- [x] Integrate authenticated REST commits, region grants, on-demand complete
  contact/constraint/group closure discovery, and server static validation.
- [x] Add packed paths, a byte-budgeted shared LRU, bounded-object reads, paginated
  scans, and an in-memory/S3 concurrent workload benchmark executable.
- [x] Add durable reader/writer pins, exclusive maintenance barriers, disk-backed
  streaming mark/sweep, and node-budgeted regional tombstone compaction.
- [x] Add a shared branch/revision model separate from per-user private snapshots.
- [x] Store versioned owner/region grants and atomically invalidate prepared edits
  on revocation; identity alone does not authorize another user's world.
- [x] Add authoritative global piece IDs derived from immutable creation records,
  separately versioned geometry, and reference-only cross-block replicas.
- [x] Track conservative swept predicates and complete old/new dependency closures;
  reject oversized edit working sets without truncating unloaded supports.
- [x] Add atomic root publication, dependency conflict detection, and safe rebase.
- [x] Add immutable authenticated audit/certificate attachments and paged history.
- [x] Keep a materialized checkpoint at every commit; viewing requires no replay.
- [x] Run bounded independent/grouped benchmarks against live Scaleway fr-par
  (2026-10-22). Functional checks pass; large-scale performance remains unqualified.
- [x] Exercise live Scaleway conditional writes: five 16-writer same-ETag races,
  immediate visibility, independent-edit rebase, and historical snapshots pass.
- [x] Add opt-in bounded group publication, individual commit/audit results,
  per-member uncertain-outcome reporting, and grouped benchmark mode. Enable only
  after measuring the deployment's actual hot-shard workload.
- [x] Add atomic CID/sequence acceptance indexes and explicit legacy upgrades.
- [x] Add explicit cross-assembly joints with endpoint permissions, incident
  predicates, complete dependency validation, and dangling-delete checks.
- [x] Page adaptive spatial references and assembly membership/adjacency/rule rows;
  migrate old layouts through streamed single-publication maintenance.
- [x] Add bounded authorization-aware SSE, shared per-world polling, resume/reset
  handling, coalescing, and no permanent subscriber snapshot pin.
- [x] Add opt-in owner keep-last-N, explicit pruning, and schema-linked collection
  preserving live birth records, meshes, models, and selected history.
- [ ] Optional: boundary certificates/model migration. Current policy rejects
  oversized closures and retains the frozen world model.
- [ ] Add optional hierarchical leases only if a future workflow requires them;
  optimistic validation is the implemented coordination policy.
- [x] Add private-summary/shared-user catalog pages, hierarchical geographic
  discovery, pre-publication advertisements, permission filtering, and backfills.
- [ ] Update the legacy private-world UI to consume summary pages instead of its
  compatibility full-list route (UI work, not a shared storage prerequisite).

## MCP and agent feedback

- [ ] Add authenticated delegation and a permission-checked global commit tool only
  after the shared-world commit service exists. Local tools must not write S3
  heads directly.
- [ ] Extend local expected-revision/job-expiry checks to coordinated multi-client
  working copies, cancellable long simulations,
  progress notifications, and richer machine-readable failure witnesses.
- [ ] Add connector load/utilization overlays, support polygons, collision witness
  images, and annotations identifying unstable or detached components.
- [ ] Add renderer parity tests between the browser viewport and headless vision
  previews, especially for imported rotations and unusual geometry.

## Dynamic destruction

- [ ] Split welded/compound rigid components when a connection breaks and conserve
  resulting linear/angular momentum.
- [ ] Recompute affected graph connectivity incrementally after bond deletion.
- [ ] Add impact damage, fatigue, torque-aware fracture, and calibrated connector
  failure rather than relying solely on impulse-threshold experiments.
- [ ] Keep live destruction local/session-scoped; publish only a settled,
  revalidated static result.

## Solidity load cases

- [ ] Import sourced earthquake accelerograms and support multi-axis/vertical
  excitation; current tests use a controlled horizontal displacement waveform.
- [ ] Add published, region-appropriate ground-motion models when accepting moment
  magnitude, source distance, and site conditions; do not equate source magnitude
  with local PGA.
- [ ] Study timestep/solver convergence at high shake frequencies and stiff impacts.
- [ ] Add repeatable parameter sweeps, randomized load ensembles, and regression
  reports that compare capacity/uncertainty profiles.
- [ ] Expose dropped-object trajectories and contact-force/connector-utilization
  overlays in web and MCP visual feedback.
- [ ] Calibrate impact contact stiffness/restitution and material-crushing behavior;
  nominal mgh does not determine peak force.

## Deployment and operations

- [ ] Add live Scaleway and Google OAuth integration coverage using configured
  test resources and authorized redirect URLs.
- [ ] Add per-user quotas, request throttling, import/solver job isolation, and
  bounded background validation queues.
- [ ] Add individual session revocation if required; signed stateless cookies
  currently support expiry and global signing-key rotation.
- [ ] Add CI for native Rust, WebAssembly, browser flows, MCP protocol behavior,
  documentation links, and SVG validity.
- [ ] Publish deployment images, operational metrics, backup/restore procedures,
  and benchmark datasets with documented scale limits.
