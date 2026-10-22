# References and related systems

These references explain the design space. They are **not** a list of algorithms
all implemented by Ergo. The implementation/status sections in the linked design
documents remain authoritative. In particular, WebGPU, general sparse equilibrium
optimization, quadric-error mesh simplification, and distributed global edit leases
are not implied by citing papers about them.

## Spatial indexing and out-of-core display

### S1. Hierarchical spatial data structures

Hanan Samet, **The Quadtree and Related Hierarchical Data Structures**, ACM
Computing Surveys, 1984. [DOI: 10.1145/356924.356930](https://doi.org/10.1145/356924.356930).

Useful for understanding recursive space partitioning, regional queries, and why
a geographic/storage hierarchy is a different concern from individual mesh
geometry. Ergo currently uses a derived resident octree and sparse storage-cell
metadata; it does not persist Samet's entire family of tree variants.

### S2. Bounding-box indexes

Anton Guttman, **R-trees: A Dynamic Index Structure for Spatial Searching**,
SIGMOD, 1984. [DOI: 10.1145/602259.602266](https://doi.org/10.1145/602259.602266).

An R-tree indexes object extents rather than subdividing a fixed grid uniformly.
It is a useful alternative when object sizes vary dramatically or when indexing
large construction bounds in a geographic catalog. The initial implementation
uses octrees, not an R-tree backend.

### S3. Hierarchical content and screen-space detail

[OGC 3D Tiles](https://www.ogc.org/standard/3dtiles/) is a useful comparable system
for hierarchical bounding volumes, view-driven content loading, and screen-space
error. [Potree](https://www.cg.tuwien.ac.at/research/publications/2016/SCHUETZ-2016-POT/)
is a related large-point-cloud renderer and academic thesis by Markus Schütz.

The transferable ideas are bounded residency, view-driven refinement, and
separating an address hierarchy from render payloads. Ergo does not currently
read 3D Tiles or Potree file formats, and its repeated brick geometry differs
from a point cloud's data model.

### S4. Progressive and simplified meshes

Hugues Hoppe, **Progressive Meshes**, SIGGRAPH, 1996.
[DOI: 10.1145/237170.237216](https://doi.org/10.1145/237170.237216).

Michael Garland and Paul S. Heckbert, **Surface Simplification Using Quadric
Error Metrics**, SIGGRAPH, 1997.
[DOI: 10.1145/258734.258849](https://doi.org/10.1145/258734.258849).

These explain geometric refinement/simplification and error-based tradeoffs.
Ergo's implemented LODs are simpler: detailed catalog mesh, parametric studded
mesh, nominal body box, and coarse colored aggregates. It does not currently
perform QEM decimation or encode progressive mesh streams.

## Rigid-body dynamics, static checks, and constraints

### P1. Contact-force computation

David Baraff, **Fast Contact Force Computation for Nonpenetrating Rigid Bodies**,
SIGGRAPH, 1994. [DOI: 10.1145/192161.192168](https://doi.org/10.1145/192161.192168).

Relevant to unilateral contacts, admissible reaction forces, and the distinction
between static feasibility and merely intersecting bounding boxes. Ergo's server
uses a conservative constructive support/wrench certificate, not this paper's
general contact-force algorithm. Full time integration stays on the client.

### P2. Iterative practical solvers

Erin Catto's [Box2D publications and GDC material](https://box2d.org/publications/)
explain sequential impulses, constraint stabilization, warm starting, contact
manifolds, and practical solver tradeoffs.

[Rapier](https://rapier.rs/docs/) is Ergo's implemented WASM CPU dynamics engine.
[Jolt Physics](https://github.com/jrouwe/JoltPhysics) is useful comparative reading
for broad phases, sleeping, and large rigid-body systems; it is not an Ergo
dependency. Engine architecture informs the design, but does not supply measured
LEGO clutch capacities.

### P3. Compliant positional constraints

Miles Macklin, Matthias Müller, and Nuttapong Chentanez, **XPBD: Position-Based
Simulation of Compliant Constrained Dynamics**, MIG, 2016.
[DOI: 10.1145/2994258.2994272](https://doi.org/10.1145/2994258.2994272).

Useful for a future compliant positioning/drag solver and for understanding the
relationship between compliance, timestep, and iterations. The current editor
uses fixed-group translation checks and force-limited Rapier manipulation;
it is not an XPBD implementation.

### P4. Articulated simulation and model-based control

Emanuel Todorov, Tom Erez, and Yuval Tassa, **MuJoCo: A Physics Engine for
Model-Based Control**, IROS, 2012.
[DOI: 10.1109/IROS.2012.6386109](https://doi.org/10.1109/IROS.2012.6386109).

A related system for articulated bodies, contact, and model-based reasoning.
It illustrates why a hinge's permitted motion and a static contact's support
conditions should be modeled explicitly. Ergo does not embed MuJoCo or claim
equivalent optimization capabilities.

## Concurrent object-store editing

### C1. Optimistic concurrency

H. T. Kung and John T. Robinson, **On Optimistic Methods for Concurrency Control**,
ACM Transactions on Database Systems, 1981.
[DOI: 10.1145/319566.319567](https://doi.org/10.1145/319566.319567).

This motivates read/write validation and why disjoint staged changes can be
rebased while overlapping semantic dependencies must be checked again. The
shared-world REST service implements optimistic dependency validation and root
publication; private snapshots remain a separate workflow.

The separate implemented [`ergo-transactions` library](transactional-storage.md)
now provides that optimistic storage substrate: persistent CBOR octrees,
read/predicate validation, disjoint-edit rebasing, and conditional single-shard
publication. The permissioned service adds bounded on-demand dependency discovery
without loading the world. Unlike an in-memory TL2 adaptation, the library has no per-location
locks or global version-clock object; immutable subtree hashes and replacement
generations provide dependency versions, and one head CAS publishes a transaction.

### C2. Leases and stale holders

Cary G. Gray and David R. Cheriton, **Leases: An Efficient Fault-Tolerant Mechanism
for Distributed File Cache Consistency**, SOSP, 1989.
[DOI: 10.1145/74850.74870](https://doi.org/10.1145/74850.74870).

Useful background for expiry, renewal, and failure recovery. A lease timestamp
alone does not make independent S3 writes transactional. Ergo currently uses
optimistic version checks and atomic root publication. Optional region leases are
future work. GC pins/barriers deliberately do not expire by time; they protect
immutable retention rather than reserving interactive editing regions.

## Earthquake inputs and physical calibration

### E1. Source magnitude versus local shaking

David M. Boore and Gail M. Atkinson, **Ground-Motion Prediction Equations for the
Average Horizontal Component of PGA, PGV, and 5%-Damped PSA at Spectral Periods
between 0.01 s and 10.0 s**, Earthquake Spectra, 2008.
[DOI: 10.1193/1.2830434](https://doi.org/10.1193/1.2830434).

This is an example of the additional modeling involved in predicting local
motion from an earthquake source. Applicability, source distance, site conditions,
regional assumptions, and uncertainty matter. Ergo currently accepts explicit
local PGA bounds or a controlled displacement waveform; it does not implement
this GMPE or silently map a Richter/moment magnitude to a force.

### E2. Geometry and empirical part data

- [LDraw file/unit specification](https://www.ldraw.org/article/218.html): nominal
  geometry units and part-reference transforms.
- [BrickLink 3001 catalog entry](https://www.bricklink.com/v2/catalog/catalogitem.page?P=3001):
  an example of the sourced catalog mass records used by the starter catalog.
- [OSM tile usage policy](https://operations.osmfoundation.org/policies/tiles/):
  source identification, attribution, interactive demand, and caching requirements.

Nominal geometry, catalog mass, and a contact solver do not determine real
connection failure capacities. Those require traceable connector measurements,
as described in [physical calibration](physical-calibration.md). No cited paper
is used as a pretext to invent universal clutch-force values.

## Where to continue reading

- [Display indexing](3d-display-indexing.md) applies S1–S4 to residency, culling, and LOD.
- [Persistence indexing](persistence-indexing.md) separates geographic addresses and render pages.
- [Physics](rigid-body-physics.md) and [positioning](interactive-constraints.md) explain P1–P4.
- [Concurrent editing](concurrent-global-editing.md) develops C1–C2 and the object-store limitations.
- [Solidity tests](solidity-testing.md) explains the explicit waveform/impact inputs behind E1.
