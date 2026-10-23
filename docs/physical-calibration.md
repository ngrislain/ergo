# Real dimensions, masses, and connection capacities

## Separate physical measurements from spatial addressing

Global storage uses logical brick coordinates and GPS-addressed cells. Neither
the GPS cell scale nor the renderer's camera distance supplies a physical mass
or length. The native/WASM physics engine converts construction coordinates with:

$$
D=\operatorname{diag}(0.008,0.008,0.0096)\quad\text{meters per public construction unit},
$$

$$
\mathbf{x}_{SI}=D\mathbf{x}_{world},\qquad
A_{SI}=DA_{world}D^{-1}.
$$

Thus a nominal 2×4 brick occupies 16×32 mm horizontally and 9.6 mm in body height.
A plate is 3.2 mm high. The nominal stud diameter is 4.8 mm and stud height 1.6 mm.
These dimensions follow the [LDraw unit specification](https://www.ldraw.org/article/218.html#ldu):
20 LDU per stud pitch, 24 per brick height, 8 per plate, 12 per stud diameter,
4 per stud height, with the documented approximate conversion of 0.4 mm/LDU.

These are **nominal dimensions**, not a claim that a manufactured brick's outer
walls fill the entire 8 mm pitch. Molding clearances, hollow walls, and mold
variants need more detailed dimensional records if the approximation matters.
Decorative studs are excluded from body collision boxes to allow interlocking.
The browser and headless renderer preserve the 9.6/8=1.2 vertical aspect ratio.
Import/publication rejects metric scaling or shear: a catalog brick cannot be
stretched to a different size while retaining the original part's measured mass.

Gravity is $9.80665\ \mathrm{m\,s^{-2}}$ in the public $-Z$ direction. Solver mass is
in kilograms; connection forces are newtons and moments are N·m.

## Sourced per-part mass

`crates/world/src/physical.rs` contains mass values read from BrickLink's Item
Info catalog on 2026-10-17. Every starter part has a mass record and a source URL.
Examples:

- [3001, brick 2×4](https://www.bricklink.com/v2/catalog/catalogitem.page?P=3001):
  2.32 g, stored as 0.00232 kg.
- [3003, brick 2×2](https://www.bricklink.com/v2/catalog/catalogitem.page?P=3003):
  1.35 g.
- [3005, brick 1×1](https://www.bricklink.com/v2/catalog/catalogitem.page?P=3005):
  0.44 g.
- [3020, plate 2×4](https://www.bricklink.com/v2/catalog/catalogitem.page?P=3020):
  1.20 g.
- [3068, tile 2×2](https://www.bricklink.com/v2/catalog/catalogitem.page?P=3068):
  0.49 g; the LDraw alias is `3068b.dat`.

`scripts/refresh-part-masses.py` fetches and prints source values for review; it
does not automatically overwrite the physics dataset. Catalog masses can vary
with color, mold revision, and measurement quality. They are a much better
starting point than assigning one arbitrary mass or multiplying a solid body box
by ABS density, but do not constitute a laboratory calibration of every part.

The simulator assigns the sourced total mass to a uniform box inertia
approximation. Hollow-part center of mass and inertia measurements are future
refinements. The current friction/restitution coefficients are solver assumptions,
not sourced material measurements, and are disclosed as such in validation reports.

## Connection strength must be measured

There is no universal “LEGO joint strength” that can safely be inferred from a
part name or its visual mesh. Relevant variables include part pair, engagement
footprint, molding/material, age/wear, connection direction, assembly history,
load rate, temperature, and whether loading is tension, shear, bending, or torsion.

In particular, a brick's compression-to-failure measurement is not its stud
pull-off force. Dividing a full eight-stud measurement by eight and multiplying
by a different engagement count is not automatically valid.

The repository therefore does **not** ship invented default clutch capacities.
Local drafts and read-only fixture examples can run with idealized joints, but
ordinary server publication requires operator-owned measured capacities for
every connection in the published model.

## Measurement-book format

Set `ERGO_CONNECTION_MEASUREMENTS` to a JSON file containing an `entries` array.
Each entry must specify:

- `part_a`, `part_b`: canonical LDraw IDs, including `.dat`.
- `kind`: `fixed` or `revolute`.
- `overlap_studs`: the horizontal X/Y engagement footprint used for this tested pair.
- `conservative_force_limit_n`: a positive measured lower-bound combined force
  capacity in newtons.
- `conservative_torque_limit_nm`: a positive measured lower-bound combined moment
  capacity in N·m.
- `source`: a traceable test report or dataset identifier.
- `samples`: positive number of tested samples.

Use values from actual measurements. Missing values should remain missing and
block calibrated publication; do not populate a plausible-looking example with
fabricated newton values. The MCP binary accepts the same book through
`--measurements`.

The same file can include an optional `parts` array for non-standard catalog
items. Each record provides canonical `part`, positive `mass_kg`, nominal
`body_bounds` (`min` and `max` in public X/Y/Z construction coordinates relative
to the part reference), traceable `source`, and positive `samples`. Bounds must
be finite and have positive extents. These are operator-owned measurements,
not guesses from mesh volume. Duplicate/noncanonical part IDs are rejected.
Server normalization overrides client mass/bounds metadata before validation;
shared worlds freeze these records with their physical model. Missing records
allow local geometry inspection but block physical tests/publication. Standard
parts retain their built-in sourced records unless the operator supplies an
explicit measured override. See [Studio catalog support](studio-catalog.md).

The server ignores capacities claimed in an uploaded working copy and overwrites
them from its own book before publication. Stored constraints retain the selected
force/torque thresholds and source. Different overlap footprints need separate
measurements. The initial matcher is deliberately restricted; a production book
should also key mold revision, orientation, contact topology, material, and wear.

## Failure criterion and its limitations

For a joint's solved linear/angular impulse at step $\Delta t$:

$$
\hat F=\frac{\|\boldsymbol\lambda_{linear}\|}{\Delta t},\qquad
\hat M=\frac{\|\boldsymbol\lambda_{angular}\|}{\Delta t}.
$$

A joint breaks if either estimate exceeds its configured conservative capacity.
This first model uses a combined isotropic envelope. It does not yet distinguish
safe compression from tensile pull-off or model directional shear/bending
interactions. Measurement values must be chosen for this conservative combined
envelope, not copied from an unrelated single-axis failure test.

Impulse estimates are timestep/solver-dependent. Compare against controlled tests,
study convergence, and model uncertainty before calling them realistic structural
predictions. A future directional interaction envelope could use, for example:

$$
\left(\frac{F_{tension}}{F_{pull}}\right)^p+
\left(\frac{\|\mathbf{F}_{shear}\|}{F_{shear,max}}\right)^p+
\left(\frac{\|\mathbf{M}_{bend}\|}{M_{bend,max}}\right)^p+
\left(\frac{|M_{twist}|}{M_{twist,max}}\right)^p\le1,
$$

where even the exponent and interaction form require experimental support.

## Publication versus examples

The generated architectural examples have authored idealized fixed fixture bonds.
Their validation report explicitly identifies them as read-only fixture examples,
not calibrated real-world connection-strength predictions. Editing an example
creates a working copy; publishing that copy goes through the ordinary measured
capacity gate and cannot inherit the example exemption.

For measured assemblies the server checks a constructive static wrench assignment
against connection capacities; it runs no dynamic fallback. A general optimizer
that searches alternative load distributions is future work. Client dynamic
working sets remain bounded to 5,000 bricks; short gravity probes default to one
step with explicit pose tolerances.
