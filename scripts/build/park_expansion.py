#!/usr/bin/env python3
"""Expand the accepted park coherently, retaining its masonry and assembly ID.

The original brick-built trees are replaced by genuine 3470/3471 moulds. A new
lake, forest paths and a low woodland hill occupy the surveyed extension. Local
previews and authenticated publication use the actual WASM MCP; writes are never
retried automatically.
"""
import argparse
import base64
import collections
import json
import os
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from scripts.build import park
from scripts.build.district import write, empty_workspace
from scripts.mcp_client import Mcp
from scripts.native_model import Model, instance, model_leaves

WIDTH, DEPTH = 384, 256
ORIGIN = [640, 0, 0]
ASSEMBLY = '6e8cdd606371e98850c23192750641a8931671d77eb3396c8b577d344cbda9fd'
HILL = (288, 196, 12)


def extension_cell(x, y):
    return x >= park.WIDTH or y >= park.DEPTH


def extension_height(x, y):
    heights = [0]
    cx, cy, peak = HILL
    for px, py, height in [(cx, cy, peak), (cx+6, cy+4, peak-4)]:
        dx, dy = x-px, y-py
        east = abs(dx)*(1.25 if dx < 0 else 1)
        north = abs(dy)*(1.3 if dy > 0 else 1)
        heights.append(height-int(max(east, north, .72*(east+north))/2))
    return max(heights)


def extension_surface(x, y):
    if ((x-280)/46)**2 + ((y-76)/26)**2 < 1:
        return 'water'
    loop = (216 <= x < 368 and (32 <= y < 40 or 120 <= y < 128)) or (
        32 <= y < 128 and (216 <= x < 224 or 360 <= x < 368))
    north_walk = 20 <= x < 368 and 228 <= y < 236
    returns = (360 <= x < 368 and 124 <= y < 236) or (92 <= x < 100 and 160 <= y < 236)
    trail = abs(x-HILL[0]) < 4 and 124 <= y <= HILL[1]
    return 'path' if loop or north_walk or returns or trail else 'grass'


def raised_path(x, y):
    return (172 <= x < 216 and 56 <= y < 64) or (92 <= x < 98 and 80 <= y < 160)


def clear_tree_position(x, y):
    if not (3 <= x < WIDTH-3 and 3 <= y < DEPTH-3):return False
    for dx in [-3, 0, 3]:
        for dy in [-3, 0, 3]:
            a, b = x+dx, y+dy
            height = extension_height(a, b) if extension_cell(a, b) else park.hill_height(a, b)
            surface = extension_surface(a, b) if extension_cell(a, b) else park.surface(a, b)
            if height or surface != 'grass' or raised_path(a, b):return False
    return True


def trees():
    original = []
    offsets = sorted([(dx, dy) for dx in range(-8, 9) for dy in range(-8, 9)],
                     key=lambda p: (p[0]**2+p[1]**2, abs(p[1]), p[1], p[0]))
    for x, y in park.TREES:
        point = next(((x+1+dx, y+1+dy) for dx, dy in offsets if clear_tree_position(x+1+dx, y+1+dy)), None)
        assert point, (x, y)
        original.append(point)
    candidates = []
    for y in range(12, DEPTH-12, 16):
        for x in range(12, WIDTH-12, 16):
            if not extension_cell(x, y):
                continue
            if clear_tree_position(x, y) and all((x-a)**2+(y-b)**2 >= 64 for a, b in original):
                candidates.append((x, y))
    candidates.sort(key=lambda point: ((point[0]*17+point[1]*31) % 97, point[1], point[0]))
    assert len(candidates) >= 48
    return original+candidates[:48]


