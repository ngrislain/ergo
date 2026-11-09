# Where the server checks a construction before commit

This is a source map of the **implemented** persistence gate. It distinguishes
body-box nonpenetration, connection modeling, and static gravity equilibrium.
The current gate does not implement every requested real-brick connection rule.

## Request to accepted commit

For `POST /api/shared-worlds/{id}/commits`:

1. [`shared::commit`](../crates/server/src/shared/mod.rs) authenticates
   the editor and calls `Service::prepare` before `publish_with`.
2. [`Service::prepare`](../crates/server/src/shared/mod.rs) checks
   permissions, stages mutations, discovers old and new affected dependencies,
   and calls `ws.validate(&affected)` before returning a `Prepared` value.
3. [`Working::validate`](../crates/server/src/shared/working.rs)
   reconstructs the complete affected scene, infers supported stud connections,
   calls `validate_static_with_measurements` with the world's frozen server-owned
   measurement book; joints the book does not cover take the reviewed connector
   estimates, and joints with neither carry no load.
   It also checks every instance in retained source-model blobs, including unused
   definitions. Nested cancelling scales cannot bypass the pose checks.
4. Only the resulting prepared operation can reach
   [`Prepared::publish`](../crates/server/src/shared/mod.rs), which calls
   the transaction's conditional `commit`. A changed dependency causes a conflict;
   disjoint dependencies can rebase according to the transaction rules.

Assembly imports run the same affected-domain validator in
[`Service::import`](../crates/server/src/shared/mod.rs). Private
`POST /api/worlds` snapshots call
[`Store::save`](../crates/server/src/store.rs), which invokes the same static
validator and applies required measurements before writing the manifest. Internal
read-only fixture examples are explicitly marked as idealized; ordinary user
publication cannot inherit that exemption.

