#!/usr/bin/env python3
"""Build an eight-chalet Swiss-style village using the real Ergo MCP transport.

Produces an MPD, a working copy, actual WebGL views and validation receipts.
Generated images stay in the ignored output directory. No implicit publication.
"""
import argparse
import base64
from collections import Counter
import importlib.util
import json
import pathlib
import shutil

ROOT = pathlib.Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("demo", ROOT / "scripts/demo-mcp.py")
assert spec and spec.loader
demo = importlib.util.module_from_spec(spec)
spec.loader.exec_module(demo)
PARTS = {
    "3005": (1, 1, 1), "3004": (2, 1, 1), "3010": (4, 1, 1),
    "3009": (6, 1, 1), "3008": (8, 1, 1), "3003": (2, 2, 1),
    "3001": (4, 2, 1), "3007": (8, 2, 1),
    "3024": (1, 1, 1/3), "3023": (2, 1, 1/3), "3710": (4, 1, 1/3),
    "3666": (6, 1, 1/3), "3022": (2, 2, 1/3), "3020": (4, 2, 1/3),
    "3795": (6, 2, 1/3), "3034": (8, 2, 1/3), "3031": (4, 4, 1/3), "3032": (6, 4, 1/3),
    "3035": (8, 4, 1/3), "41539": (8, 8, 1/3),
    "3068b": (2, 2, 1/3), "2431": (4, 1, 1/3),
}
IDENTITY = (1, 0, 0, 0, 1, 0, 0, 0, 1)
QUARTER = (0, 0, -1, 0, 1, 0, 1, 0, 0)
HALF = (-1, 0, 0, 0, 1, 0, 0, 0, -1)
WINDOW = (1, 0, 0, 0, 0, 1, 0, -1, 0)


def reference(file, color, x, north, height, matrix=IDENTITY):
    values = (x*20, -height*24, north*20, *matrix)
    return f"1 {color} " + " ".join(format(v, ".12g") for v in values) + " " + file


def place(lines, part, color, x, north, height, turn=False):
    width, depth, tall = PARTS[part]
    if turn:
        width, depth = depth, width
    lines.append(reference(part+".dat", color, x+width/2, north+depth/2, height+tall,
                           QUARTER if turn else IDENTITY))


def strip(lines, length, color, x, north, height, turn=False, plate=False, roof=False):
    choices = [("3007", 8), ("3001", 4)] if roof else (
        [("3666", 6), ("3710", 4), ("3023", 2), ("3024", 1)] if plate else
        [("3008", 8), ("3009", 6), ("3010", 4), ("3004", 2), ("3005", 1)])
    offset = 0
    for part, size in choices:
        while length >= size:
            place(lines, part, color, x+(0 if turn else offset),
                  north+(offset if turn else 0), height, turn)
            offset += size
            length -= size
    assert length == 0


def floor(lines, height, color=71):
    for x in range(0, 24, 8):
        for north in [0, 8]:
            place(lines, "41539", color, x, north, height)


