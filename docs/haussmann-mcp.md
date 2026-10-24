# Test a Haussmann-style building with MCP

This exercise builds a **proposal**, inspects real WASM/WebGL images, and records
both static and gravity-test results. It does not create an alternative hosted
world or publish an uncertified model. Normal accepted production edits still go
to the one public [Ergo Sandbox](https://ergosandbox.com/).

## Run it

Install the v0.4.0 prebuilt bundle with npm, or use the current developer binary:

```sh
npm install --global 'https://github.com/ngrislain/ergo/releases/download/v0.4.0/ngrislain-ergo-mcp-0.4.0.tgz'
python3 scripts/build-haussmann.py --output target/haussmann-test

# Developer equivalent after cargo build -p ergo-mcp and cargo xtask build-web:
python3 scripts/build-haussmann.py --binary target/debug/ergo-mcp \
  --output target/haussmann-test
```

Use a fresh output directory. Chrome/Chromium is needed for headless execution;
set `ERGO_CHROME_BIN` if necessary. The script uses only Python's standard library
and the real MCP stdio transport. It performs an isolated non-destructive test,
with no production URL/token passed to the bridge.

Outputs: `haussmann.mpd`, `haussmann.ergo.json`, front/street PNGs, and
`report.json` containing the part counts and actual validation/gravity reports.

## Design and MCP sequence

The design has six pale-stone storeys with five repeated window bays, horizontal
cornices, continuous dark balconies at levels two and five, and a stepped dark
mansard with five dormers. It is a LEGO interpretation, not an architectural or
real-world structural certification.

The builder packs long wall runs with 2×8, 2×6, 2×4 and 2×2 bricks, uses large
8×8 floor plates, and reuses MPD storey/balcony/dormer definitions. An internal
bearing spine supports interior panels. Large plates span supports instead of
leaving small isolated floor panels floating. This reduced the supported design
from 927 to **868 parts**, preserving its visible footprint.

Windows are actual rotated plates; no mesh is stretched into a window. An LDraw
type-1 reference carries its true affine pose in LDU. The vertical window rotation
uses a rigid axis permutation with rows `[0,0,1,x]`, `[-1,0,0,y]`,
`[0,-1,0,z]`, `[0,0,0,1]`, turning a 2×4 plate into a tall narrow pane.
Nominal dimensions remain unchanged in metric space.

The real MCP calls are:

1. `place_submodel` with the generated inline MPD. This applies a complete batch
   atomically and preserves reusable subparts.
2. `validate_construction` and paged `list_bricks` for the actual count/BOM.
3. `test_visual_appearance` for front and street cameras with `lighting: "studio"`.
4. `test_stability_by_gravity` with the default one-step probe, without mutating
   the candidate.

For example, a street camera uses public `[east,north,height]` units:

```json
{
  "name": "test_visual_appearance",
  "arguments": {
    "camera": {
      "position_world": [65,-95,65],
      "target_world": [16,5,21],
      "vertical_fov_degrees": 38
    },
    "width": 768, "height": 768, "lighting": "studio"
  }
}
```

## Recorded result, 2026-10-24

The 868-part run is body-overlap-free and passes the zero-step static certificate
under the current ideal joint model. **It is not publication-ready.** The
connector book lacks a measured capacity for `41539.dat ↔ 3007.dat`, fixed,
2×6-stud overlap. No clutch capacity was invented.

The strict one-step gravity probe also reports about **0.03767 construction units
of maximum motion**, above its default 0.001 position tolerance, so that probe
does not pass. Its calibrated-stability result is false. A static certificate,
finite solver probe, and measured strength are distinct checks; the script does
not relax tolerances just to obtain a green result.

The tested files are in `target/haussmann-mcp-final-20261024/`. Production
publication requires resolving the measurements and diagnostics using the
sandbox's frozen model; setting a client-supplied guessed force cannot bypass
that requirement. See [physical calibration](physical-calibration.md).

### Generate MCP renders locally

[Download the LDraw/MPD proposal](examples/haussmann.mpd). Its front and street
renderings are reproducible MCP WebGL captures and are not committed to Git.
Generate them with a fresh output directory:

```sh
python3 scripts/build-haussmann.py --binary target/debug/ergo-mcp \
  --output target/haussmann-preview --documentation-assets docs/examples
```

The command writes `docs/examples/haussmann-front.png` and
`docs/examples/haussmann-street.png`, both ignored by Git, and refreshes the
versioned MPD source. The captures show the actual model; they do not establish
that it passed calibrated admission.

For an agent-driven modification of an accepted construction, use the
[current/specified-position workflow](agent-shared-editing.md). Ask the agent to
preserve the shape while minimizing part count, inspect actual images after each
meaningful design change, and distinguish a proposal from a confirmed S3 edit.