def additions(origin=ORIGIN):
    model = Model()
    for index, (x, y) in enumerate(trees()):
        model.definitions['main'].append(instance('part', '3470.dat' if index % 2 == 0 else '3471.dat', [x, y, 2/3], 2))
    raster = [(x, y) for y in range(0, DEPTH, 2) for x in range(0, WIDTH, 2) if extension_cell(x, y)]
    for kind, colour in [('grass', 288), ('path', 19), ('water', 321)]:
        cells = [(x, y) for x, y in raster if extension_surface(x+1, y+1) == kind]
        parts = ['3068b'] if kind != 'grass' else ['41539', '3035', '3032', '3031', '3020', '3022', '3958', '3036']
        for part, x, y, turn in park.rectangles(cells, parts):
            model.part(part, colour, x, y, 0, turn)
    for level in range(HILL[2]):
        for colour in [72, 71, 19]:
            cells = []
            for x, y in raster:
                height = extension_height(x+1, y+1)
                if height <= level:
                    continue
                shade = 19 if extension_surface(x+1, y+1) == 'path' and height == level+1 else (
                    71 if height == level+1 and (x//8+y//6) % 5 == 0 else 72)
                if shade == colour:
                    cells.append((x, y))
            for part, x, y, turn in park.rectangles(cells, ['4204', '4201', '3007', '2456', '3001', '3003'], reverse=bool(level % 2)):
                model.part(part, colour, x, y, 1/3+level, turn)
    # Thin paved connections rest on the retained lawn, never replacing the
    # accepted masonry underneath them or leaving an unsupported intermediate edit.
    for x, y, width, depth in [(172, 56, 44, 8), (92, 80, 6, 80)]:
        for east in range(x, x+width, 2):
            for north in range(y, y+depth, 2):
                if not extension_cell(east, north):
                    assert park.hill_height(east+1, north+1) == 0
                else:
                    assert extension_height(east+1, north+1) == 0
                model.part('3068b', 19, east, north, 1/3)
    return model.export(origin)


def full_model(origin=ORIGIN):
    original = park.landscape([0, 0, 0])
    retained = [ref for ref in original['definitions']['main'] if ref['target'] != dict(kind='definition', id='conifer')]
    model = Model()
    model.definitions['main'] = retained+additions([0, 0, 0])['definitions']['main']
    return model.export(origin)


def key(part, colour, matrix):
    return part, colour, *(round(v, 7) for row in matrix[:3] for v in row)


def brick_key(brick):
    rotation = brick['transform']
    matrix = [[rotation[3*j+i] for j in range(3)]+[brick['position'][i]] for i in range(3)]
    return key(brick['part'], brick['color'], matrix)


def tree_ids(bricks, origin=ORIGIN):
    index = collections.defaultdict(list)
    for brick in bricks:
        index[brick_key(brick)].append(brick['id'])
    original = park.landscape(origin)
    original['definitions']['main'] = [ref for ref in original['definitions']['main'] if ref['target'] == dict(kind='definition', id='conifer')]
    removed = []
    for leaf in model_leaves(original):
        matches = index[key(leaf['part'], leaf['color'], leaf['transform'])]
        if not matches:
            raise ValueError('An original tree component differs from the accepted baseline; inspect before replacing it')
        removed.append(matches.pop())
    assert len(removed) == len(set(removed)) == 160
    return sorted(removed)


def route(origin=ORIGIN):
    result: dict = park.walking_route(origin)
    points: list = result['waypoints']
    def add(x, y, label):
        height = (1/3+park.hill_height(x, y)) if not extension_cell(x, y) else (1/3+extension_height(x, y))
        if not (0 <= x < WIDTH and 0 <= y < DEPTH):height = 0
        if raised_path(x, y):height = 2/3
        points.append(dict(name=label, feet=[x, y, height]))
    for x, y, label in [(97, 77, 'old-junction'), (95, 81, 'woodland-pass-entry'), (95, 159, 'woodland-pass'),
                         (95, 165, 'north-connection'), (95, 233, 'north-promenade'), (365, 233, 'forest-east'),
                         (365, 125, 'new-lake-north-east'), (365, 37, 'new-lake-south-east'),
                         (217, 37, 'new-lake-south-west'), (217, 125, 'new-lake-north-west'), (289, 125, 'woodland-hill-trail')]:
        add(x, y, label)
    for y in range(127, HILL[1]+2, 2):add(289, y, f'woodland-hill-ascent-{y}')
    for y in range(HILL[1]-1, 124, -2):add(289, y, f'woodland-hill-descent-{y}')
    for x, y, label in [(217, 125, 'new-lake-return'), (217, 61, 'east-connection'), (215, 61, 'paved-connection'),
                         (173, 61, 'retained-lawn-crossing'), (169, 61, 'old-lake-loop'), (169, 27, 'old-loop-south'),
                         (97, 27, 'entrance-junction'), (97, -3, 'entrance-return-expanded')]:
        add(x, y, label)
    return result


def cache(scene):
    result = {field: scene[field] for field in ['schema', 'name', 'geo', 'model', 'meshes', 'joints', 'fixed_bricks', 'groups', 'ground']}
    assert result['model']
    result.update(format='ergo-construction-1', pieces=[{field: brick[field] for field in ['id', 'rgb', 'step']} for brick in scene['bricks']])
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input', type=pathlib.Path, required=True, help='Complete accepted authoring workspace response')
    parser.add_argument('--output', type=pathlib.Path, required=True)
    parser.add_argument('--binary', default=str(ROOT/'target/debug/ergo-mcp'))
    parser.add_argument('--url', default='https://ergosandbox.com')
    parser.add_argument('--token-file', type=pathlib.Path)
    parser.add_argument('--publish', action='store_true')
    args = parser.parse_args()
    source = json.loads(args.input.read_text())
    assert source['assembly_id'] == ASSEMBLY and not source['external_dependencies']
    args.output.mkdir(parents=True, exist_ok=True)
    workspace, journal = args.output/'park.cache.json', args.output/'progress.json'
    if workspace.exists() or journal.exists():raise ValueError('Preserve the existing attempt; reconcile before another write')
    if args.publish and not args.token_file:parser.error('--publish requires --token-file')
    removed = tree_ids(source['scene']['bricks'])
    delta = additions()
    if len(json.dumps(dict(model=delta, replace_ids=removed)).encode()) > 950_000:
        raise ValueError('Atomic proposal exceeds the bounded MCP line; compact definitions before sending')
    write(args.output/'additions.model.json', delta)
    write(args.output/'complete.model.json', full_model())
    write(args.output/'walking.route.json', route())
    write(workspace, empty_workspace(park.NAME) if args.publish else cache(source['scene']))
    if args.publish:os.environ['ERGO_SHARED_ASSEMBLY_ID'] = ASSEMBLY
    client = Mcp(args.binary, workspace, args.url if args.publish else None, args.token_file if args.publish else None)
    try:
        if args.publish:
            sidecar = workspace.with_suffix('.shared.json')
            binding = json.loads(sidecar.read_text()) if sidecar.exists() else {}
            if binding.get('assembly_id') != ASSEMBLY:
                binding = client.call('open_shared_construction', assembly_id=ASSEMBLY)['structuredContent']
            assert binding['base_revision'] == source['base_revision'], 'Accepted head changed; inspect/rebase the proposal instead of overwriting work'
            assert not binding.get('pending'), 'Reconcile the pending authoring outcome before continuing'
        for part in sorted({leaf['part'] for leaf in model_leaves(delta)}):
            description = client.call('get_catalog', part=part)['structuredContent']['requested_part']
            assert description['quality'] == 'source' and description.get('mass_kg'), part
        write(journal, dict(state='attempting' if args.publish else 'local-preview', assembly_id=ASSEMBLY,
                            base=source['base_revision'], removed_tree_components=removed))
        feedback = client.call('place_submodel', model=delta, replace_ids=removed)['structuredContent']
        write(args.output/'placement.json', feedback)
        if args.publish and not feedback.get('shared_commit'):raise ValueError('No confirmed accepted publication receipt')
        client.call('export_construction')
        exported = json.loads(workspace.read_text())
        before = {brick['id']: brick_key(brick) for brick in source['scene']['bricks'] if brick['id'] not in removed}
        leaves = list(model_leaves(exported['model']))
        assert len(leaves) == len(exported['pieces'])
        after = {piece['id']: key(leaf['part'], leaf['color'], leaf['transform']) for piece, leaf in zip(exported['pieces'], leaves)}
        assert all(after[id] == value for id, value in before.items()), 'Retained masonry changed'
        write(journal, dict(state='accepted' if args.publish else 'local-preview', assembly_id=ASSEMBLY,
                            feedback=feedback, retained_pieces_verified=len(before), genuine_trees=len(trees()), area_factor=3.2))
        walk = subprocess.run([str(ROOT/'target/debug/examples/check_walk'), str(workspace), str(args.output/'walking.route.json')], capture_output=True, text=True)
        write(args.output/'walking.result.json', json.loads(walk.stdout) if walk.returncode == 0 else dict(passed=False, error=walk.stderr))
        image = client.call('test_visual_appearance', width=1024, height=768, lighting='studio',
                            camera=dict(position_world=[1140, -175, 290], target_world=[832, 122, 5], vertical_fov_degrees=45))
        (args.output/'overview.png').write_bytes(base64.b64decode(next(c['data'] for c in image['content'] if c['type'] == 'image')))
        print(json.dumps(dict(published=bool(feedback.get('shared_commit')), pieces=feedback['brick_count'],
                              overlap_free=feedback['overlap_free'], static=feedback['static_under_current_joint_model'],
                              walking_passed=walk.returncode == 0, genuine_trees=len(trees()), retained=len(before))), flush=True)
        if walk.returncode:raise ValueError('Walking check failed; inspect its diagnostic before publication')
    finally:client.close()


if __name__ == '__main__':main()
