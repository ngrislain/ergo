# Ergo part descriptions and imports

The shared renderer consumes Ergo geometry and per-instance transforms/colors.
Parts are stored as **`ergo-part-1`** descriptions, not external model documents.

## Description fields

`assets/parts/<part-id>.json` contains:

- `format: "ergo-part-1"`, `schema: 2`, and canonical `part` ID.
- `mesh_hash`: SHA-256 of the geometry object's stored CBOR bytes.
- `bounds` and nominal `body_bounds` in public part-local coordinates.
- Optional `catalog_mass: {kg,source}`; this is sourced catalog information,
  not an invented physical measurement or publication permission.
- `credits`: source file/URL, author and license metadata.

Geometry is `objects/<mesh_hash>.cbor`, using Ergo's schema-2 positions, normals,
face colors, bounds and physical metadata. Vertices are unindexed triangles with
nine floats per vertex. Negative face colors inherit the instance color.
The resolver verifies hash, schema and part identity when reading cached geometry.

## Resolution

The catalog recommends 31 standard brick/plate/tile geometries, 17 source-only
architectural/figure/vehicle entries and 20 opaque colors. It exposes the sourced
complete color palette and resolves other
part IDs when actual geometry is available. These recommendations help minimize
piece count; they are not an allowlist or a popularity ranking.

