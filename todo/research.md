# Research references for unimplemented work

These are design inputs, not claims that their algorithms are implemented.

- [OGC 3D Tiles](https://www.ogc.org/standard/3dtiles/) and
  [Potree](https://www.cg.tuwien.ac.at/research/publications/2016/SCHUETZ-2016-POT/):
  out-of-core refinement and hierarchical content.
- Hoppe, [Progressive Meshes](https://doi.org/10.1145/237170.237216), and Garland/
  Heckbert, [Quadric Error Metrics](https://doi.org/10.1145/258734.258849).
- Macklin/Müller/Chentanez, [XPBD](https://doi.org/10.1145/2994258.2994272):
  compliant positional constraints.
- Todorov/Erez/Tassa, [MuJoCo](https://doi.org/10.1109/IROS.2012.6386109):
  articulated/contact optimization comparisons.
- Boore/Atkinson, [ground-motion prediction](https://doi.org/10.1193/1.2830434):
  magnitude/distance/site inputs rather than a direct magnitude-to-PGA conversion.
- [Jolt](https://github.com/jrouwe/JoltPhysics): sleeping/broad-phase comparisons.
- [LDView primitive controls](https://github.com/tcobbs/ldview/blob/master/Help.html#L2198-L2289),
  [LeoCAD mesh levels](https://github.com/leozide/leocad/blob/master/common/lc_mesh.cpp)
  and [Studio rendering options](https://studiohelp.bricklink.com/hc/en-us/articles/6505772624023-Photoreal-Eyesight-options):
  comparative source-viewer research, not a claim of proprietary renderer parity.

Any adoption needs implementation, appropriate calibration and verification
before moving into `docs/` as current behavior.
