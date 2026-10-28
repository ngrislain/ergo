# Walkable monument recipes

## District recipes and accepted construction

On 2026-10-28 the eleven new ten-storey buildings and three separate site assemblies
were accepted in the common Sandbox around the unchanged cell-19 tower:
https://ergosandbox.com/app?x=19&y=0&z=0. The new buildings contain 81,305 pieces;
streets, furniture and two footbridges contain 4,998. Including the original
4,962-piece tower, the district contains 91,265 pieces. Street life includes eight
cars, 24 figures, twelve benches and twelve lampposts.

The viewer's native walking engine passed 1,499 explicit waypoints against the
accepted cached geometry: 126 per new building, a 103-waypoint continuous district
route visiting all entrances and crossing both bridges, and two five-waypoint
two-way bridge checks. These used neither jumping nor teleporting. Walking,
accepted static validation, image rendering and dynamics remain separate evidence.

`scripts/build/district.py` authors through the real stdio MCP client. Its durable
`target/district-mcp/progress.json` records stable assembly IDs and each attempted
`place_submodel` publication; unknown outcomes require reconciliation before retry.
Authoring recipes share `scripts/mcp_client.py`; they do not import a demo program
for transport behavior.
It uses publication's authoritative validation without a duplicate diagnostics gate.
The eleven-site layout surrounds the existing cell-19 tower. The recipe implements
foundations, ten nine-course structural storeys with the accepted tower's open
switchback stairs, and supported roof decks. `--through roof --publish` grows the
same assemblies in supported stages. `--through finished --publish` additionally
authors applicable stepped or slab-like extensions, genuine window frames,
round-column details and roof slopes/railings. These stages resolve actual nominal
datums through MCP before generating placements. Street details remain a separate
workflow. Implemented recipes are not claims that every stage is published; consult
actual accepted receipts for publication progress and published storey counts.
Slab-house wings are hollow, with full-width standard 2×16 plate beams bearing on
both perimeter walls and a thinner upper plate layer. This preserves the silhouette
while reducing brick count compared with solid infill.
`district_models.walking_route(spec)` visits rooms on all ten storeys and returns
through the entrance. Its start/end positions use the district foundation and
pyramid/courtyard entrance depths rather than the original monument's wider base.

`--site --through finished --publish` uses the same journalled MCP path for a
separately named boulevard/furniture assembly and two footbridges. Those are
decorations, not extra buildings. Road/curb masks reserve existing foundations,
one-stud modular joints and bridge footprints. The furniture recipe uses catalogue
wheel/tyre units and genuine factory minifigure components; vehicle displays do
not claim rolling/steering or calibrated axle retention. The recorded `decorations`
and `buildings` sections distinguish actual accepted work from planned stages.
The boulevard first publishes one already-planned, grounded paving instance as
`street-anchor`, establishing its assembly identity before the larger road edit.
The remaining `roads-01`… stages exclude that same instance and preserve the
original construction order, with at most 600 expanded leaves per chunk (or a
single indivisible reusable instance). Ground masonry precedes the paving it
supports. Chunking adds no extra bricks or separate street assemblies.
`district_streets.walking_route(spec)` describes a continuous two-way crossing of
either bridge for `check_walk`, including both stair flights. Local prototype
walking evidence does not establish that those assemblies have been published or
that the final combined district route has been verified.

The standalone monument drivers below live in `scripts/build/`. They generate
nominal standard-size parts, consult `get_building_techniques`, and preflight through
the real local MCP. They do not publish implicitly. Choose fresh output directories to preserve prior
checks and publication journals.

## Pyramid

`monument_models.pyramid` builds an original 64×64-stud stepped pyramid on a bonded
terrace. Its eight-stud-wide crossing corridors connect three 12×12-stud chambers;
the interior has nine native units of headroom. Orthogonal 2×16 plate beams bear
on masonry around the openings. Alternating masonry directions bond the mass,
with standard 8×8/8×16 bricks for solid infill and smaller standard bricks on the
exterior; no part is scaled. Two-course/two-stud tiers preserve even-stud bearings.
and entrance pylons and a gold-coloured summit distinguish the entrance/profile.

The visual reference is the
[Great Pyramid of Giza](https://en.wikipedia.org/wiki/Great_Pyramid_of_Giza), not a
replica of its footprint, passage plan or historic dimensions.

## Ten-level tower

`monument_models.tower` builds ten full-size storeys with nine brick courses of
clear room height and separate two-plate floor slabs. Bonded perimeter walls,
supported window/door lintels and masonry stringers carry the floors and open
switchback stair treads. A continuous route ascends all ten levels, visits rooms
and descends without teleporting. Lower wings, a tiered crown and a red/gold
spire are an original interpretation of the
[Moscow State University main building](https://en.wikipedia.org/wiki/Main_building_of_Moscow_State_University)
and the Seven Sisters silhouette, not a scale replica of that much taller building.

The stair flights use four-stud clear walking lanes, brick-height steps and
explicit landings; the room-height guarantee is checked separately from stair
headroom. No escalator, elevator or procedural teleport substitutes for stairs.
Aligned header bands retain broad stud engagements above openings; the running
bond below and staggered floor layers tie those beam segments together.

## Build and check

```sh
python3 scripts/build/monuments.py pyramid --binary ergo-mcp \
  --output target/pyramid-preflight --origin 15872 64 0
python3 scripts/build/monuments.py tower --binary ergo-mcp \
  --output target/tower-preflight --origin 19968 64 0

cargo run --release --locked -p ergo-client --example check_walk -- \
  target/tower-preflight/construction.ergo.json target/tower-preflight/walk-route.json
```

Outputs include native geometry/model files, a studio preview, attributed design
notes, static diagnostics and an explicit walking route. Static, walking, visual
and dynamic checks are different evidence; none implies the others.

For developer-native gravity and the default seeded 0.01g, three-second shake
plus two-second settling check, add `--estimated-connectors` to the build command,
or run:

```sh
cargo run --release --locked -p ergo-world --features simulation \
  --example check_stability -- target/tower-preflight/construction.ergo.json \
  target/tower-preflight/native-stability.json --estimated-connectors
```

This uses the same `ergo-world` solver/load-case implementation and parameters as
the WASM tool, but reports native execution explicitly. It requires the developer
Rust toolchain. `--wasm-estimated-connectors` instead selects the bounded MCP/WASM
job; large dense models may exceed its 1,800-second deadline. A timeout is not a
physical failure or a successful stability result.

All estimated diagnostics disclose `ergo-stud-ballpark-v2`, retain
`calibrated_stability_verified:false`, and do not alter server measurements.
Publication still requires the independent authoritative static gate and an
accepted read-back. The part count must fit 5,000 bodies for complete dynamics.
Large bonded models can satisfy the zero-joint-wrench static certificate while
an iterative dynamic probe flags startup drift or estimated bond breaks. Retain
those diagnostic failures separately; these recipes do not promise earthquake
survival or measured connector calibration.

Building patterns are paraphrased from
[Brick Builder's Handbook](https://brickbuildershandbook.com/basic-lego-techniques/).
Source: `scripts/build/monuments.py`, `scripts/build/monument_models.py`,
`scripts/native_model.py`, and `crates/world/examples/check_stability.rs`.
