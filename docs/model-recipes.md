# Walkable monument recipes

Developer recipes live in `scripts/build/`. They generate nominal standard-size
parts, consult `get_building_techniques`, and preflight through the real local MCP.
They do not publish implicitly. Choose fresh output directories to preserve prior
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
