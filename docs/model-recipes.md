# Walkable monument recipes

## District recipes and accepted construction

The building-link/street baseline has **91,543 pieces in the same 15 assemblies**
(2026-10-29). Its revisions below supersede the original
road-bridge and figure/car details in the 2026-10-28 baseline record.

On 2026-10-28 the eleven new ten-storey buildings and three separate site assemblies
were accepted in the common Sandbox around the unchanged cell-19 tower:
https://ergosandbox.com/app?x=19&y=0&z=0. The new buildings contain 81,305 pieces;
streets, furniture and two footbridges contain 4,998. Including the original
4,962-piece tower, that initial district contained 91,265 pieces. Street life included eight
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
foundations, profile-specific five-to-ten nine-course structural storeys with the accepted tower's open
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
`district_models.walking_route(spec)` visits rooms on every retained storey and returns
through the entrance. Its start/end positions use the district foundation and
pyramid/courtyard entrance depths rather than the original monument's wider base.

`--site --through finished --publish` uses the same journalled MCP path for a
separately named boulevard/furniture assembly and two bridge assemblies. Those are
decorations, not extra buildings. Road/curb masks reserve existing foundations,
and one-stud modular joints. The furniture recipe uses catalogue
wheel/tyre units and genuine factory minifigure components; vehicle displays do
not claim rolling/steering or calibrated axle retention. The recorded `decorations`
and `buildings` sections distinguish actual accepted work from planned stages.
The boulevard first publishes one already-planned, grounded paving instance as
`street-anchor`, establishing its assembly identity before the larger road edit.
The remaining `roads-01`… stages exclude that same instance and preserve the
original construction order, with at most 600 expanded leaves per chunk (or a
single indivisible reusable instance). Ground masonry precedes the paving it
supports. Chunking adds no extra bricks or separate street assemblies.
The original two stair-flight crossings remain available as
`district_streets.legacy_bridge()` and `legacy_walking_route(spec)` for interpreting
the 2026-10-28 receipts. Current `district_streets.walking_route(spec)` delegates
to the building-link route described below. Local prototype walking evidence does
not establish that a revision has been published or that the final combined
district route has been verified.

### Building-link and street-detail revisions

`scripts/build/district_skybridges.py` defines two third-storey links: Copper
Needle–White Slab and Colonnade–North Arch. Six-stud-wide, two-layer decks use genuine
6×24 plates with staggered upper seams, standard guardrails and split piers on
the pavements. Their routes traverse both complete endpoint buildings and return
without stairs in the traffic lanes.

`scripts/build/district_revisions.py` derives bounded atomic replacements from
complete authoritative construction caches. Portal edits preserve the surrounding
walls and add supported lintels; street edits replace exactly eight old cars and
24 plate caps, and restore road surfaces after the old stair bridges are removed.
Unchanged occurrences retain their IDs. A fresh district build must also apply
the endpoint portal edits before its links are usable; the base building stages
do not create these openings automatically. Replacement files are proposals, not
shared commits: use authenticated MCP `place_submodel(replace_ids=..., model=...)`
and retain accepted receipts/readbacks before recording a revision as live.

