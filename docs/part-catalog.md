# Ergo part descriptions and imports

The shared renderer consumes Ergo geometry and per-instance transforms/colors.
Every buildable part is an entry of the single [part library](part-library.md)
(`ergo-part-library-2`), not an external model document.

## Library entries

A library entry (`ergo_world::part_library::LibraryPart`) contains:

- `revision`: SHA-256 over the synthesis version, the part's pinned source
  closure and its catalogue record.
- `vertex_hash`: SHA-256 of the complete internal float32 vertex stream.
- `mesh_hash`: SHA-256 of the native Mesh CBOR at `objects/<mesh_hash>.cbor`.
- `catalogue`: `id`, `name`, LDraw `category` and `keywords`, deduplicated
  `bricklink` numbers, `rebrickable` ID, identity `aliases`, `mass_kg` and
  `mass_source`, then exactly one of:
  - a reviewed rectangular `size` (+ `studs`);
  - a source-LDU `nominal` profile, plus optional reviewed `connectors` and
    `set_count`, and `derivation` for derived parts.

Geometry objects use Ergo's schema-2 positions, normals, face colors, bounds and
physical metadata, and carry their LDraw file credits (author, licence, source
URL). Vertices are unindexed triangles with nine floats per vertex. Negative face
colors inherit the instance color.

## Resolution

`ergo_assets::PartCache::resolve(id)` does the following:

1. Maps the ID or alias to its library entry. Unknown IDs are errors.
2. Reads the immutable `objects/<mesh_hash>.cbor`.
3. Verifies the CBOR hash, schema, part identity, `source` quality and vertex hash.
4. Keeps a 64 MiB LRU of decoded meshes.

