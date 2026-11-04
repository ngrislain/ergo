#!/usr/bin/env python3
"""Read-only triangle attribution for a saved current WorldExport scene."""
import argparse
import collections
import json
import pathlib

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('scene', type=pathlib.Path)
args = parser.parse_args()
scene = json.loads(args.scene.read_text())
counts = collections.Counter(brick['part'] for brick in scene['bricks'])
rows = [dict(part=part, occurrences=count,
             triangles_per_part=len(scene['meshes'][part]['vertices']) // 27,
             resident_source_triangles=count * (len(scene['meshes'][part]['vertices']) // 27))
        for part, count in counts.items()]
for row in sorted(rows, key=lambda row: -row['resident_source_triangles'])[:20]:
    print(json.dumps(row))