The cars adapt LEGO **6505 Fire Chief's Car**, modeled by **Robert Paciorek
[bercik]** in the [official LDraw OMR](https://library.ldraw.org/omr/sets/610),
under its dual CCAL 2.0 / BSD-MIT license. Genuine chassis, mudguards, windscreen,
roll bar and steering retain the classic open-cabin silhouette; the derivative
adds a gravity-supported chassis and uses the existing factory tyre units.
Figures use genuine **3901** hair. Vehicle/hair collision proxies and unmeasured
interfaces are described in [the part catalogue](part-catalog.md); neither source
attribution nor walking diagnostics certify dynamic or calibrated clutch strength.

### Accepted 2026-10-29 revision and verification

The four doorway edits, both atomic bridge relocations and the street replacement
were accepted through authenticated MCP, preserving their existing assembly IDs
and unchanged local occurrences. The completed bridges were then renamed through
MCP. The final accepted revision is
`2c6d3d3a102ae9294bd86f6ad7f7e3387516f6f116ed4c93913c24b2c450eb31`.

Copper Needle contains 4,585 pieces, White Slab 7,750, Colonnade Exchange 4,628
and North Arch 7,750. Each skybridge contains 140 pieces; the existing boulevard
assembly contains 4,964. The eleven newer buildings total 81,337 pieces; site
assemblies total 5,244. The original 4,962-piece tower retains its existing assembly.
There remain twelve buildings overall, including that original tower; bridges and
street details are separate decorations, not additional buildings.

The pinned public 91,543-piece region contains all fifteen district assemblies
fully resident. Its actual street inventory includes eight each of the genuine
4211 chassis, 3788 mudguard, 3823 windscreen, 4083 roll bar and 3829c01 steering
units, thirty-two 4624c01 factory wheel/tyre units and twenty-four 3901 hair pieces.
The two retired bridge intersections are filled with supported road masonry and
markings; the links connect the buildings at third-storey height.

Complete-region walking checks passed **546 explicit waypoints**: 126 for each of
the four modified buildings, sixteen for each two-way skybridge crossing and five
for each two-way restored-road crossing. The same native viewer engine checked
four-unit clearance, arrival floors and continuous movement, with no jumping or
teleporting. Actual local WASM/WebGL2 studio images of the accepted public packet
confirmed both links, the classic open-cabin cars and genuine hair.

The public CBOR packet SHA-256 is
`c593a05084fbfa7d6b8e0ee03fe5f5a4238af7551af81584f44c3f1d3b292009`.
Its derived read-only native walking interchange SHA-256 is
`112a715b03eb7767de6cbf13ae3647933db5e660298560fc99d3fdb6158ae586`.
The combined file is diagnostic interchange, not a new editable assembly or a
combined dynamics/calibrated-strength certificate. Authoritative static acceptance,
walking and image evidence remain separate.

A five-minute gateway failure during the White Slab edit was reconciled by an
authoritative read confirming the original state before an explicit retry. Long
edits subsequently used the same released API on loopback with unchanged deployed
S3/common-world, policy, signing, pacing and frozen physics settings and a valid
human-issued delegation. Full affected assemblies were warmed by read-only MCP
opens before further writes. Actual edits still passed the authoritative API's
ordinary validation/CAS path, exact readbacks and a final public-region read;
local JSON changes were not used as shared commits. The temporary API workers were
stopped after all requests and verification completed.

The standalone monument drivers below live in `scripts/build/`. They generate
nominal standard-size parts, consult `get_building_techniques`, and preflight through
the real local MCP. They do not publish implicitly. Choose fresh output directories to preserve prior
checks and publication journals.

### Varied-height and intermediate-terrace refinement recipes

Current `district_models.SPECS` select six core heights from five through ten full
storeys: Azure eight, Copper ten, White Slab eight, Emerald five, Ochre seven,
Civic nine, Crimson six, Sandstone nine, Colonnade seven, North Arch eight and
Silver six. The original tower is a separate unchanged assembly. White Slab's
side wings end after four/six storeys, Crimson's after two/four and North Arch's
after three/five. Their existing supported 16×32 next-floor slabs become outdoor
terraces with three-sided upright catalogue guardrails, supported access lintels
and doors from the corresponding core floor. Each retained room still has nine
brick courses. The walking route includes the intermediate terraces.

`scripts/build/district_refinements.py` prepares atomic edits from full accepted
assembly caches. It removes upper levels, replaces only the changed last-storey
stair occurrences, closes the shortened roof on the existing stringers and moves
the original crown. A generated brick matching a removed source occurrence in
part, colour and pose keeps its original ID instead of being needlessly replaced.
The bridge-side third-storey openings are below the edits and remain usable.
Building techniques reuse the existing bonded masonry and overlapping slabs;
[official OMR **10243 Parisian Restaurant**](https://library.ldraw.org/omr/sets/487), modeled by **Willy Tschager [Holly Wood]**
under CCAL 2.0, was inspected as a terrace reference. The district proposals are
original adaptations, not an imported replica of that set.

Opposite skybridge fence rows rotate genuine 3633 sections by 180° about the
vertical axis. They have fourteen sections each, equal lengths/heights and
inward-facing matching profiles. Reflection checks against the actual frozen
catalogue mesh use a 0.002-stud tolerance for rounded LDraw source coordinates
(observed maximum error 0.0014 stud); no reflected/scaled physical transforms are
used. Local preview/route checks and source recipes do not establish shared
acceptance: retain the separate authenticated MCP receipts and accepted readbacks.

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

The current `tower()` recipe resolves genuine 60594/60592 frames before generation.
It has a rusticated grey entrance/podium, lower five-course window bays with paired
transoms, regular framed middle-storey bays and six-course stacked upper-storey openings.
Recessed frames, belt-course colours and a gold-accented upper floor distinguish
base, middle, upper shaft and existing stepped crown. Room heights and the open
ten-storey switchback route are retained. `tower(facade=False)` generates the older
unframed recipe for interpreting historical receipts. The accepted facade is
recorded separately below.

The facade's base/shaft/crown rhythm was researched using the official OMR
[21046 Empire State Building](https://library.ldraw.org/omr/sets/987), modeled by
Orion Pobursky [OrionP], CCAL 2.0. Its microscale SNOT/grille construction is a
visual reference; this minifigure-scale design uses upright sourced frames and
bonded masonry rather than copying that model or its scale.

### Accepted tower facade (2026-10-30)

The existing tower assembly was updated at revision
`116c9fca408e798aa5040e34782483badaea687fae6693a9a53896d32bd8203e`.
It now contains **5,151 pieces**, including **159 genuine 60594 frames and 66
60592 frames**. The atomic edit retained 3,996 existing occurrences, removed 966
changed ones and added 1,155; its assembly identity and absolute position remain.
Independent authoritative readback matched the exact intended part/colour/pose/ID
rows. The accepted geometry passed all **126** original ten-storey walking
waypoints with nine-unit room clearance, without jumping or teleporting.
Actual WASM/WebGL studio images verified the facade. Static acceptance, walking
and images remain separate evidence, not a complete dynamic/calibrated-strength
certificate; this tower exceeds the 5,000-body dynamics working-set limit.

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