Single and batch assembly replacements share
[`replace_assemblies`](../crates/server/src/shared/authoring.rs). A batch discovers
all old dependencies first, stages every member, validates the complete final
affected domain and publishes one receipt. It does not validate or expose partial
intermediate assemblies. See the [REST workflow](concurrent-global-editing.md#rest-workflow).

Immutable blobs can be staged before validation; acceptance of the world head is
the commit boundary. See [concurrent editing](concurrent-global-editing.md) for
dependency closure, spatial phantom protection, and optimistic publication.

## 1. Bricks must not overlap

[`validate_static_with_measurements`](../crates/world/src/validation.rs)
normalizes nominal bounds from the trusted catalog/measurement book, validates
poses and joint references, and checks:

- Proper metric isometries: translation and rotation only, with orthonormal metric
  rotation and determinant +1. Scale, shear and reflection are rejected, both in
  materialized pieces and in model instances. Native construction axes have
  different scales; the test is performed after metric conversion.
- Axis-aligned compression-bearing faces. Tilted pieces must remain above ground
  and have a joint-supported load path; their envelope faces do not supply
  unilateral contact patches. Dynamic diagnostics use their actual oriented proxies.
- Baseplate penetration: the per-brick minimum-height check.
- Baseplate registration: grounded receiving interfaces must be upright, with
  half-integer public stud centres, within the server position tolerance. The
  nominal footprint edge is the fallback for parts without registered cavities.
  Reviewed tyres bear on the flat ground below their axle instead of engaging studs.
  This includes smooth tiles; flat-plane support alone does not authorize a
  fractional-stud foundation. Client snapping is convenience, not a substitute
  for this independent commit gate.
- Brick-to-brick penetration: octree AABB candidates, followed by oriented-box SAT
  and the reviewed compound shield/spear proxies and finite connector clearances.
- Catalogue stud/solid interference:
  [`first_stud_collision`](../crates/world/src/interference.rs) permits a stud
  inside another nominal body only at a registered compatible underside cavity.

A pair is rejected when it has positive-volume overlap beyond the configured
position tolerance (default `1e-4` construction units). Face touching is allowed.
The reviewed, upright/coaxial **3957a antenna / 4495a pennant** bore has narrowly
scoped [geometric clearance](part-catalog.md#castle-equipment-and-printed-heraldry)
within the real shaft range. This is not a general overlap exemption and does not
waive support, mass, registration or joint-capacity requirements.
Studs are excluded from the nominal body boxes, but the separate catalogue stud
clearance check rejects a floor stud entering a sideways solid face. Registered
stud engagement does not count as illegal penetration.

**Limit:** this is conservative nominal-body-box collision, not exact triangle,
cavity, Technic-hole, or connector-shape collision. A detailed visual mesh does
not make the persistence collision model exact. Reviewed Technic bores and grips
have explicit nominal clearance prisms, not universal exact hollow-mesh collision.
A round pin or axle in a round bore keeps that clearance at any roll of the beam
about it, so beams pinned at free angles do not collide with their own pins.

## 2. Bricks connect through studs or declared joints

[`spatial::relation`](../crates/world/src/spatial.rs) recognizes a
registered stud/cavity engagement under proper metric rotations, factory body
clips and reviewed hand/wheel/Technic interfaces.
[`constraints::inferred_joints`](../crates/world/src/constraints.rs)
turns each recognized pair into a joint.

**Stud joints remain `Fixed`, even with a one-stud footprint.** The automatic rule
“one engaged stud permits rotation about that stud” is not implemented.
Wheel bearings, frictionless `3673` pins and axles in round bores infer free
revolute joints; friction pins and minifigure shoulder, wrist and hand-grip clips
infer revolute joints with a friction torque; keyed axles and studs infer fixed
joints. Client simulation delegates
to the same inference implementation. Inference includes peripheral port bounds,
and server dependency discovery accounts for their bounded overhang beyond body indexes.

The serialized [`JointKind`](../crates/world/src/lib.rs) supports only:

- `Fixed`.
- `Revolute { axis, limits }`, authored explicitly or inferred from a reviewed bearing.

[`validate_joint_ids`](../crates/world/src/constraints.rs) checks endpoint
existence, distinct bodies, finite anchors, positive finite capacity claims, and
valid hinge axes/limits and duplicate part-pair joints.
[`attachments::validate_joints`](../crates/world/src/attachments.rs) then checks
every claimed attachment against server-owned connector geometry at server
tolerances: registered engagement, canonical anchor, permitted fixed/revolute
kind and the bearing axis. Current free bearings have no registered stops;
invented limits and side-face welds are rejected. These checks precede the
zero-joint-wrench shortcut, even if gravity alone supports every piece.

Specialty meshes must match their complete pinned catalogue vertex stream, both
on import/replacement and direct immutable mesh lookup. Client geometry cannot
relocate a hand or bore while retaining that catalogue ID's trusted interfaces.
REST regressions submit shifted/tilted spears, shields and bows, forged anchors,
kinds, wheel axes/stops, duplicate joints and modified meshes; rejection leaves
the accepted head unchanged, even with synthetic test-only capacity records.

[`ConnectionMeasurements::apply`](../crates/world/src/physical.rs) replaces
client strength claims on every joint, in one resolution order:

1. a matching server-owned part-pair, joint-kind and overlap-footprint
   measurement;
2. otherwise the reviewed estimate for the registered connector type
   ([`connector-estimates.json`](../crates/world/library/connector-estimates.json),
   source `estimate:ergo-connector-review-v1`), labelled as an estimate;
3. otherwise no capacity: the joint may carry no load.

Studs have no reviewed estimate, so an unmeasured stud joint carries no load and
no arbitrary clutch strength is assumed. A constructive compression proof can
assign zero wrench to every joint; `joint_strength_required:false` identifies that
gravity-only certificate without claiming calibrated clutch strength.

**Limits:** the reviewed estimates are engineering assumptions, not laboratory
measurements (see [physical calibration](physical-calibration.md)). A finite
authored anchor must still lie on compatible registered connectors. The gate also
permits compressive supporting contact without a stud joint and
checks each supported component; it does not require an entire shared world to
be one connected assembly.

## 3. Gravity must be balanced by admissible reactions

[`certify_components`](../crates/world/src/validation.rs) first discovers complete
independent contact/joint islands with
[`validation_components.rs`](../crates/world/src/validation_components.rs).
For each island requiring the general solver,
[`support_certificate`](../crates/world/src/validation.rs) builds a static
equilibrium problem at the submitted pose and
[`equilibrium::certify`](../crates/world/src/equilibrium.rs) solves it:

$$
\sum \mathbf{F}_{reaction} + m\mathbf{g} = 0,
\qquad
\sum \boldsymbol\tau = 0.
$$

The important parts are:

- Use sourced masses and apply gravity to every body.
- Give each joint an unknown wrench, each horizontal contact patch between
  axis-aligned bodies nonnegative corner forces, each ground contact nonnegative
  reactions, and each permitted foundation anchor an unrestricted reaction.
- Keep every joint load inside its capacities: force, axial force, bending, and
  for a revolute joint the moment about its axis within its friction torque (a
  free bearing's friction is tiny). A joint without capacity carries no load.
- Solve the physical island with one or more coupled local sparse SOCPs, so
  load loops such as hanging chains and pinned trusses share their loads. A
  computational cut copies actual interface reactions, never an anchor. Final
  admission requires a globally reconciled witness, checked body-by-body against
  canonical input independently of partition plans. See [partitioned stability](partitioned-stability.md).
- Re-check each solution independently and aggregate `max_joint_utilisation` by
  maximum. Report island count as `stability_components` and the largest general
  solve as `largest_equilibrium_component`; compression-only scenes report zero
  for the latter. Historical/unvalidated reports use zero when these counts are
  unavailable.
- On failure, name the joints that would need more capacity than measured or
  estimated, or report that no equilibrium respects contact unilaterality.
  A partition budget/numerical/convergence failure instead reports
  `validation_state:"inconclusive"`; it proves no physical instability and never
  falls back to an unbounded full-island factorization.

A faster constructive path is tried first for each unanchored island: it balances
loads top-to-bottom through nonnegative reactions inside horizontal support
patches with zero joint wrench, and its result is a feasible point of the same
problem. Own weights are initialized on demand; processed accumulated loads are
discarded, retaining only the pending-load frontier rather than a whole-scene
load vector. The affected scene and spatial membership index still materialize;
this does not claim constant-memory or arbitrary-chunk validation. All islands
must pass before the single atomic publication.

The default residual tolerances are `1e-6 N` and `1e-8 N·m` (raised in proportion
to the current island's weight for large islands); comparisons are not exact floating-point
equality. Capacities come from the frozen server-owned measurement book or the
reviewed estimates; a zero-joint-wrench compression proof consumes no capacity.

**Meaning of zero velocity/acceleration:** the saved state contains poses and
constraints, not transient velocities or accelerations. The server checks that
a rest pose admits the declared static equilibrium; it does not simulate motion
and compare velocity/acceleration fields with zero. The report's
`max_displacement = 0.0` identifies the no-time-integration path, not a measured
zero-motion trajectory. Under the declared model, balanced forces/moments are
the zero-acceleration condition at rest.

This certificate searches all load distributions of the declared model, but it is
not a real-world structural-safety certificate: capacities are measured lower
bounds or reviewed estimates, and bodies are rigid.
Client gravity and small-shake diagnostics do not replace this gate. Their
results disclose missing or omitted connections, and tall loose stacks need
solver-convergence qualification before a failed run justifies physical claims
or destructive cleanup.
See [rigid-body physics](rigid-body-physics.md#lightweight-static-certificate) and
[physical calibration](physical-calibration.md) for the assumptions and limits.

## Commands for the existing checks

Run from the repository root:

```sh
# Static overlap, floating-part, center-of-mass, and support-certificate tests.
cargo test --locked -p ergo-world validation::tests

# Sourced dimensions/masses and the capacity resolution order.
cargo test --locked -p ergo-world physical::tests

# Equilibrium certificate: hanging chain, cantilever, pinned truss, no-capacity joint.
cargo test --locked -p ergo-world equilibrium::tests

# Shared-world imports, dependency domains, conflicts, and rejected edits.
cargo test --locked -p ergo-server shared::tests
```

These existing tests exercise the implemented model. They are not evidence that
automatic single-stud hinges or connector-specific mating checks exist. The
[operations runbook](operations.md) lists the build/publication/deployment commands.
