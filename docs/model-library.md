# Model references for MCP building

`get_lego_models` searches for relevant official or configured custom LEGO models
and retrieves a reusable LDraw reference without changing the current construction.
MCP initialization and `get_catalog` building guidance explicitly require this
lookup **before designing a new model**. Existing-model repairs do not require a
replacement reference for every individual brick operation.

## Search, inspect, then reuse or adapt

```json
{"query":"6505","source":"ldraw","limit":5}
```

The official LDraw OMR result includes `model_id`, set number, name, theme, year,
source page and thumbnail URL. Use broader subject/style queries such as `car`,
`house` or `bridge`, then inspect the closest useful result. If a first query has
no suitable result, try related terminology instead of inventing a model match.

Retrieve by ID, optionally selecting a reusable MPD section:

```json
{"model_id":"ldraw:610:671","submodel":"6505 - car.ldr"}
```

This example is LEGO **6505 Fire Chief's Car**, from
[the official OMR entry](https://library.ldraw.org/omr/sets/610), modeled by
**Robert Paciorek [bercik]**. Retrieval returns the exact selected source document,
SHA-256, author/license, submodel names and expanded part inventory. Selecting a
submodel retains only its reachable embedded files, so unrelated minifigures or
alternate models are not accidentally placed. File comments/attribution remain
in the source. Multiple official/alternate files are returned as variants when a
set ID alone is ambiguous.

Use `document` with `place_submodel`, or adapt its demonstrated geometry, part
choices and construction patterns. Retain source URL, author and license in the
derivative's attribution. For example, a district car can adapt the official
Town model's genuine chassis, mudguard, windscreen and roll bar rather than
substituting an arbitrary stack of bricks.

This is **reference retrieval, not a physical certificate**. The agent must still
resolve genuine part geometry and sourced masses, preserve proper metric
isometries, adapt to the requested scale, and obtain authoritative acceptance for
an actual shared edit. Rounded/affine transforms in an imported reference remain
subject to placement checks; inventories alone contain no assembly poses. If no
useful model exists, report the searches and build an original sourced design.

## Providers and limits

- **LDraw OMR:** searches its public model table using its actual guest-session
  search protocol, not an unfiltered query-string page. Model downloads are
  restricted to the official library's public model-download paths. No account
  login is required.
- **Rebrickable:** optional set search/details through its API. Configure
  `ERGO_REBRICKABLE_API_KEY` in the MCP process environment. The key is sent in an
  HTTP authorization header, never a URL/tool result. Results are explicitly
  `catalogue_reference` metadata/images, not invented spatial model geometry.
- **BrickLink:** public catalogue search links are returned for inspiration and
  part/set lookup. The tool does not claim to obtain model poses from a catalogue
  inventory or download paid instructions.
- **Custom LDraw:** set `ERGO_MODEL_LIBRARY_DIR` to a local directory with an
  `index.json` manifest and `.ldr`/`.mpd` files. Entries declare `id`, `name`,
  relative `path`, `author`, `license` and optional `tags`. Search matches names
  and tags; retrieve with `model_id:"custom:ID"`. Paths/symlinks must stay inside
  the configured directory.

```json
[
  {"id":"town-car","name":"Custom Town car","path":"town-car.mpd",
   "author":"Example Builder","license":"CC BY 4.0","tags":["car","vehicle"]}
]
```

Queries are at most 200 bytes; result limits are 1–10; documents are at most
8 MiB. Inline documents are capped at the placement tool's 128 KiB document bound; larger references are written to
the private `ERGO_MODEL_CACHE_DIR`, defaulting to the user's Ergo cache directory,
and returned by path so the agent can choose a smaller submodel or inspect them.
Public HTTP requests have a 20-second deadline and bounded redirects. Model-library
I/O/parsing executes in the native bridge; construction edits, simulation and
actual Ergo image capture remain in WASM.

Reconnect/restart the MCP client after installing a bundle with the new tool.
The server's building instructions and tool schema are delivered at initialization.

## Verification

```sh
cargo test -p ergo-mcp models
python3 scripts/test-model-library.py
# Actual public OMR search/download through stdio MCP, opt-in network:
python3 scripts/test-model-library.py --network
```

The checks cover the published tool schema, custom lookup, source attribution,
exact document retrieval/submodel selection and unchanged working geometry.
Implementation: `crates/mcp/src/models.rs`, `crates/mcp/src/main.rs` and the shared
schemas/building guidance in `crates/world/src/mcp.rs`.
