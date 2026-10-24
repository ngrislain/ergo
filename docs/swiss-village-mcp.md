# Swiss-style village built with MCP

`scripts/build-swiss-village.py` builds eight chalets around a small village
square through **real Ergo MCP stdio calls**, then requests actual shared-engine
WebGL images. It does not write a construction directly to S3 or imply a live
publication from a successful local build.

## Generate it

Use the current developer build to exercise the current unified renderer:

```sh
cargo xtask build-web
cargo build --locked -p ergo-mcp
ERGO_ASSET_FETCH=0 python3 scripts/build-swiss-village.py \
  --binary target/debug/ergo-mcp --output target/swiss-village-mcp
```

Chrome/Chromium is required for headless MCP execution. Choose a fresh output
directory; existing working copies are not overwritten. `ERGO_ASSET_FETCH=0`
uses the standard parts' nominal parametric meshes without upstream requests.
Omit it to allow normal cached/local/upstream LDraw resolution. `--model-only`
writes reproducible MPD source without starting MCP, but that mode is not an MCP
execution or validation receipt.

Outputs in the ignored `target/` directory:

- `swiss-village.mpd`: reusable chalet/pine subfiles and positioned village root.
- `swiss-village.ergo.json`: actual MCP working copy, in canonical construction format.
- `swiss-village-aerial.png`, `swiss-village-square.png`,
  `swiss-village-chalets.png`: generated views, never checked into Git.
- `report.json`: placement/validation feedback, complete bill of materials and
  the MCP tools used. Read the reported publication state, not just image existence.

## Design

Three reusable chalet designs use pale plaster/wood upper storeys, red/slate/brown
stepped gable roofs, shutters, timber balconies, flower boxes and chimneys. Four
homes face the square from each side. Twelve pines, flower beds, benches, lamps
and a fountain frame paved lanes and a central plaza.

Bricks/plates retain their nominal physical dimensions; windows use rigidly
rotated plates. Largest suitable standard parts pack repeated runs. A bearing
roof core, supported shutter timbers, and grounded balcony posts avoid merely
adjacent/floating roof and facade pieces. This is a block-built architectural
interpretation, not a claim of authentic Swiss structural engineering.

The script calls `place_submodel`, `validate_construction`, paged `list_bricks`,
and `test_visual_appearance` for aerial, square and chalet cameras. All construction
and images execute in the local WASM client through the MCP bridge.

## Recorded result

The final 2026-10-24 run at `target/swiss-village-mcp-20261024-v9/` contains
**3,162 bricks, eight homes, three reusable chalet designs and 12 pines**.
Overlap checks and the static certificate under idealized joints pass.
**It is not committed to the public Sandbox.** Calibrated admission is blocked
by missing connector measurements, first reported for `41539.dat ↔ 41539.dat`,
fixed, 8×8-stud overlap. No capacity was guessed and no publication exemption was
used. See [physical calibration](physical-calibration.md).
