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

The local checked draft has **64,727 pieces** and reaches **148⅓ brick heights**.
It uses unequal twin towers, two west-front colonnade orders, a long arcaded nave,
side aisles, chapels, corbelled barrel roof, sanctuary, a golden meridian-like
floor line, organ cases, 30 pews and many genuine `3308a` arches. Stained glass is
made from actual transparent bricks and tiles with dark lead-like cames; these
are not opaque painted window textures. Glass tops leave clearance below the
actual curved arch feet. Two-way joist grillages and rear-room columns support
the tower floors; no unmeasured clutch strength substitutes for missing support.

The current static kernel and actual WASM validation both find a zero-joint-wrench
gravity certificate. The street-to-nave/side-aisle route uses the real walking
engine without jumping or teleporting. These local checks are **not a publication
receipt**. Publication through the updated server and exact accepted readback
remain separate steps.

## Technic Eiffel Tower

The reference coordinate is **48.85822222, 2.2945**, from the
[Eiffel Tower](https://en.wikipedia.org/wiki/Eiffel_Tower). The retrieved official
LEGO 21019 reference is [LDraw OMR 81](https://library.ldraw.org/omr/sets/81),
model `ldraw:81:87`, by **Damien Roux [Darats]**, CCAL version 2.0. Its rotated
four-leg/tiered silhouette informed the larger original design; no source model
parts or transforms were scaled.

The checked draft contains **78,046 pieces**, reaches **567⅓ brick heights**, and
uses genuine pin-holed Technic posts and two-way girder grillages throughout its
open lower legs and upper shaft. Progressively dropping outer post lines creates
the flared/tapered silhouette while exposed inner load-bearing posts remain
vertical and supported. There are no hidden solid pylons or invented pin/joint
capacities. Observation decks are at heights 115 and 237. A ground-level perimeter
route passes the real walker; an ascent route is not claimed.

Both the current kernel and the released v0.5.20 WASM validator find a
zero-joint-wrench gravity certificate. The full-source WebGL overview has also
been inspected. The authenticated import is currently **unconfirmed** after
transport timeouts; durable journals and accepted-history/candidate reconciliation
are required before declaring it live. Never replay those attempt directories.

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
