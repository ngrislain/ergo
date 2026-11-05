# Ergo

A shared, geographically addressed world built from blocks. The browser,
homepage illustration and MCP vision tools use one Rust/WASM WebGL2 engine.
Accepted edits live in S3 with authenticated author attribution.

- **Sandbox:** https://ergosandbox.com/
- **Project guide:** https://www.ergosandbox.com/
- **Agent setup:** https://ergosandbox.com/login#connect
- **Current implementation:** [docs/README.md](docs/README.md)
- **Unimplemented work:** [TODO.md](TODO.md)

## One public world

Anyone can view Ergo Sandbox. Signed-in users can create and edit constructions
subject to permissions, their brick-change allowance and server validation.
The scrolling construction list navigates to models at their actual shared-world
positions. Live camera GPS, height and storage-cell coordinates update as you move.
Signed-in camera positions are saved privately for agent placement.
Construction rows show author, recorded creation time and size, with sorting of
loaded rows. Complete selected constructions download as standard self-contained
LDraw/MPD or native Ergo JSON.
Accepted region reads use [compact compressed packets and revision-safe caches](docs/loading-and-cache.md);
selecting fully resident constructions reuses their loaded geometry.
Loading follows physical viewing distance with a 384-stud prefetch margin;
sunlight caster coverage is independent of camera zoom, colour LOD and clipping.
Read-only regions have independent budgets, defaulting to 50,000 pieces and
64 MiB of charged reads, for denser neighborhoods.
Storage operators can [warm an accepted region's discardable cache](docs/loading-and-cache.md#operator-warm-up)
without publishing or changing constructions.
The loading overlay shows the current stage, elapsed time and known-total stage
percentages; discovery reports counts while its final brick total is still unknown.
Packed spatial pages bulk-load block metadata for local traversal. The browser
keeps checksum-verified construction/index and discovery data in an IndexedDB LRU,
deduplicates identical content, protects active data and revalidates access/version
before reuse. Cache writes do not delay display; stale or stalled navigation can
be replaced by another construction selection.
Large region reads have a 30-minute client deadline and display revision-pinned,
read-only partial previews while materializing. The complete current basic library
(138 sourced parts and five common constructs) is proactively cached locally;
obsolete pre-migration construct versions are rejected. Native saves/exports share
identical reusable definitions while retaining every placed occurrence.
The basic OSM ground style retains natural features, roads, place names and main
landmarks while filtering commercial points of interest before rasterization.
Current-source [Paris landmark recipes](docs/paris-landmarks.md) prepare detailed
Saint-Sulpice and an extremely tall Technic Eiffel Tower at their real coordinates.
Both are published with exact public geometry/source-credit readback and checked
walking routes, recorded separately from static and visual evidence.
The navigator's [Render detail control](docs/3d-display-indexing.md#scene-wide-budget-and-quality-controls)
adds scene-wide distance LOD with soft triangle targets and independent shadow
detail; full geometry remains available for editing, picking and collisions.
Large static regions cache merged exterior surfaces, removing internal faces and
combining coplanar same-color quads. Idle navigator views stop resubmitting draws.

The server owns authorization, static acceptance and persistence. Dynamic gravity,
earthquake/drop tests, editing feedback and actual image rendering run locally in
WASM. The native MCP process is a thin I/O/stdio bridge.

<a id="install-the-mcp-client-from-github"></a>

## Install the MCP client

The prebuilt bundle includes native binaries and WASM assets; consumers need
Node.js 20+ and Chrome/Chromium, not Rust.

```sh
npm install --global --ignore-scripts @ngrislain/ergo-mcp@0.5.22
ergo-mcp --login --publish-url https://ergosandbox.com
```

The [GitHub Release](https://github.com/ngrislain/ergo/releases/tag/v0.5.22)
also provides the verified complete tarball and checksum.

Generate/download a token in the signed-in browser, keep it in a private file,
and configure the client:

```sh
ERGO_PUBLISH_URL=https://ergosandbox.com \
ERGO_PUBLISH_TOKEN_FILE=/absolute/private/path/ergo-mcp-token \
ergo-mcp --workspace /absolute/path/sandbox.cache.json --headless
```

Use `get_user_position` for the user's saved location and its ground-level
`model_transform`. `place_submodel` accepts native Ergo definitions; pass that
transform when creating a standing model there. To edit an existing construction,
use `open_shared_construction` with its ID or a specified position. Accepted
mutations update the same shared assembly automatically; the working file is a
cache. Read the confirmed `shared_commit` receipt.

Call `get_building_techniques` to propose sourced wall, junction, opening, floor
and roof patterns before building. Placements snap complete models coherently to
the stud/plate grid. The server independently checks ground registration and
proper metric isometries (translation + rotation, no scale/shear/reflection).
Before a new design, [search/retrieve similar models](docs/model-library.md) with
`get_lego_models`, reuse or adapt useful official/custom references, and retain
their source/author/licence attribution.

See [agent workflows](docs/agent-shared-editing.md), [MCP tools](docs/local-construction-mcp.md)
and [distribution](docs/distribution.md).

## Native descriptions and assets

[Ergo construction files](docs/construction-format.md) use reusable definitions,
typed part/definition/common-construct targets and row-major 4×4 transforms in public east/north/up
brick units. They retain occurrence identities, colors, groups and physical metadata.
Imported files are converted at the boundary. Stored assets are
[Ergo part descriptions](docs/part-catalog.md) and geometry with source attribution.
One [pinned canonical library](docs/part-library.md), derived from official LDraw
and BrickLink, supplies 138 parts, reviewed common-part usage counts, hand/wheel/Technic
interfaces and plate-free minifigure/car references,
including [Lion Knights guards and supported genuine Castle pennants](docs/part-library.md#pre-assembled-common-constructs).
Its compact native pack resolves the complete selected geometry offline.

[S3 organization](docs/object-store-layout.md) separates the authoritative world,
part/terrain caches, private profiles/positions/plans and operational backups.
[The format reference](docs/object-store-formats.md) defines each record family.

## Coordinates and physical acceptance

Public axes are X east, Y north, Z up. A storage cell is 128 studs × 128 studs ×
128 bricks high (384 plates); [old indexes are migrated atomically](docs/cells-and-migration.md).
the default address mapping is one arc-second per horizontal cell. The Paris
reference is 48.8566° N, 2.3522° E. The reference is fixed; camera GPS is derived
from your current position. Storage-cell membership is not a physical anchor.
Virtual baseplates use [OSM-derived per-stud colours](docs/ground-and-coordinates.md),
restricted to exact standard LEGO colours per stud, with rounded categorical
region borders and no RGB gradients. Generated colour
fields are small display caches, independent of authored bricks and physics.

Nominal geometry uses 8 mm stud pitch, 9.6 mm brick height and 3.2 mm plate height.
The simulator uses sourced masses and SI gravity with box collision/inertia
approximations. Loose blocks need a compressive static support certificate;
certificates relying on physical joints need server-owned measured capacities.
A contact-only gravity proof may instead use zero joint wrench, without assuming
any clutch strength. Authoritative acceptance does not invent measured forces;
explicit local ballpark diagnostics report their estimated assumptions separately.
Dynamic simulations are bounded to 5,000 pieces; the world has no aggregate
piece-count limit. God-mode accounts have no authoring brick-count cap; physical
acceptance and byte/request budgets still apply.

## Developer setup

From the repository root, with the pinned Rust toolchain:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked
cargo xtask build-web
cargo run --release -p ergo-server
```

Open http://127.0.0.1:3000. The server creates one local `data/world` sandbox
and an explicit development account. Configure storage/authentication using
[.env.example](.env.example). The deployed server uses Scaleway S3 and Google
OpenID Connect; credentials stay on the server.

```sh
cargo test --workspace --locked
cargo fmt --all -- --check
python3 scripts/check-docs.py
```

The [operations runbook](docs/operations.md) covers browser/MCP checks, GitHub
releases, the public website, deployment and private configuration recovery.
For a complete construction cache and explicit route, `cargo run -p ergo-client
--example check_walk -- construction.ergo.json route.json` checks continuous
walking, floor arrivals and headroom with the viewer's actual movement engine.
The [connector requirements helper](docs/physical-calibration.md#inventory-the-measurements-a-construction-requires)
inventories the exact calibration keys a construction needs without supplying
invented strength values.

Original [walkable monument recipes](docs/model-recipes.md) are in
`scripts/build/`, including a corridor pyramid and a ten-level Seven Sisters-inspired tower.
The [terraced park recipe](docs/model-recipes.md#terraced-park-recipe) adds a sourced,
explicitly published landscape workflow with ponds, trees and summit walking checks.
The [castle mountain recipe](docs/model-recipes.md#castle-mountain-recipe)
prepares a moat, walkable arched bridge, genuine arrowslit panels and plate-free
standing figures with capes, helmets and horses. Held weapons and archer poses
remain pending measured connector capacities; local validation and shared
publication are recorded separately.
Local [flush parking and two-colour lance proposals](docs/model-recipes.md#local-flush-parking-and-lance-standard-proposals)
and [closed-roof/articulated Paris drafts](docs/paris-landmarks.md) preserve
unrelated accepted occurrences and record validation separately from publication.
The server's unlisted `/catalogue` reference shows all canonical base pieces, genuine
geometry previews, reviewed interfaces, mass sources and a LEGO/LDraw preview palette.
