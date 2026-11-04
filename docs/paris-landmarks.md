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
same static kernel through `validate_native`; `--render` requests actual WASM
WebGL captures. `scripts/verify-landmark.py` independently checks an existing draft.
The Python construct expander preserves individually posed current limb rotations.

`scripts/publish-landmark.py` performs a fresh authenticated preflight and writes
a durable MCP sidecar/journal. Only its explicit `--publish` submits a mutation.
The normal server still owns permission, pacing, complete-domain validation and
CAS. `--expected-base` fences a manually reconciled recovery to its original
revision; concurrent identical imports retain their atomic overlap predicates
and cannot create duplicate geometry. A timeout is not a rejection or receipt.
