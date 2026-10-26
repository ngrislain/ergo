# Editing and navigation plans

These items are not implemented.

- Translation/rotation gizmos, snapping and typed metric inputs.
- Continuous rotational sweeps and richer constrained sliding.
- Exact local joint frames, motors and advanced parameters in the UI.
- Multi-step semantic undo/redo, progress/cancellation and coordinated client jobs.
- Complete shared-world CAD manipulation with surrounding context and certified
  mechanical boundaries. The current construction list navigates the shared world.
- Exact global nearest-construction search beyond bounded/local hints.
- Gameplay triggers and automatic trap mechanisms; modeled trap geometry is not
  a trigger/event system.

## Quasi-static precision positioning

For pose vector q, target t and previous pose q₀, a proposed editor solver is:

$$
\min_{\mathbf q}\quad \tfrac12\|h(\mathbf q)-\mathbf t\|_W^2+
\tfrac\alpha2\|\mathbf q-\mathbf q_0\|^2
$$

subject to joint equalities, nonpenetration and angle limits. The regularizer
chooses nearby solutions; active sets or compliant increments need bounded steps
and unreachable-target feedback. This nonconvex optimization is not the current
fixed-group/Rapier positioning implementation.