There is no runtime upstream fetch, local LDraw directory or per-part
description record. The [synthesis](part-library.md#deterministic-synthesis)
is the only producer of geometry. A missing object fails rather than introducing
a box substitute. Resolution is concurrent, with no global lock.

MCP mirrors fetch missing objects from the origin's `/api/part-objects/{hash}`,
verify them and store them locally. The rendered envelope (`mesh.bounds`)
includes studs and details. The nominal body comes from the catalogue record.

The reviewed tier recommends 44 standard brick/plate/tile geometries and 94
architectural, figure, vehicle and Technic entries, with 20 opaque colors. It also
exposes the complete sourced LDraw palette. These recommendations help minimize
piece count; they are not an allowlist. Derived parts are buildable too.

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

Long pedestrian spans can use genuine **3026 Plate 6×24**, 19.43g, rather than
scaled smaller plates. Its nominal dimensions and catalogue mass are sourced from
[BrickLink 3026](https://www.bricklink.com/v2/catalog/catalogitem.page?P=3026).

Imports (`/api/import`) take library parts' geometry from the library. Parts that
are not in the library are baked from geometry embedded in the uploaded
MPD/Studio document. Without a sourced operator part record they cannot be
admitted.

## Derived profiles (auto-v1)

`crates/world/src/derive.rs` profiles every non-reviewed part from its baked
official geometry in source LDU (LDraw axes, Y down):

- **Top studs:**
  - upright, unit-scaled placements of the standard stud primitives (`stud`,
    `stud2`, `stud2a`, `stud6`, `stud6a`, `stud10`, `stud15`, `stud17a`),
    collected during baking;
  - sideways studs are not used.
- **Body envelope:** the mesh bounds, with the 4-LDU stud protrusion removed
  when studs are the highest geometry.
- **Receiving grids:** stud-lattice positions (0- or 10-LDU phase, or the top-stud
  phase) on a downward-facing plane within one plate of the lowest point.
  1. Each position must leave a stud's height free above the plane.
  2. It must have a ceiling within the body and nothing below its mouth.
  3. It must be surrounded by gripping material: 16 horizontal rays 2 LDU above
     the plane must hit within 11 LDU with no angular gap over 180°. Empty space
     under an arch opening is therefore not a socket.
  4. Open positions are grouped into maximal rectangles.
- **Mass:** a three-axis majority vote of ray-parity voxel fills (≤ 8 M voxels)
  gives the solid volume. It is multiplied by ABS density 1.05 g/cm³ and the
  library's calibration factor. `mass_source` states it is an estimate.
- **Not derived:**
  - connectors (Technic bores, pins, axles, bars, clips);
  - compound collision boxes;
  - bearing regions beyond the body box;
  - any connector capacity.

On the 138 reviewed parts, auto-v1 reproduces:

- 132/138 stud sets exactly;
- 115/138 receiving-grid sets exactly;
- 112/138 body boxes within 0.06 units.

Most differences are deliberate reviewed choices, such as bearing cores for
minifigure parts, trees and capes. The remaining misses are conservative: no
socket is derived where the reviewed part has one. Measure with
`cargo run -p ergo-world --features test-library --example derive_check -- /path/to/ldraw`.

Derived parts therefore rest, stack and connect through studs and sockets.
Their envelopes are nominal boxes, which are coarser than reviewed cores.
Joint-dependent publication still requires measured capacities, as for every part.

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
are generated for the complete selected official library, with each part's original
author/license retained. The digest hashes every internal float32 vertex
component in little-endian order with signed zero normalized, independent of
mass/physical-record variants and deterministic CBOR axis roundtrips.
A filename suffix, attribution or bounding box alone is not proof of shape;
unknown/modified streams remain source-only. The alias, physical metadata and
frozen source geometry remain intact.
Use `profile_scene --inventory` (see [render profiling](3d-display-indexing.md#measurements-and-repeatable-profiling))
to list parts, source triangle cost, vertex fingerprints and display-profile coverage.
The independent [greedy surface cache](3d-display-indexing.md#cached-greedy-surfaces-hidden-faces-and-coplanar-merging)
can merge distant upright standard bodies without creating new catalogue pieces.

## Unlisted HTML reference

The server serves `/catalogue` and `/catalogue/` without adding a main-app
navigation link. The page declares `noindex,nofollow`; it is an unlisted public
reference for already-public catalogue data, not an authentication boundary.

APIs:

- `/api/catalog`: colours, constructs, the reviewed parts and a `library`
  summary (revision, reviewed/derived/listed counts, mass calibration).
- `GET /api/catalog/parts?q=&category=&tier=&cursor=&limit=`: paged search over
  the whole library.
  - `tier` is one of `all`, `buildable`, `reviewed`, `derived`, `listed`;
    `limit` is at most 128.
  - Results are in one natural ID order across every tier (`2` < `10` < `3001`
    < `3001b`); listed items use their BrickLink number.
  - Cursors are offsets bound to the immutable library revision.
  - Matching is case-insensitive over ID, name, aliases, BrickLink numbers,
    category and keywords. `2x4`, `2 x 4` and `2 × 4` match alike.
  - Category facets are returned with the results.
- `/api/parts/{id}`: one verified asset with its library entry.

Part records include identity aliases and BrickLink numbers, `mass_kg`,
`mass_source`, `derivation` for derived parts, `nominal_dimensions_mm` in
east/north/up order, `top_studs`, `receiving_grids` and `library_revision`.
Each receiving grid supplies width/depth and its first centre in public
part-local construction coordinates. Mechanical port radii remain SI metres.

The reference page provides:

- server-side search with paged results;
- library tier and category filters (the default shows everything, ordered by ID);
- a Reviewed/Derived/BrickLink-only badge per card;
- real source-mesh thumbnails;
- an orbit/zoom/keyboard preview and optional interface markers.

Previews and thumbnails share one camera-relative key light (upper left,
behind the viewer) with two-sided shading, so official parts with
inconsistent face winding shade the same way.
It distinguishes nominal physics envelopes from rendered envelopes, labels
derived masses as estimates, and links mass records and geometry credits.
BrickLink-only items show their BrickLink link and no preview; no generic box
substitutes for missing geometry.

Colour controls use the LEGO/LDraw preview palette and retain fixed printed face
colours. They do not claim every mould was manufactured in every palette colour.
Swatches use catalogue RGB/alpha; preview lighting and simple transparency are
illustrative, not metallic/refraction/material qualification.

Verification: `scripts/test-catalogue.py` runs against the isolated real server
through `scripts/test-web-isolated.py`, which publishes the synthesized fixture
library. It checks reviewed paging, tiers, a derived part and an alias, source
geometry, colour changes, keyboard orbit, interfaces and mobile layout.

## Physical and material limits

Reviewed records in `crates/world/library/reviews.json` (consumed through
`crates/world/src/part_profiles.rs`) add catalogue-average masses and
explicit nominal body/connector proxies for frames 60594/60592, slopes 3037/3039,
fence 3633, round bricks 3062b/3941/87081, lamppost 2039, round plate 4073, dish
4740, centre-stud plate 87580, minifigure units 970c00/76382p00/3626a/3626ap01,
and wheel/tyre unit 4624c01. Their original source geometry and credits remain
required; a failed resolution never substitutes a generic rectangular brick.

Reviewed additions on 2026-10-29 support genuine **3901 hair** (0.30g) and classic
vehicle parts **4211 base** (2.71g), **3788 mudguard** (1.41g), **3823 windscreen**
(2.19g), **4083 roll bar** (0.74g) and **3829c01 steering unit** (0.60g). Each profile
links its individual BrickLink item record; local datums follow official LDraw
geometry. These are source-only parts, not fabricated box assets.

Hair's nominal collider is its upper load-bearing cap; strands wrapping the head
remain visual. Vehicle shells use their lower structural floors as bearing
cores, excluding wheel arches/passenger cavities from filled collision boxes.
The steering factory unit uses its baseplate core; the stem/wheel remain rendered.
Windscreen/roll-bar boxes remain nominal hollow/wedge proxies, not exact solids.
Registered wheel bearings infer rotation, but axle retention capacity, steering
articulation, exact hollow contact and calibrated connector strength are not inferred.
Agents should use demonstrated model datums and
avoid loading empty apertures. Source geometry and full catalogue mass remain.

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

### Castle equipment and printed heraldry

Reviewed 2026-11-02 additions pin official geometry and linked BrickLink masses:
**3844 helmet** (0.64g), **4497 spear** (0.40g), **3846p4h shield** (0.42g),
**3957a antenna** (0.42g), **4495a pennant** (0.37g), and **76382p4f Lion Knights
torso with arms/hands** (1.25g, BrickLink 973px138c01). The printed torso is a
distinct genuine asset, not a colour overlay or an alias to the unprinted torso.

The helmet uses its upper bearing cap, leaving the neck protector visual; the
factory torso keeps the existing torso-core interface. Spear/shield mass/support
profiles are nominal envelopes; collision checks also use source-defined shield
face/handle boxes and separate spear blade/shaft boxes. The flag retains its full asymmetric footprint
and mass; its bearing face follows the cloth hem at 20 LDU, while the lower hollow
stud extends to 24 LDU. Its receiving datum remains at that lower connector.

The Castle standard's overlap clearance is the source-defined 3.2mm antenna shaft through
the pennant's hollow stud: upright, axis-aligned parts, coaxial within tolerance,
and only within the straight shaft's axial range. Wrong parts, lateral offsets,
tilts, the wide antenna foot and placement above the tip remain rejected. Client
edits, group moves, server admission and inferred connectivity share this rule.
This geometric clearance does not establish retention, friction or clutch strength.
The supported standard has a capped bearing post under the cloth; removing it
requires measured connector capacities for any force/moment consumed by admission.
Dynamic tests retain their nominal box/contact approximations; this is not an
exact annular-contact simulation or a new calibrated connection model.

Nominal collision boxes intentionally differ from render bounds, which include
studs/details. The server supplies trusted mass/body measurements and connector
capacities from its frozen physical model; uploaded description claims cannot
override them. Missing measurements are disclosed and block the relevant physical
admission. See [calibration](physical-calibration.md).
Collision clearance and connectivity use the same interface lookup in
`part_profiles.rs`; strength estimation remains a separate opt-in mechanism.

Current shading uses lit RGB plastic and catalogue alpha for transparent colours,
including source windscreens. Source attribution and licenses remain with converted
data. Fixed baked face colours remain opaque; texture, glitter, metallic rendering,
refraction and coloured transmission are not implied by a native description.

## Hand, wheel and Technic attachments

`crates/world/src/attachments.rs` shares source-local mating rules across relation
inspection, inferred constraints and finite bore-clearance checks. Port definitions
are reviewed records in `crates/world/library/reviews.json`.
They distinguish 3.2 mm bars/grips, Technic round pins/bores, cross axles/keyed bushes,
small wheel pins/bearings, and explicitly matched rim/tyre groups.

- Factory torso grips use the actual transformed **3820 hand** datums, including
  the 14.5-degree local hand angle. Standalone hands and genuine accessory handles
  use the same lookup. New guard/archer poses use separate source arms/hands and
  real shoulder/wrist interfaces rather than forcing upright equipment into
  fixed factory hands. A nearby tool with an offset, wrong diameter or wrong axis
  does not become held just because its envelope touches the figure.
- Technic **3700/3701/3894/3702/3895/3703/6541/32000** bricks and the reviewed
  straight beams expose their individual bores. Friction pins infer fixed joints;
  frictionless pins and cross axles through round bores infer revolute bearings.
  Cross axles in bushes require quarter-turn key registration and infer fixed joints.
- **4600** wheel holders and **4624/4624c01** wheel bearings infer revolute joints.
  Rims and tyres require their explicit matching group and coincident seating.
  Wrong rims, absent engagement, mis-keyed axles and affine/scaled poses fail mating.
- Standard stud/cavity inference also follows proper metric rotations, including
  sideways assemblies; catalogue force estimates retain their narrower existing scope.

Fits require coaxial axes and full functional receiving-span engagement within
tolerance. Body overlap is cleared only when the complete nominal intersection
fits a matching finite bore prism. Other collisions on the same pair remain
collisions. Static/edit collision preflights use oriented-box SAT, with compound
boxes for shield face/handles, spear blade/shaft and bow limbs/string/arrow; these remain approximate
solids, not exact mesh cavities. Dynamic colliders share those compound proxies.
The older restricted antenna/pennant bore rule remains available.

Peripheral grips/pins extend beyond bearing cores. Local inference indexes their
port bounds; server dependency discovery expands body-index queries by the reviewed
port overhang, so it can find a held object starting from either endpoint.

Connector anchors and free axes feed fixed/revolute joints, leaving measured force
and torque capacities unset. Tilted pieces can use joint-supported load paths, but
their envelope faces never become invented compression-bearing patches. Tyres
support the ground below the axle line, without an assumed rolling-resistance
moment. Feet snap/register by receiving stud centres rather than rounded envelope
edges. These rules support plate-free v2 constructs without declaring their joints
calibrated. See [calibration](physical-calibration.md).

Authoritative admission also validates **claimed** joints against source
interfaces: canonical anchors, fixed/revolute kind, bearing axes and absence of
invented stops. Side-face contact or a measured force record cannot authorize an
unregistered weld. Duplicate part-pair joints are rejected. Uploaded specialty
vertices must match the pinned complete stream, including direct immutable mesh
lookups; a caller cannot move the rendered hand/bore while retaining its trusted
catalogue connector. This runs before any zero-joint-wrench shortcut.

Implementation: `crates/assets/src/lib.rs`, `crates/world/src/attachments.rs`, `geometry.rs`,
`catalog.rs`, `physical.rs`, and the server import boundary.
