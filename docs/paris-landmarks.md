# Paris landmark recipes and validation

`scripts/build/paris_landmarks.py` builds original current-source interpretations
at verified Paris coordinates. Part geometry is never scaled; repeated frame,
storey, window and furniture definitions are referenced rather than copied per
occurrence. The complete native output retains one source mesh per part ID.

## Saint-Sulpice

The reference coordinate is **48°51′04″N, 2°20′05″E** (48.8511111, 2.3347222),
from [Saint-Sulpice](https://en.wikipedia.org/wiki/Saint-Sulpice,_Paris).
The [parish](https://paroissesaintsulpice.paris/) also records its 73-metre real
height. The model is an interpretation, not a dimensionally exact replica.

The published model has **64,727 pieces** and reaches **148⅓ brick heights**.
It uses unequal twin towers, two west-front colonnade orders, a long arcaded nave,
side aisles, chapels, corbelled barrel roof, sanctuary, a golden meridian-like
floor line, organ cases, 30 pews and many genuine `3308a` arches. Stained glass is
made from actual transparent bricks and tiles with dark lead-like cames; these
are not opaque painted window textures. Glass tops leave clearance below the
actual curved arch feet. Two-way joist grillages and rear-room columns support
the tower floors; no unmeasured clutch strength substitutes for missing support.

The current static kernel and actual WASM validation both find a zero-joint-wrench
gravity certificate. The updated authoritative server accepted it, and independent
public readback verifies the exact part/colour/pose multiset, source vertices and
credits. All 21 street-to-nave/side-aisle waypoints pass against the accepted
geometry with the real walker, without jumping or teleporting. This is a static
compression certificate, not a calibrated shake/impact test.

- Assembly: `6a54c03a243cd4330abbf4f277763f27416c608a8c25f75a34a55c84980ce354`.
- Accepted revision: `ab98f1b44dc1b1f806d1970100cad09661e9eaaeeddd8273fcefb18b38342424`.
- [Open Saint-Sulpice](https://ergosandbox.com/app?shared_world=fde184c4-222b-4a16-8a1f-c320da8a2f35&x=-63&y=-20&z=0).
- Geometry-multiset SHA-256:
  `38a54a384a331a6c73afaa23442a35d7ec3fba79dd4dd73072027d7c65976daa`.
- Complete public CBOR packet SHA-256:
  `2179feada4d760caf921a285406ab9251102a4f6c29623aa7143060ab8a41129`.

Anonymous real-browser checks of both public landmark links load all pieces,
finish the complete region (not a partial preview) and render without page errors.
Final actual-WASM nave/stained-glass views were also inspected.

### Local closed-roof revision (not published)

The current builder additionally produces a **75,716-piece** revision at the same
148⅓-brick height. Architectural references include the construction vocabulary of
[21061 Notre-Dame de Paris](https://brickset.com/article/109391/review-21061-notre-dame-de-paris)
and [71043 Hogwarts Castle](https://brickset.com/sets/71043-1/Hogwarts-Castle).
It adds genuine `3037`/`3039` slopes, `3941`/`87081`/`3062b` round detailing and
eight grey minifigure statues. The narthex, nave, aisles, choir and apse have closed
overhead masonry. The pointed vault is bonded/corbelled with genuine plate spans;
this is not a claim of calibrated voussoir-arch lateral thrust mechanics.

Native and actual-WASM static validation pass with a zero-joint-wrench compression
certificate. All 21 walking waypoints pass. The `check_roof` example casts upward
rays against opaque source triangles at **19,668 room stud centres**, with no open
sample; the same plan detects the older published roof's openings. This is sampled
coverage, not a continuous watertightness proof. These local checks do not replace
the accepted model recorded above and do not establish shake/impact survival.

```sh
cargo build --release -p ergo-world --example check_roof
target/release/examples/check_roof target/church-draft/construction.native.json \
  target/church-draft/roof-coverage.plan.json
```

## Technic Eiffel Tower

The reference coordinate is **48.85822222, 2.2945**, from the
[Eiffel Tower](https://en.wikipedia.org/wiki/Eiffel_Tower). The retrieved official
LEGO 21019 reference is [LDraw OMR 81](https://library.ldraw.org/omr/sets/81),
model `ldraw:81:87`, by **Damien Roux [Darats]**, CCAL version 2.0. Its rotated
four-leg/tiered silhouette informed the larger original design; no source model
parts or transforms were scaled.

The published model contains **78,046 pieces**, reaches **567⅓ brick heights**, and
uses genuine pin-holed Technic posts and two-way girder grillages throughout its
open lower legs and upper shaft. Progressively dropping outer post lines creates
the flared/tapered silhouette while exposed inner load-bearing posts remain
vertical and supported. There are no hidden solid pylons or invented pin/joint
capacities. Observation decks are at heights 115 and 237. A ground-level perimeter
route passes the real walker; an ascent route is not claimed.

Both the current kernel and the released v0.5.20 WASM validator find a
zero-joint-wrench gravity certificate. The full-source WebGL overview has also
been inspected.

The tower is **published and independently read back** in the public Sandbox:

- Assembly: `b8b4a8a6637cf382fcec53c75574ff8aa1026e068d3441fb9b2e03f0347cf172`.
- Accepted revision: `95a16e7d4d52ede0c5f2c96ca478c3bc78ca1416957aa5658cdcec9af241c0e2`.
- [Open the Eiffel Tower](https://ergosandbox.com/app?shared_world=fde184c4-222b-4a16-8a1f-c320da8a2f35&x=-208&y=5&z=0).
- The exact 78,046-piece part/colour/pose multiset, source vertices and source
  credits match the checked draft. Multiset SHA-256:
  `46a203b5ea1b28c64e880dae24c1ec71a5f3e87841d8e4a47969e503d599a235`.
- The complete public CBOR packet SHA-256 is
  `850502890475124991acafed7d7fa35e83e3a33819c620ebc2573f8de3b136d4`.
- All five perimeter waypoints pass against the accepted public geometry, without
  jumping or teleporting. This does not establish an ascent route or shake test.

Earlier transport-timeout attempts remain preserved as uncertain journals, not
accepted receipts. Read-only reconciliation established the unchanged original
accepted head and empty site before the explicit original-base recovery. The
successful import used optimized v0.5.21 through a temporary production-backed API
worker with the same authentication, pacing, frozen model, validation and atomic
overlap/CAS predicates. It made no unchecked bucket edits. Never replay the old
attempt directories or clear durable activity pins merely because a client timed out.

Both final publications used the same optimized production-backed API worker;
it was stopped after both receipts were confirmed and public readback completed.
The world retains its original policy, coordinate reference and frozen physical
model CID. The earlier failed public request returned a hosting-proxy read
timeout despite the 1800-second container deadline. Client/container deadlines do
not guarantee that every intermediary or resource limit can sustain a request.
The local worker's observed resident memory exceeded 4 GiB during import; logical
CBOR and cache budgets are not a complete process-heap quota. No specific hosted
out-of-memory cause was established from the unavailable request logs.

## Developer workflow

```sh
cargo xtask build-web
cargo build -p ergo-mcp
cargo build --release -p ergo-world --example validate_native
python3 scripts/build/paris_landmarks.py saint-sulpice --output target/church-draft
python3 scripts/build/paris_landmarks.py eiffel --output target/eiffel-draft
```

Output directories must be fresh. `--prepare-only` emits compact model/proposal
data; `--assets-from` reuses a current native source-asset cache, resolving missing
current parts from the embedded complete library. `--native-validation` runs the
same static kernel through `validate_native` and needs `--library` (a current
part-library JSON); `--render` requests actual WASM
WebGL captures. `scripts/verify-landmark.py` independently checks an existing draft.
The Python construct expander preserves individually posed current limb rotations.

`scripts/build/landmark_revision.py` prepares an identity-preserving local
replacement from an explicit complete accepted workspace and a desired native
draft. It matches unchanged part/colour/pose occurrences one-to-one, retaining
their exact accepted coordinates, local IDs, RGB and steps, including duplicate
occurrences. Matching rounds pose entries to seven decimal places to avoid
serialization noise; retained output keeps the original floats. Changed/new
occurrences receive fresh IDs above the accepted maximum;
unused old occurrences are removed in the proposal. Existing source records,
credits and frozen physical metadata take precedence over the draft's asset cache.
The helper rejects authored joints, anchors and groups requiring an explicit
metadata-aware edit. It does not certify external dependencies or submit a write;
publication still needs a fresh base check and complete server validation.

For the checked closed-roof revision, **48,679** unchanged accepted occurrence IDs
are retained, with 16,048 removals and 27,037 additions. Its input local IDs were
cross-checked against the original accepted receipt, and its exact checkpoint
geometry/source credits against the independently read public packet. This is
local proposal evidence, not a new accepted revision.

```sh
python3 scripts/build/landmark_revision.py --input /private/accepted-church-workspace.json \
  --draft target/church-draft/construction.native.json --output target/church-revision
```

The separate `scripts/build/eiffel_truss.py` generates an **articulated Technic
tower**, not a replacement of the accepted tower. It uses genuine long Technic
beams at free angles, cross axles in round bearings and retaining bushes. Its four
inclined legs lie in two orthogonal planes, with X-braces and horizontal ties at
the feet, two floor levels and the top. Pillar angles increase from approximately
64.62° to 75.52° and 83.41°. Parts retain proper metric rotations and their
unscaled source geometry. Two spaced axles join overlapping beam splices; every
round bearing is a free revolute joint, so the braced panels, not friction, carry
the loads.

- **Central bearing bricks** (3700) hang on the cross-tie axles. Their centre of
  mass lies just below the hole, so they rest without a stud connection.
- **Feet:** each foot axle carries bearing bricks on every free layer, resting on
  one foundation block of 2×2 bricks and plates. Loads entering the axle along
  its whole length then reach the ground directly.

The 4,096-part tower is overlap-free and publication-ready under the general
equilibrium certificate with the reviewed connector estimates. Its most loaded
joint uses 46% of its capacity, with 4,680 inferred joints. The two planes are
joined only through the ground; it is a skeletal truss, not a detailed replica.
It is not yet published: the production server must first run the equilibrium
certificate (release 0.5.32 or later). `inspect_mechanism` reports its joint
components.

```sh
cargo build --release -p ergo-world --example validate_native --example inspect_mechanism
python3 scripts/build/eiffel_truss.py --output target/articulated-tower-draft \
  --assets-from /absolute/path/to/current-source-construction.native.json
target/release/examples/validate_native --library part-library.json \
  target/articulated-tower-draft/construction.native.json
target/release/examples/inspect_mechanism target/articulated-tower-draft/construction.native.json
python3 scripts/publish-landmark.py --construction target/articulated-tower-draft/construction.native.json \
  --token-file /path/to/token --output target/articulated-tower-publication --publish
```

`validate_native` reads a native construction, a materialized export or a shared
workspace response. It needs the current part library, the one the server installs:
`/api/library/<revision>/library.json`, where `/api/config` gives the revision as
`part_library`. `scripts/publish-landmark.py` downloads it from its `--url` before
validating.

`scripts/publish-landmark.py` performs a fresh authenticated preflight and writes
a durable MCP sidecar/journal. Only its explicit `--publish` submits a mutation.
The normal server still owns permission, pacing, complete-domain validation and
CAS. `--expected-base` fences a manually reconciled recovery to its original
revision; concurrent identical imports retain their atomic overlap predicates
and cannot create duplicate geometry. A timeout is not a rejection or receipt.
