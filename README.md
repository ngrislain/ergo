# Ergo
An Ergo Land

A Rust web workbench for geographically addressed brick constructions, with a
Leptos UI, WebAssembly/WebGL2 rendering, object-store persistence, and a local
MCP construction editor.

The public guide is at **https://www.ergosandbox.com**: authenticate a human,
connect an agent, and start building. Its brick scene uses the navigator's
shared WebGL/WASM engine, with fixed studio lighting and orbit/zoom controls.
The navigator uses geographic sun/shadows and a night flashlight.
The guide also provides a [privacy notice](https://www.ergosandbox.com/privacy/)
and [terms](https://www.ergosandbox.com/terms/). See [website deployment](docs/static-site.md)
for its GitHub Pages source and publishing workflow.
The apex shows sign-in to guests and opens the viewer for signed-in users;
`/login#connect` remains the agent-token setup page.
The [operations runbook](docs/operations.md) lists the shell commands for tests,
publication, DNS/TLS, server deployment, and configuration recovery.
For a complete npm-to-production demonstration, see [MCP building examples](docs/mcp-production-examples.md).

## Run locally

Install Rust, the WebAssembly target, and the `wasm-bindgen` CLI matching
`Cargo.lock`:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked
cargo xtask build-web
cargo run --release -p ergo-server
```

Open **http://127.0.0.1:3000**. Run commands from the repository root. The server
creates a local `data/` object store, a labeled development user, and read-only
fixture examples. Configure `.env` using [`.env.example`](.env.example) for other
storage/authentication settings. Neither a JavaScript package manager nor a
database is needed.

Local development also accepts **http://localhost:3000**. If an import or save
reports **Unexpected request origin**, open the configured `ERGO_PUBLIC_URL` or
set it to the browser-facing origin and restart the server. Google-enabled and
non-loopback deployments require that origin to match.

`cargo xtask build-web` runs the Rust build orchestrator in `crates/xtask`.
It compiles both WASM entry points separately and generates their bindings with
`wasm-bindgen`, honoring Cargo's configured target directory. The task runs
explicitly, so ordinary server builds/tests do not require the WASM toolchain.
`scripts/build-web.sh` remains a forwarding compatibility wrapper.

## What works

- Instanced 3D brick rendering, full-screen orbit/fly/walk navigation, nearest-construction navigation, mesh picking, part
  inspection, layer separation, and a base grid.
  Fly uses arrows to steer and A/Z to accelerate/slow. Walk uses up/down to move,
  left/right to turn, A/Z for speed, and Space to jump; focus the canvas first.
  Walking steps up 3 vertical units; unobstructed jumps peak 12 units above
  takeoff, with metric gravity, body collisions, ceiling hits, and landing.
- Frustum/fog-bounded streaming, progressive distance LOD, a resident octree, and bounded persistent
  browser caching of immutable block/geometry objects.
- Lazily generated studded 1024×1024 special baseplates below Z=0, each using one
  standard color nearest the average satellite/aerial image color for that footprint.
- Cache-first supported LDraw parts: missing meshes/dependencies are pulled and
  persisted in local storage or S3 with source/license credits.
- BrickLink Studio `.io`, LDraw `.ldr`/`.mpd`, and Ergo JSON import into a working
  copy. The 27 basic bricks/plates/tiles and 20 opaque colors are preferred agent
  recommendations; other Studio/LDraw parts resolve on demand, with a full sourced
  LDraw color palette and explicit measured-physics requirements for publication.
- Fixed and revolute constraints, collision-aware group nudges, dynamic pulling,
  gravity simulation, and non-destructive earthquake/falling-object load tests.
- Persistent nested authoring groups with rigid transforms, validated membership,
  pivots, and collision/constraint-aware destination edits.
- [LDraw-aligned construction files](docs/ldraw-storage.md) with reusable nested
  subparts, 4×4 homogeneous LDU transforms and legacy JSON compatibility;
  MCP `place_submodel` instantiates LDraw/MPD assemblies atomically.
- Server-side save validation: schema/catalog checks, overlap checks, grounding,
  static support/wrench checks with numerical tolerances, and measured connector-capacity requirements. The server runs no dynamic simulation.
- Google OpenID Connect, object-store user profiles, signed HttpOnly sessions,
  a landing/setup page with MCP/API tokens, per-user snapshots, and fixture examples.
- Collaborative shared worlds with configurable per-user pacing (default 5,000
  changed bricks refilled per day); paid entitlements bypass pacing, not validation.
- An authenticated shared-world REST API with versioned region grants, optimistic
  commits, stable content-derived piece identities, paged spatial access, and
  on-demand validation of complete affected mechanical domains, and bounded shared-world navigation.
- A thin stdio/REST MCP bridge and minimal local WASM client with editing,
  stability feedback, actual WebGL2 PNG image tools for vision models, and
  authenticated shared-world publication with accepted-revision read-back.

## Coordinates and physical data

The Paris default origin is **48.8566° N, 2.3522° E**. Public axes are X east,
Y north, Z height. **One 1024³ storage block maps to one arc-second east/north**, not one
arc-second per individual brick. This convention is configurable.
Piece bases rest at Z=0; vertical storage boundaries do not create additional floors.

Physics is separate: nominal dimensions use 8 mm stud pitch, 9.6 mm brick height,
and 3.2 mm plate height; all starter parts have sourced BrickLink mass records.
The solver uses SI units and standard gravity. Detailed visual meshes coexist
with simplified body-box collisions and box inertia.

**Connection strength is not guessed.** Publishing ordinary snapshots containing
joints requires a server-owned measurement book via
`ERGO_CONNECTION_MEASUREMENTS`. Without it, local drafts and idealized fixture
examples can be explored, but a real-world connection-strength claim is not made.
See [physical calibration](docs/physical-calibration.md).

The interactive workspace is currently limited to 5,000 bricks/128 blocks;
streamed imports can contain up to 200,000 parts. Render paging, a general sparse
equilibrium search, and calibrated directional fracture remain future work.
The client gravity probe defaults to one step with settable step count and
numeric tolerances; the server uses a zero-step static certificate.

## Scaleway S3 and Google sign-in

The hosted server is at **https://ergosandbox.com**; open
**https://ergosandbox.com/app** for the workbench. Its deployment uses
the [OpenTofu configuration](infra/scaleway/) and
[Scaleway deployment guide](docs/scaleway-deployment.md). It uses the existing
`ergo` bucket, a single API/web container, apex DNS, and managed HTTPS.
Container builds and runtime smoke tests use Podman; Dockerfile compatibility
does not require Docker Desktop.

Set `S3_BUCKET`, `S3_ENDPOINT`, `S3_REGION`, `AWS_ACCESS_KEY_ID`, and
`AWS_SECRET_ACCESS_KEY`. The default endpoint is `https://s3.fr-par.scw.cloud`
and the default region is `fr-par`. Geometry/blocks are immutable CBOR objects;
manifests and user profiles are JSON. Credentials stay on the server.
Shared-world heads, packed indexes, policies, and geometry use a separate CBOR
namespace. Local development uses a filesystem-lock CAS adapter.

For Google authentication, configure `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`,
`ERGO_SESSION_SECRET` (at least 32 random characters), and `ERGO_PUBLIC_URL`.
Register `<ERGO_PUBLIC_URL>/auth/google/callback` as the authorized Google redirect
URI. Outside localhost, HTTPS is required. With Google unset, the app uses the
explicit local development account.

An optional `LDRAW_DIR` supplies local LDraw sources. Otherwise missing supported
parts are fetched from the official library and cached; `ERGO_ASSET_FETCH=0`
disables network fetching and exposes a labeled parametric fallback for standard
parts; non-standard parts require their actual geometry. Default ground imagery
uses Esri World Imagery satellite/aerial tiles; an alternative XYZ provider can
be configured. See [ground and coordinates](docs/ground-and-coordinates.md),
[identity and ownership](docs/identity-and-ownership.md)
and [persistence indexing](docs/persistence-indexing.md).

The root is the sign-in/setup landing page; `/app` is the workbench. See
[account/MCP/API onboarding and edit pacing](docs/access-and-onboarding.md) and
[Studio catalog support](docs/studio-catalog.md). Paid-plan entitlement is currently
operator-managed; checkout/payment-webhook integration is future work.

## Local MCP editor

Release automation targets the API image `ghcr.io/ngrislain/ergo` and the prebuilt
`@ngrislain/ergo-mcp` npm package. Check the corresponding release workflow and
[distribution/setup status](docs/distribution.md) before using registry URLs;
tagging the source does not by itself confirm either registry publication.
For server deployments, the workflow also supports an explicit
[image-only CI publication](docs/scaleway-deployment.md#build-and-publish-in-github-actions)
using GitHub's built-in registry credentials.
Install the prebuilt npm package from its GitHub Release:

```sh
export ERGO_MCP_PACKAGE='https://github.com/ngrislain/ergo/releases/download/v0.3.1/ngrislain-ergo-mcp-0.3.1.tgz'
npm install --global "$ERGO_MCP_PACKAGE"
ergo-mcp --workspace /path/construction.ergo.json
```

It includes native/WASM assets; consumers do not need Rust. After registry
publication, `ERGO_MCP_PACKAGE` can select `@ngrislain/ergo-mcp@VERSION`.
The current-source npm launcher's
`--login` and authenticated publishing default to `https://ergosandbox.com`;
override with `ERGO_PUBLISH_URL` or `--publish-url`. Local editing needs no token.
Source builds remain:

```sh
cargo build --release -p ergo-mcp
./target/release/ergo-mcp --workspace /absolute/path/construction.ergo.json
```

Build both WASM entry points with `cargo xtask build-web` first. The default
MCP command opens a minimal client in the existing browser; `--attach` prints its
local attachment URL and `--headless` uses isolated Chrome for automation. The
bridge does not load the full application UI or run a native renderer/solver.

The parent directory must exist. Edits are stored in this local working copy.
`publish_construction` can publish a new shared-world assembly through the
authenticated REST service when `ERGO_PUBLISH_URL` and a user delegation token
are configured. The bridge confirms the accepted revision by remote read-back;
S3 credentials stay on the server. See [publication setup](docs/local-construction-mcp.md#remote-publication-setup).
`view_construction` returns images from
isometric, north, south, east, west, or top views. `test_solidity` returns
earthquake/drop-test diagnostics and an optional post-test image.
`test_moderate_earthquake_stability` supplies a named, two-axis 0.15g-bound preset
with calibrated-stability feedback and optional post-test images.
The primary testing tools are `test_stability_by_gravity`,
`test_visual_appearance` (including arbitrary camera poses), and
`test_stability_by_earthquake` with configurable local PGA intensity in g.
`create_group`, `set_group_members`, `transform_group`, `list_groups`, and
`ungroup` manage nested authoring assemblies.

See [MCP tools and configuration](docs/local-construction-mcp.md).

## Documentation and next features

See the [release notes](CHANGELOG.md) for the current version's changes and limits.

### Concurrent CBOR/S3 transaction library

[`ergo-transactions`](crates/transactions/) provides immutable geometry-block
octrees, atomic multi-block commits through a conditional S3 root update,
read/predicate dependency validation, automatic rebase of disjoint subtree edits,
and materialized historical snapshots. Separate shard roots provide independent
publication domains. Ancestor/descendant edits conflict; cross-branch geometry
dependencies can be registered with transactional reads or `watch`.
The follow-ups include atomic accepted-revision indexes, adaptive dense spatial
pages, paged rules, cross-assembly joints, scoped discovery catalogs, revision SSE,
opt-in group publication, and explicit owner-configured retention.

```sh
cargo test -p ergo-transactions
# With exported S3_BUCKET/AWS credentials; defaults to Scaleway fr-par:
cargo run -p ergo-transactions --example s3_edits
```

See [transactional storage](docs/transactional-storage.md) for the API, independence
rules, S3 endpoint checks, and scaling limits. It also backs the permission-checked
shared-world REST API; private snapshot saves retain their own workflow. See
[shared-world editing](docs/concurrent-global-editing.md).
Use [storage operations](docs/storage-operations.md) for paging APIs, legacy
upgrade/backfill commands, retention policy, and deployment flags.

Start with the [technical documentation index](docs/README.md). It covers display
indexing, geographic persistence, rigid-body physics, constrained positioning,
physical calibration, identity, and concurrent editing, with equations and SVG
figures.

Shared worlds have **no aggregate size or piece-count cap**. The server loads
spatial pages and follows identity/constraint references on demand. Limits apply
to each import/edit working set; oversized dependency closures are rejected
explicitly. Piece IDs hash immutable creation records; edits change their
content-addressed versions without changing identity. Coordination is optimistic;
region leases are an optional future extension. See
[concurrent global editing](docs/concurrent-global-editing.md) and
[`TODO.md`](TODO.md) for the remaining implementation backlog. Documentation must stay
synchronized with code as required by [`AGENTS.md`](AGENTS.md).

## Checks

```sh
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo check -p ergo-viewer --target wasm32-unknown-unknown
cargo xtask build-web
python3 scripts/check-docs.py
python3 scripts/check-execution-boundaries.py
```

Live Google OAuth and Scaleway behavior require configured external resources;
native tests exercise local storage, asset caching, ownership/session logic, and
the shared physics/checking model. MCP rendering/simulation tests use the local
WASM browser runtime.

End-to-end MCP checks use the built native binary:

```sh
cargo build -p ergo-mcp -p ergo-server
cargo xtask build-web
python3 scripts/test-mcp.py
python3 scripts/test-mcp-publish.py
```

Run the complete isolated browser flow with the configured Playwright Python environment:

```sh
python3 scripts/test-web-isolated.py
```

Verify a built Scaleway image with Podman:

```sh
python3 scripts/test-container.py --image localhost/ergo:v0.3.0
```

The publication check starts an isolated server and exercises signed delegation,
MCP/WASM construction, new/existing-world imports, persistent remote read-back,
and authorization/physics rejection. It uses a disposable filesystem object store;
live Google and Scaleway integration still require configured external resources.

Browser smoke tests require Python Playwright (`scripts/requirements-test.txt`),
Google Chrome, the built web client, and an isolated running development server:

```sh
python3 -m pip install -r scripts/requirements-test.txt
python3 scripts/test-web.py --url http://127.0.0.1:3000
```

The browser test imports and saves test constructions, so point it at a disposable
local object store with `ERGO_GROUND_SOURCE=disabled` and `ERGO_ASSET_FETCH=0`.
This avoids automated requests to public map services. Fixture-based native tests
cover imagery averaging, palette projection, and persistent ground/part caches.
