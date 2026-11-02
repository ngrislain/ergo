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
   measurement book, and requires measured capacities if the certificate needs
   joint strength.
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
waive support, mass, registration or measured joint-strength requirements.
Studs are excluded from the nominal body boxes, but the separate catalogue stud
clearance check rejects a floor stud entering a sideways solid face. Registered
stud engagement does not count as illegal penetration.

**Limit:** this is conservative nominal-body-box collision, not exact triangle,
cavity, Technic-hole, or connector-shape collision. A detailed visual mesh does
not make the persistence collision model exact. Reviewed Technic bores and grips
have explicit nominal clearance prisms, not universal exact hollow-mesh collision.

## 2. Bricks connect through studs or declared joints

[`spatial::relation`](../crates/world/src/spatial.rs) recognizes a
registered stud/cavity engagement under proper metric rotations, factory body
clips and reviewed hand/wheel/Technic interfaces.
[`constraints::inferred_joints`](../crates/world/src/constraints.rs)
turns each recognized pair into a joint.

**Stud joints remain `Fixed`, even with a one-stud footprint.** The automatic rule
“one engaged stud permits rotation about that stud” is not implemented.
Wheel bearings, frictionless pins and axles in round bores infer revolute joints;
keyed axles, friction pins and grips infer fixed joints. Client simulation delegates
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

When publication needs joint strength,
[`ConnectionMeasurements::apply`](../crates/world/src/physical.rs)
replaces client strength claims with a matching server-owned part-pair,
joint-kind, and overlap-footprint measurement. Missing required measurements
reject publication instead of assuming an arbitrary clutch strength. A
constructive compression proof can instead assign zero wrench to every joint;
`joint_strength_required:false` identifies that gravity-only certificate without
claiming calibrated clutch strength.

**Limits:** a generic revolute joint can idealize a hinge or axle, but there is no
axle/pin/clip-specific connector taxonomy or geometric mating proof. A finite
authored anchor is not proof that it lies on compatible physical connectors.
The measurement lookup is not a substitute for that missing topology check.
The gate also permits compressive supporting contact without a stud joint and
checks each supported component; it does not require an entire shared world to
be one connected assembly.

## 3. Gravity must be balanced by admissible reactions

[`compression_certificate` and `support_certificate`](../crates/world/src/validation.rs)
construct static force-and-moment balance at the submitted pose:

$$
\sum \mathbf{F}_{reaction} + m\mathbf{g} = 0,
\qquad
\sum \boldsymbol\tau = 0.
$$

The important parts are:

- `compression_certificate`: balance loads top-to-bottom through nonnegative
  reactions inside horizontal support patches, with zero joint wrench.
- `support_certificate`: construct a joint/contact graph and select a forest.
- Use sourced masses, apply gravity, identify ground
  supports, and account for permitted foundation anchors.
- Require the projected center of mass to lie in the
  support polygon and construct nonnegative ground reactions.
- Propagate subtree loads, reject joint force/torque
  overload, and reject torque about a revolute joint's free axis.
- Allow only compressive, non-tipping face contacts.
- Reject residual unbalanced force or moment.

The default residual tolerances are `1e-6 N` and `1e-8 N·m`; comparisons are not
exact floating-point equality. Certificates consuming joint strength must obtain
capacities from the frozen server-owned measurement book; a zero-joint-wrench
compression proof consumes no unknown clutch capacity.

**Meaning of zero velocity/acceleration:** the saved state contains poses and
constraints, not transient velocities or accelerations. The server checks that
a rest pose admits the declared static equilibrium; it does not simulate motion
and compare velocity/acceleration fields with zero. The report's
`max_displacement = 0.0` identifies the no-time-integration path, not a measured
zero-motion trajectory. Under the declared model, balanced forces/moments are
the zero-acceleration condition at rest.

This is a conservative sufficient certificate, not a general optimizer over all
possible load distributions and not a real-world structural-safety certificate.
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

# Sourced dimensions/masses and rejection of missing connector capacities.
cargo test --locked -p ergo-world physical::tests

# Shared-world imports, dependency domains, conflicts, and rejected edits.
cargo test --locked -p ergo-server shared::tests
```

These existing tests exercise the implemented model. They are not evidence that
automatic single-stud hinges or connector-specific mating checks exist. The
[operations runbook](operations.md) lists the build/publication/deployment commands.
