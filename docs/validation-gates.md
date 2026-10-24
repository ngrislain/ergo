# Where the server checks a construction before commit

This is a source map of the **implemented** persistence gate. It distinguishes
body-box nonpenetration, connection modeling, and static gravity equilibrium.
The current gate does not implement every requested real-brick connection rule.

## Request to accepted commit

For `POST /api/shared-worlds/{id}/commits`:

1. [`shared::commit`](../crates/server/src/shared/mod.rs#L821-L833) authenticates
   the editor and calls `Service::prepare` before `publish_with`.
2. [`Service::prepare`](../crates/server/src/shared/mod.rs#L430-L567) checks
   permissions, stages mutations, discovers old and new affected dependencies,
   and calls `ws.validate(&affected)` before returning a `Prepared` value.
3. [`Working::validate`](../crates/server/src/shared/working.rs#L489-L546)
   reconstructs the complete affected scene, infers supported stud connections,
   applies the world's frozen server-owned measurements, and calls
   `validate_static_with_measurements`.
4. Only the resulting prepared operation can reach
   [`Prepared::publish`](../crates/server/src/shared/mod.rs#L692-L705), which calls
   the transaction's conditional `commit`. A changed dependency causes a conflict;
   disjoint dependencies can rebase according to the transaction rules.

Assembly imports run the same affected-domain validator in
[`Service::import`](../crates/server/src/shared/mod.rs#L323-L423). Private
`POST /api/worlds` snapshots call
[`Store::save`](../crates/server/src/store.rs#L213-L231), which applies measurements
and invokes the same static validator before writing the manifest. Internal
read-only fixture examples are explicitly marked as idealized; ordinary user
publication cannot inherit that exemption.

Immutable blobs can be staged before validation; acceptance of the world head is
the commit boundary. See [concurrent editing](concurrent-global-editing.md) for
dependency closure, spatial phantom protection, and optimistic publication.

## 1. Bricks must not overlap

[`validate_static_with_measurements`](../crates/world/src/validation.rs#L95-L164)
normalizes nominal bounds from the trusted catalog/measurement book, validates
poses and joint references, and checks:

- Baseplate penetration: `validation.rs:114-119`.
- Brick-to-brick penetration: `validation.rs:120-134`, using an octree to find
  candidate pairs and the per-axis overlap of their nominal body AABBs.

A pair is rejected when it has positive-volume overlap beyond the configured
position tolerance (default `1e-4` construction units). Face touching is allowed.
Decorative studs are excluded from these body boxes so intended stud engagement
does not count as penetration.

**Limit:** this is conservative nominal-body-box collision, not exact triangle,
cavity, Technic-hole, or connector-shape collision. A detailed visual mesh does
not make the persistence collision model exact.

## 2. Bricks connect through studs or declared joints

[`spatial::relation`](../crates/world/src/spatial.rs#L165-L208) recognizes a
restricted stud-contact case: matching top/bottom contact, upright unscaled
catalog parts, coincident stud lattices, and studs on the lower part.
[`constraints::inferred_joints`](../crates/world/src/constraints.rs#L13-L35)
turns each recognized pair into a joint.

**Important gap: every automatically inferred joint is currently `Fixed`, even
when the engagement footprint is one stud.** The requested automatic rule
“one engaged stud permits rotation about that stud” is not implemented.
The client-side inference in `physics.rs` uses the same fixed-joint simplification.

The serialized [`JointKind`](../crates/world/src/lib.rs#L262-L289) supports only:

- `Fixed`.
- `Revolute { axis, limits }`, authored explicitly.

[`validate_joint_ids`](../crates/world/src/constraints.rs#L41-L81) checks endpoint
existence, distinct bodies, finite anchors, positive finite capacity claims, and
valid hinge axes/limits. Before publication,
[`ConnectionMeasurements::apply`](../crates/world/src/physical.rs#L225-L253)
replaces client strength claims with a matching server-owned part-pair,
joint-kind, and overlap-footprint measurement. Missing measurements reject
normal publication instead of assuming an arbitrary clutch strength.

**Limits:** a generic revolute joint can idealize a hinge or axle, but there is no
axle/pin/clip-specific connector taxonomy or geometric mating proof. A finite
authored anchor is not proof that it lies on compatible physical connectors.
The measurement lookup is not a substitute for that missing topology check.
The gate also permits compressive supporting contact without a stud joint and
checks each supported component; it does not require an entire shared world to
be one connected assembly.

## 3. Gravity must be balanced by admissible reactions

[`support_certificate`](../crates/world/src/validation.rs#L177-L439) constructs a
static force-and-moment balance at the submitted pose:

$$
\sum \mathbf{F}_{reaction} + m\mathbf{g} = 0,
\qquad
\sum \boldsymbol\tau = 0.
$$

The important parts are:

- `validation.rs:189-242`: construct a joint/contact graph and select a forest.
- `validation.rs:273-305`: use sourced masses, apply gravity, identify ground
  supports, and account for permitted foundation anchors.
- `validation.rs:307-343`: require the projected center of mass to lie in the
  support polygon and construct nonnegative ground reactions.
- `validation.rs:361-389`: propagate subtree loads, reject joint force/torque
  overload, and reject torque about a revolute joint's free axis.
- `validation.rs:391-423`: allow only compressive, non-tipping face contacts.
- `validation.rs:433-437`: reject residual unbalanced force or moment.

The default residual tolerances are `1e-6 N` and `1e-8 N·m`; comparisons are not
exact floating-point equality. Ordinary publication must first obtain connector
capacities from the frozen server-owned measurement book.

**Meaning of zero velocity/acceleration:** the saved state contains poses and
constraints, not transient velocities or accelerations. The server checks that
a rest pose admits the declared static equilibrium; it does not simulate motion
and compare velocity/acceleration fields with zero. The report's
`max_displacement = 0.0` identifies the no-time-integration path, not a measured
zero-motion trajectory. Under the declared model, balanced forces/moments are
the zero-acceleration condition at rest.

This is a conservative sufficient certificate, not a general optimizer over all
possible load distributions and not a real-world structural-safety certificate.
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