def chalet(wall, timber, roof, shutters):
    lines = []
    floor(lines, 0)
    for upper in [False, True]:
        bottom = 17/3 if upper else 1/3
        rows = 4 if upper else 5
        for level in range(rows):
            color = timber if upper else wall
            strip(lines, 24, color, 0, 15, bottom+level)
            for x in [0, 23]:
                strip(lines, 14, color, x, 1, bottom+level, turn=True)
            if level == rows-1:
                strip(lines, 24, color, 0, 0, bottom+level)
            else:
                for x, width in [(0, 3), (7, 3), (12, 3), (19, 5)]:
                    strip(lines, width, color, x, 0, bottom+level)
                if level == 0:
                    for x in [3, 15]:
                        strip(lines, 4, color, x, 0, bottom)
                # Recessed timber door; no stretched or fabricated window meshes.
                strip(lines, 2, 70, 10, 0, bottom+level)
        for x in [3, 15]:
            lines.append(reference("3020.dat", 321, x+2, 0.55, bottom+11/6, WINDOW))
            for side in [x-1, x+4]:
                # Shutters stand on actual timber supports, not floating beside
                # the facade. Upper supports bear on the balcony deck.
                place(lines, "3005", 70, side, -1, bottom if upper else 0)
                if not upper:
                    place(lines, "3024", 70, side, -1, 1)
                for level in [1, 2]:
                    place(lines, "3005", shutters, side, -1, bottom+level)
                if not upper:
                    place(lines, "3005", 70, side, -1, bottom+3)
        strip(lines, 24, 70, 0, -1, bottom+rows-1, plate=True)
        if not upper:
            floor(lines, 16/3, 70)
    # Ground-connected supports, a continuous deck, rail and flower boxes.
    for x in [1, 22]:
        place(lines, "3024", 70, x, -4, 0)
        for level in range(5):
            place(lines, "3005", 70, x, -4, 1/3+level)
    for x in range(0, 24, 8):
        place(lines, "3035", 70, x, -4, 16/3)
    for x in [0, 5, 10, 15, 20, 23]:
        place(lines, "3005", 70, x, -4, 17/3)
    strip(lines, 24, 70, 0, -4, 20/3, plate=True)
    for x in [3, 15]:
        place(lines, "3710", 70, x, -3, 17/3)
        place(lines, "3710", 2, x, -3, 6)
        for i in range(4):
            place(lines, "3024", 4 if i % 2 else 14, x+i, -3, 19/3)
    # Broad overhanging stepped gable roof, ridge and stone chimney.
    roof_base = 29/3
    floor(lines, roof_base, 70)
    roof_base += 1/3
    for tier in range(5):
        front, back = -1+2*tier, 15-2*tier
        for north in sorted({front, back}):
            strip(lines, 28, roof, -2, north, roof_base+tier, roof=True)
        # A stepped bearing core supports each separate roof segment. A hollow
        # roof made of adjacent, unconnected rows would leave floating parts.
        for north in range(front+2, back, 2):
            strip(lines, 24, wall, 0, north, roof_base+tier, roof=True)
    for x, part in [(-2, "3034"), (6, "3034"), (14, "3034"), (22, "3020")]:
        place(lines, part, 72, x, 7, roof_base+5)
    for level in range(3):
        place(lines, "3003", 71, 16, 8, roof_base+16/3+level)
    place(lines, "3068b", 72, 16, 8, roof_base+25/3)
    return lines


def pine():
    lines = []
    for h in range(3):
        place(lines, "3003", 70, -1, -1, h)
    place(lines, "41539", 288, -4, -4, 3)
    place(lines, "3003", 70, -1, -1, 10/3)
    place(lines, "3032", 2, -3, -2, 13/3)
    place(lines, "3003", 70, -1, -1, 14/3)
    place(lines, "3031", 288, -2, -2, 17/3)
    place(lines, "3003", 70, -1, -1, 6)
    place(lines, "3003", 2, -1, -1, 7)
    place(lines, "3005", 288, -0.5, -0.5, 8)
    return lines


