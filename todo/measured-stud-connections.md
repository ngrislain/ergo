# Measured stud-connection pathway

The initial selected direction was measured stud connections. After reviewing
the missing pairwise data, the operator requested ballpark estimates instead.
The explicit local [estimated diagnostic mode](../docs/physical-calibration.md#explicit-ballpark-diagnostic-mode)
implements those disclosed engineering assumptions; it does not supply measured
calibration or alter the authoritative world's frozen measurement book. This file
records literature and remaining work toward genuinely measured connections.

## Verified literature

- Luo et al., [Legolization: Optimizing LEGO Designs (2015)](https://www.cs.columbia.edu/~yonghao/siga15/luo-Legolization.pdf),
  Appendix A, describes hanging-load separation tests: 70 arrangements, three
  trials per arrangement, increasing hanging mass in 10 g steps. The authors fit
  one internal contact-point friction parameter across arrangements using torque
  balance and least squares. The reported value is
  `71.658 g × 9.8 m/s² = 0.7022484 N` per modeled contact point.
- This is a fitted model parameter, not a published minimum or confidence-bound
  isotropic force/torque capacity for every catalog part pair. The detailed hanging
  tests cover 1×3/4/6/8 and 2×3/4/6/8 bricks; 1×1, 1×2 and 2×2 were excluded from
  those tests because they were too small to hang the weights. The model assumes
  equal friction capacity for three contact geometries and no brick/stud fracture.
  It analyzes static loads, not validated dynamic earthquakes. Plates, large floor
  slabs, slopes and specialty parts are not established by that calibration.
- Liu et al., [StableLego (2024)](https://arxiv.org/html/2402.10711v1#S4),
  reports a manually measured `0.98 N` contact-point friction limit and a related
  directional force model. That paper likewise does not provide the current
  matcher's complete per-pair isotropic force/torque book. Its
  [implementation](https://github.com/intelligent-control-lab/StableLego) is MIT
  licensed; the dataset contains predicted assembly stability, not raw connector
  force/torque samples. The two fitted values must not be silently averaged or
  presented as the same experiment.
- [Brick Architect's test implements](https://brickarchitect.com/2021/lego-clutch-test-implements-bricks/)
  describe measuring hardware and geometry, not numerical stud-joint capacities.
  [Brick Experiment Channel's part tests](https://brickexperimentchannel.wordpress.com/2023/11/15/characteristics-of-lego-parts/)
  include motor, axle and clutch-gear torques; those are different mechanisms from
  the palace's stud/cavity connections.

## Implementation and data still required

1. Use the [connector requirements helper](../docs/physical-calibration.md#inventory-the-measurements-a-construction-requires)
   to enumerate the actual proposal's required keys. Reduce unnecessary part-pair
   diversity while preserving nine upright courses, walkable stairs and the shape.
2. Either obtain applicable measured combined-force/moment envelopes for the
   existing matcher, or implement a separately named directional contact-point
   model corresponding to the literature. Do not invent torque values from a
   pull-off force or relabel a least-squares fit as a conservative measured bound.
3. For a literature-calibrated model, preserve the study's scope and assumptions,
   distinguish reported arrangements/trials from independent specimen counts,
   identify unmeasured directions and parts, and validate against the paper's
   physical fixtures before using it for the palace.
4. Revisit single-stud rotational freedom, compression versus tension, bending,
   torsion, overlapping contact footprints and breakage feedback. A generic fixed
   joint with one scalar break force/torque does not reproduce the measured model.
5. Qualify force-impulse conversion and timestep/solver convergence for dynamics;
   static support alone does not validate random-shake survival.
6. Plan any authoritative model/data transition explicitly: shared worlds freeze
   their measurement book. New data must not silently rewrite historical model
   meaning or acquire an existing world's authority from an untrusted client.

The replacement palace remains unpublished until its selected model and required
data are implemented and its results are reproducible within their stated scope.
