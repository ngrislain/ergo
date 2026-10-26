# Physical fidelity and solver plans

These items are not implemented. Current guarantees are in
[validation](../docs/validation-gates.md) and [calibration](../docs/physical-calibration.md).

- Obtain measured connector capacities by part pair, mold, engagement, wear and
  loading direction. Do not invent universal clutch-force defaults.
- Measure centers of mass, hollow-part inertia, friction, restitution and mold variants.
- Extend the constructive certificates with a general sparse feasibility search:

$$
A\mathbf f+\mathbf w=0,\qquad
\mathbf f\in\mathcal K_{contact},\qquad
\mathbf f_{joint}\in\mathcal K_{capacity}.
$$

- Experimentally justify directional failure envelopes, for example:

$$
\left(\frac{F_{tension}}{F_{pull}}\right)^p+
\left(\frac{\|\mathbf F_{shear}\|}{F_{shear,max}}\right)^p+
\left(\frac{\|\mathbf M_{bend}\|}{M_{bend,max}}\right)^p+
\left(\frac{|M_{twist}|}{M_{twist,max}}\right)^p\le1.
$$

The interaction form and exponent require measurements; this is not a shipped model.

- Larger complete mechanical islands, explicit boundary supports, sleeping,
  compound rigid components and a region/island/body broad phase.
- Incremental graph splitting after failure, momentum-conserving component
  separation, fatigue, impact damage, crushing and fragment generation.
- Recorded accelerograms, vertical/multi-axis/rotational excitation, spectral
  matching, randomized ensembles and parameter/convergence studies.
- Qualify tall unbonded stacks against analytic equilibrium and refined solver
  settings. An earlier bounded dynamics profile numerically collapsed a
  thirty-course loose stack that remained near equilibrium with a substantially
  more expensive constraint solve. Refined profiles still need independent
  loose-stack qualification. Do not use unqualified failures for
  destructive cleanup or infer measured clutch strength from an empty joint set.
- Ground-motion models only with traceable region/distance/site inputs; magnitude
  is not local PGA. Impact energy alone does not specify peak force.
- Contact-force, support-polygon and connector-utilization overlays.
- Variable physical terrain/support domains beyond the global Z=0 floor.