def model():
    main = []
    files = {"village.ldr": main,
             "chalet-red.ldr": chalet(15, 19, 320, 288),
             "chalet-slate.ldr": chalet(19, 70, 72, 4),
             "chalet-timber.ldr": chalet(15, 70, 70, 272), "pine.ldr": pine()}
    # Low lawn platform, paved lanes and a compact civic square.
    for x in range(-8, 128, 8):
        for north in range(-8, 104, 8):
            place(main, "41539", 378 if (x+north) % 24 else 2, x, north, 0)
    for row, north in enumerate([0, 72]):
        for column, x in enumerate([0, 32, 64, 96]):
            kind = ["red", "slate", "timber"][(column+row) % 3]
            main.append(reference(f"chalet-{kind}.ldr", 16, x+(24 if row == 0 else 0),
                                  north+(16 if row == 0 else 0), 1/3, HALF if row == 0 else IDENTITY))
    for north in [24, 28, 64]:
        for x in range(0, 120, 8):
            place(main, "3035", 71, x, north, 1/3)
    for x in [32, 80]:
        for north in range(32, 64, 4):
            place(main, "3035", 71, x, north, 1/3)
    for x in range(40, 80, 8):
        for north in range(32, 64, 4):
            place(main, "3035", 7 if (x+north) % 12 else 71, x, north, 1/3)
    for x in [8, 40, 72, 104]:
        for north in [20, 60]:
            if north != 60 or x not in [40, 72]:
                place(main, "3035", 28, x, north, 1/3)
    # Fountain: lower blue basin, raised rim and a small central water column.
    place(main, "41539", 321, 56, 44, 2/3)
    for north in [42, 53]:
        strip(main, 12, 71, 54, north, 2/3)
    for x in [54, 65]:
        strip(main, 10, 71, x, 43, 2/3, turn=True)
    place(main, "3003", 71, 59, 47, 1)
    place(main, "3031", 71, 58, 46, 2)
    place(main, "3005", 321, 59.5, 47.5, 7/3)
    place(main, "3024", 15, 59.5, 47.5, 10/3)
    for x in [2, 18, 102, 118]:
        for north in [36, 56, 96]:
            main.append(reference("pine.ldr", 16, x, north, 1/3))
    # Benches, lamps and planted flower beds frame the square.
    for x in [44, 72]:
        for north in [36, 58]:
            for foot in [x, x+4]:
                place(main, "3005", 72, foot, north, 2/3)
            strip(main, 6, 70, x, north, 5/3, plate=True)
    for x in [41, 78]:
        for north in [33, 62]:
            for h in range(3):
                place(main, "3005", 72, x, north, 2/3+h)
            place(main, "3022", 14, x-0.5, north-0.5, 11/3)
            place(main, "3022", 72, x-0.5, north-0.5, 4)
    for x in [24, 88]:
        for north in [36, 52]:
            place(main, "3035", 70, x, north, 1/3)
            place(main, "3035", 2, x, north, 2/3)
            for dx, dn in [(1, 1), (3, 2), (5, 1), (7, 2)]:
                place(main, "3024", 4 if dx % 3 else 14, x+dx, north+dn, 1)
    return "\r\n".join("0 FILE "+name+"\r\n0 "+name+"\r\n"+"\r\n".join(lines)
                         for name, lines in files.items())+"\r\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default="ergo-mcp")
    parser.add_argument("--output", type=pathlib.Path, default=pathlib.Path("target/swiss-village-mcp"))
    parser.add_argument("--model-only", action="store_true", help="Write MPD source without starting MCP")
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    workspace = output/"swiss-village.ergo.json"
    if workspace.exists():
        parser.error("Choose a fresh output directory; an existing working copy must not be overwritten")
    text = model()
    (output/"swiss-village.mpd").write_bytes(text.encode())
    if args.model_only:
        return
    binary = shutil.which(args.binary)
    if not binary:
        parser.error("Install ergo-mcp or pass --binary target/debug/ergo-mcp")
    mcp = demo.Mcp(binary, workspace, "https://ergosandbox.com", None)
    try:
        placed = mcp.call("place_submodel", document=text, color=71)["structuredContent"]
        validation = mcp.call("validate_construction")["structuredContent"]
        counts = Counter()
        for offset in range(0, validation["brick_count"], 200):
            page = mcp.call("list_bricks", offset=offset, limit=200)["structuredContent"]
            counts.update(b["part"] for b in page["bricks"])
        for name, camera in [
            ("aerial", {"position_world": [180, -115, 135], "target_world": [60, 44, 3], "vertical_fov_degrees": 40}),
            ("square", {"position_world": [58, 39, 24], "target_world": [60, 76, 6], "vertical_fov_degrees": 68}),
            ("chalets", {"position_world": [-35, 34, 33], "target_world": [50, 77, 5], "vertical_fov_degrees": 45}),
        ]:
            result = mcp.call("test_visual_appearance", camera=camera, width=1024, height=768, lighting="studio")
            image = next(c for c in result["content"] if c["type"] == "image")
            (output/f"swiss-village-{name}.png").write_bytes(base64.b64decode(image["data"]))
        report = {"homes": 8, "reusable_chalet_designs": 3, "pine_trees": 12,
                  "brick_count": validation["brick_count"], "parts": dict(sorted(counts.items())),
                  "placement": placed, "validation": validation, "shared_committed": False,
                  "mcp_tools": ["place_submodel", "validate_construction", "list_bricks", "test_visual_appearance"]}
        (output/"report.json").write_text(json.dumps(report, indent=2)+"\n")
        print(json.dumps({"homes": 8, "brick_count": report["brick_count"],
                          "overlap_free": validation["overlap_free"], "publication_ready": validation["publication_ready"],
                          "static_under_current_joint_model": validation["static_under_current_joint_model"],
                          "failure": validation.get("failure"),
                          "calibration_issue": validation.get("calibration_issue"), "output": str(output)}, indent=2))
    finally:
        mcp.close()


if __name__ == "__main__":
    main()
