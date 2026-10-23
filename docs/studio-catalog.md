# Studio/LDraw catalog and standard-first agent guidance

## Availability versus recommendations

The 27 basic bricks/plates/tiles and 20 opaque colors remain the preferred
recommendations for agent builds. They are **not an import/placement allowlist**.
Any normalized Studio/LDraw `.dat` part ID can be resolved from the persistent
asset cache, configured local library, or the configured LDraw source. This
includes slopes, specialty pieces, and decorated/mold-variant IDs. Paths are
normalized and sandboxed; invalid/traversal/query-bearing part IDs are rejected.

`LDRAW_DIR` can point to a Studio/LDraw library containing `parts`, `p`, and
`unofficial/parts` / `unofficial/p`. The resolver loads bounded dependencies and
retains source/license credits. Official LDraw is fetched on demand by default;
Studio-specific assets absent upstream need the configured local library or
geometry embedded in the model. Ergo does not bundle or claim to redistribute
the full proprietary Studio library.

Standard recommended parts can use labeled parametric geometry when their
source is unavailable. **Non-standard parts require actual geometry** and are
never silently substituted with a generic proxy. Embedded Studio/MPD geometry
and explicit custom RGB colors are retained during import. JSON publication
requires valid complete triangles, finite rigid poses, and the existing geometry
budgets. Non-standard body-box approximations are not exact wheel/clip/hole
collision shapes.

## Colors

`crates/world/src/ldraw_colors.rs` embeds the official LDraw `LDConfig.ldr`
snapshot dated 2026-05-29: 322 definitions, including material/alpha metadata.
Catalog output excludes the two inherited/edge sentinel codes 16 and 24, lists
the 20 preferred colors first, and marks each entry with `recommended`.
Direct RGB codes and model `!COLOUR` definitions are also supported.

The renderer currently uses RGB shading: transparent, metallic, glitter, and
other optical finishes are **not full material simulations**. Their IDs/RGB and
catalog alpha metadata are available without inventing optical or mechanical
properties. The standard ground palette remains the 20 preferred opaque colors.

Per-piece RGB is preserved in private exports and optional shared geometry
`rgb`; older shared geometry without this field uses the current code palette.
The new source palette does not rewrite explicit RGB in existing construction
files. Derived ground cache signatures include the palette values.

Regenerate the embedded snapshot from the official source, then review the diff:

```sh
python3 scripts/update-ldraw-colors.py
cargo fmt --all
```

If the upstream download is unavailable, use an already downloaded official file:
`python3 scripts/update-ldraw-colors.py --source /path/to/LDConfig.ldr`.
The generated header records the upstream URL, update, and source SHA256.

## MCP discovery and placement

`get_catalog` defaults to the standard-first recommendations rather than flooding
an agent's context with a full part/color listing. Set `include_extended: true`
to return the full embedded color catalog. Set `part: "3039"`, for example, to
resolve/inspect a non-standard part's geometry and available mass before placing
it. `place_brick` accepts any resolvable part ID and supported color code; use
standard parts/colors when they express the same design.

The `parts` array is the recommendation list, not an exhaustive list of all
Studio molds. The response's `part_resolution` explains on-demand resolution.
Geometry selection, bounds, and colors can be edited locally even when physical
measurements are missing; feedback reports that absence explicitly.

## Measured physics remains required

The broader visual catalog does not grant publication readiness. Every part
needs a sourced mass and nominal body bounds, and every declared/inferred
connection needs the server-owned capacity data already required by Ergo.
No visual-only publication mode or guessed solid-volume mass is introduced.

The 27 standard parts retain their built-in sourced mass/nominal dimensions.
For another part, add a `parts` record to `ERGO_CONNECTION_MEASUREMENTS`:
`part`, positive `mass_kg`, `body_bounds` (`min`/`max` in public X/Y/Z construction
coordinates relative to the part reference), traceable `source`, and positive
`samples`. These records accompany the existing `entries` connection array.
The server overwrites client mass/bounds claims from this operator-owned book;
shared worlds freeze the complete measurement book/model at creation. New
measurements require a new world or a future explicit model migration, not an
implicit reinterpretation of an existing shared world.

The local MCP `--measurements` option accepts the same book for placement and
diagnostics. Missing records block physical tests/publication with an actionable
error. Paid entitlement changes pacing only, not this requirement. See
[physical calibration](physical-calibration.md) for units, force/torque records,
model assumptions, and sourced-data requirements.
