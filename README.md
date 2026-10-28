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
Read-only regions have independent budgets, defaulting to 50,000 pieces and
64 MiB of charged reads, for denser neighborhoods.
Storage operators can [warm an accepted region's discardable cache](docs/loading-and-cache.md#operator-warm-up)
without publishing or changing constructions.
The loading overlay shows the current stage, elapsed time and known-total stage
percentages; discovery reports counts while its final brick total is still unknown.
Packed spatial pages bulk-load block metadata for local traversal. The browser
keeps checksum-verified construction/index packets locally and revalidates access
and versions before reuse; brick specifications and meshes are cached separately.
The navigator's [Render detail control](docs/3d-display-indexing.md#scene-wide-budget-and-quality-controls)
adds scene-wide distance LOD with soft triangle targets and independent shadow
detail; full geometry remains available for editing, picking and collisions.

The server owns authorization, static acceptance and persistence. Dynamic gravity,
earthquake/drop tests, editing feedback and actual image rendering run locally in
WASM. The native MCP process is a thin I/O/stdio bridge.

## Install the MCP client from GitHub

The prebuilt bundle includes native binaries and WASM assets; consumers need
Node.js 20+ and Chrome/Chromium, not Rust.

```sh
npm install --global --ignore-scripts 'https://github.com/ngrislain/ergo/releases/download/v0.5.11/ngrislain-ergo-mcp-0.5.11.tgz'
ergo-mcp --login --publish-url https://ergosandbox.com
```

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

See [agent workflows](docs/agent-shared-editing.md), [MCP tools](docs/local-construction-mcp.md)
and [distribution](docs/distribution.md).

## Native descriptions and assets

[Ergo construction files](docs/construction-format.md) use reusable definitions,
typed part/definition targets and row-major 4×4 transforms in public east/north/up
brick units. They retain occurrence identities, colors, groups and physical metadata.
Imported files are converted at the boundary. Stored assets are
[Ergo part descriptions](docs/part-catalog.md) and geometry with source attribution.

[S3 organization](docs/object-store-layout.md) separates the authoritative world,
part/terrain caches, private profiles/positions/plans and operational backups.
[The format reference](docs/object-store-formats.md) defines each record family.

## Coordinates and physical acceptance

Public axes are X east, Y north, Z up. A storage cell is 1024³ brick units;
the default address mapping is one arc-second per horizontal cell. The Paris
reference is 48.8566° N, 2.3522° E. The reference is fixed; camera GPS is derived
from your current position. Storage-cell membership is not a physical anchor.

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