The starter plate set includes 3832 (2×10, 2.91g) and 4282 (2×16, 4.50g), with
nominal dimensions and catalogue masses sourced from their BrickLink records:
[3832](https://www.bricklink.com/v2/catalog/catalogitem.page?P=3832) and
[4282](https://www.bricklink.com/v2/catalog/catalogitem.page?P=4282). Catalogue mass
is not a connector-strength measurement.

For solid monument interiors, real standard bricks 4201 (8×8, 16.30g) and 4204
(8×16, 30.66g) are also supported, rather than scaling smaller parts. Dimensions
and catalogue weights are sourced from BrickLink
[4201](https://www.bricklink.com/v2/catalog/catalogitem.page?P=4201) and
[4204](https://www.bricklink.com/v2/catalog/catalogitem.page?P=4204).

Resolution is cache-first. An import adapter can read a configured local source
library or retrieve a missing source, bake it into Ergo geometry, and store its
native description. Source text is transient in a bounded 32 MiB process cache;
it is not a second persisted asset representation in S3.

`ERGO_ASSET_LIBRARY_DIR` configures the source-library adapter. `ERGO_ASSET_FETCH=0` disables
upstream requests. Known standard shapes can use labeled nominal parametric
geometry when a source is unavailable. Nonstandard parts never become fake boxes.
The renderer retains arbitrary source silhouettes through medium distance tiers.

## Reviewed display-detail library

`crates/client/src/part_lod.rs` contains disposable **render-only** meshes, separate
from the source assets and physical profiles. The district inventory contains no
unlisted catalogue IDs; its missing support was optimized display geometry:

- Standard rectangular bricks/plates/tiles have an eight-sided-stud medium mesh
  and a body-only far mesh. Their nominal dimensions, 0.6-stud stud diameter and
  1/6-brick stud height match the existing parametric geometry; the source asset
  and the 12-sided parametric import fallback are unchanged.
- Window frames **60594** and **60592** have four-bar meshes preserving the open
  aperture, plus the two top studs in medium detail. Glazing recesses, hollow-stud
  bores, underside tubes and clip pockets are omitted from these display proxies.
  Front-aperture coordinates are derived from the official LDraw sources:
  [60594, J.C. Tchang](https://library.ldraw.org/library/official/parts/60594.dat)
  and [60592, Michael Heidemann / LEGO Universe Team](https://library.ldraw.org/library/official/parts/60592.dat).
- Round brick **87081** has a 24-sided medium or 12-sided far annulus, preserving
  its round outline and central pinhole; medium detail retains all twelve top
  studs. Radius 40 LDU, body height 24 LDU, central opening radius 7 LDU and stud
  positions come from [87081](https://library.ldraw.org/library/official/parts/87081.dat)
  and [87081s01](https://library.ldraw.org/library/official/parts/s/87081s01.dat),
  Magnus Forsberg / Orion Pobursky. Underside moulding and snap details remain
  source-only at close range.

The specialty display definitions retain source URLs/authors and CC BY 4.0
attribution. They were reviewed on 2026-10-28. They do not make a missing source
asset publishable or create new masses, colliders or connector capacities.
Near/capture geometry, picking and physical tests still use the original mesh/body
models. Other unreviewed specialty parts retain their source silhouettes.

Generated `view/.../<part>` and `readback-variants/.../<part>` aliases can recover
rectangular display support when their **parametric vertices and nominal body
bounds exactly match** that known part, or when a sourced variant matches a reviewed
whole-vertex SHA-256 and its nominal body bounds. The current source fingerprints
cover official [3001](https://library.ldraw.org/library/official/parts/3001.dat) and
[3022](https://library.ldraw.org/library/official/parts/3022.dat), James Jessiman,
CC BY 4.0, reviewed 2026-10-28. The digest hashes every internal float32 vertex
component in little-endian order, independent of mass/physical-record variants.
A filename suffix, attribution or bounding box alone is not proof of shape;
unknown/modified streams remain source-only. The alias, physical metadata and
frozen source geometry remain intact.
Use `profile_scene --inventory` (see [render profiling](3d-display-indexing.md#measurements-and-repeatable-profiling))
to list parts, source triangle cost, vertex fingerprints and display-profile coverage.
The independent [greedy surface cache](3d-display-indexing.md#cached-greedy-surfaces-hidden-faces-and-coplanar-merging)
can merge distant upright standard bodies without creating new catalogue pieces.

## Physical and material limits

`crates/world/src/part_profiles.rs` adds reviewed catalogue-average masses and
explicit nominal body/connector proxies for frames 60594/60592, slopes 3037/3039,
fence 3633, round bricks 3062b/3941/87081, lamppost 2039, round plate 4073, dish
4740, centre-stud plate 87580, minifigure units 970c00/76382p00/3626a/3626ap01,
and wheel/tyre unit 4624c01. Their original source geometry and credits remain
required; a failed resolution never substitutes a generic rectangular brick.

Bounds use catalogue dimensions and the official LDraw local datum, including
the slope's offset footprint. `get_catalog(part=...)` returns both render `bounds`
and nominal `body_bounds`, plus mass source and geometry credits. Legacy derived
descriptions are upgraded lazily, publishing new hashed native geometry before
the new description; visual vertices are retained unchanged.

These are **nominal proxy models**, not exact hollow/curved collision solids.
Factory figure components use their load-bearing cores; arm/hand overhangs and
connector pegs remain rendered but are outside those collision boxes. Factory
units are rigid in this model, without internal articulation. Do not interpret a
nominal-box support certificate as exact surface/contact or material validation.
In particular, avoid loading empty frame apertures or the empty wedge over a roof
slope. Registered stud and factory-clip relations infer connectivity, never strength.
The ordinary rectangular-stud estimate does not assign capacities to these new
interfaces. Missing capacities remain missing.

The torso assembly's 1.30g is a declared sum of catalogue averages for 973
(0.82g), 981/982 (0.15g each), and two 983 hands (0.09g each); it is not a new
laboratory measurement. Other profile values link their individual BrickLink
records. All were reviewed on 2026-10-27.

Nominal collision boxes intentionally differ from render bounds, which include
studs/details. The server supplies trusted mass/body measurements and connector
capacities from its frozen physical model; uploaded description claims cannot
override them. Missing measurements are disclosed and block the relevant physical
admission. See [calibration](physical-calibration.md).
Collision clearance and connectivity use the same interface lookup in
`part_profiles.rs`; strength estimation remains a separate opt-in mechanism.

Current shading is opaque RGB plastic. Source attribution and licenses are
retained with converted data. Native descriptions do not imply texture, glitter,
metallic or transparent-material rendering support.

Implementation: `crates/assets/src/lib.rs`, `crates/world/src/geometry.rs`,
`catalog.rs`, `physical.rs`, and the server import boundary.
