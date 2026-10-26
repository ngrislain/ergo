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

The catalog recommends 29 standard brick/plate/tile geometries and 20 opaque
colors. It also exposes the sourced complete color palette and resolves other
part IDs when actual geometry is available. These recommendations help minimize
piece count; they are not an allowlist or a popularity ranking.

The starter plate set includes 3832 (2×10, 2.91g) and 4282 (2×16, 4.50g), with
nominal dimensions and catalogue masses sourced from their BrickLink records:
[3832](https://www.bricklink.com/v2/catalog/catalogitem.page?P=3832) and
[4282](https://www.bricklink.com/v2/catalog/catalogitem.page?P=4282). Catalogue mass
is not a connector-strength measurement.

Resolution is cache-first. An import adapter can read a configured local source
library or retrieve a missing source, bake it into Ergo geometry, and store its
native description. Source text is transient in a bounded 32 MiB process cache;
it is not a second persisted asset representation in S3.

`ERGO_ASSET_LIBRARY_DIR` configures the source-library adapter. `ERGO_ASSET_FETCH=0` disables
upstream requests. Known standard shapes can use labeled nominal parametric
geometry when a source is unavailable. Nonstandard parts never become fake boxes.
The renderer retains arbitrary source silhouettes through medium distance tiers.

## Physical and material limits

Nominal collision boxes intentionally differ from render bounds, which include
studs/details. The server supplies trusted mass/body measurements and connector
capacities from its frozen physical model; uploaded description claims cannot
override them. Missing measurements are disclosed and block the relevant physical
admission. See [calibration](physical-calibration.md).

Current shading is opaque RGB plastic. Source attribution and licenses are
retained with converted data. Native descriptions do not imply texture, glitter,
metallic or transparent-material rendering support.

Implementation: `crates/assets/src/lib.rs`, `crates/world/src/geometry.rs`,
`catalog.rs`, `physical.rs`, and the server import boundary.
