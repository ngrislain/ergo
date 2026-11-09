#!/usr/bin/env python3
"""Author a supported terraced landscape through the actual stdio MCP.

The landscape is an original adaptation of the mountain/terrace references
listed in REFERENCES. Publication is explicit and journalled; uncertain writes
must be reconciled before continuing. Articulated pedestrians are a separate
stage requiring sourced leg interfaces and measured joint-dependent loads.
"""
import argparse
import base64
import json
import pathlib
import subprocess
import sys
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from scripts.build.district import empty_workspace, write
from scripts.mcp_client import Mcp
from scripts.native_model import Model, PARTS, model_leaves

WIDTH, DEPTH = 192, 160
NAME = 'Parc des Deux Sommets'
REFERENCES = [
    dict(model_id='ldraw:1141:1404', submodel='31025 - tree.ldr',
         author='Stefan Frenz [smf]', license='CCAL version 2.0',
         url='https://library.ldraw.org/omr/sets/1141'),
    dict(model_id='ldraw:312:1099', submodel='3061 - terasse.ldr',
         author='Philippe Hurbain [Philo]', license='CCAL version 2.0',
         url='https://library.ldraw.org/omr/sets/312'),
]
TREES = [(12, 12), (32, 16), (80, 12), (112, 14), (166, 14), (178, 32),
         (16, 48), (80, 54), (184, 80), (4, 100), (4, 138), (78, 156),
         (32, 156), (184, 138), (184, 112), (140, 14)]
HILLS = [(48, 112, 20), (136, 112, 24)]
WORLD = 'fde184c4-222b-4a16-8a1f-c320da8a2f35'


def hill_height(x, y):
    # Bevelled, asymmetric ridges rather than square display pyramids. Every
    # occupied upper cell has complete masonry bearing beneath it.
    heights = [0]
    for cx, cy, height in HILLS:
        for px, py, peak in [(cx, cy, height), (cx+10, cy+6, height-6)]:
            dx, dy = x-px, y-py
            east = abs(dx)*(1.25 if dx < 0 else 1)
            north = abs(dy)*(1.3 if dy > 0 else 1)
            distance = max(east, north, .72*(east+north))
            heights.append(peak-int(distance/2))
    return max(heights)


def surface(x, y):
    if any(((x-cx)/rx)**2 + ((y-cy)/ry)**2 < 1
           for cx, cy, rx, ry in [(52, 44, 20, 12), (136, 46, 16, 10)]):
        return 'water'
    loop = (20 <= x < 172 and (22 <= y < 30 or 72 <= y < 80)) or (
        22 <= y < 80 and (20 <= x < 28 or 164 <= x < 172))
    entrances = 90 <= x < 102 and y < 80
    climbs = any(abs(x-cx) < 4 and 76 <= y <= cy for cx, cy, _ in HILLS)
    return 'path' if loop or entrances or climbs else 'grass'


def rectangles(cells, parts, reverse=False):
    """Greedy exact cover on a two-stud raster using real rectangular parts."""
    cells = set(cells)
    if reverse:
        cells = {(y, x) for x, y in cells}
    shapes = sorted({(int(w*d), part, int(w), int(d), turn)
                     for part in parts for w, d, turn in [
                         (*PARTS[part][:2], False), (PARTS[part][1], PARTS[part][0], True)]
                     if w % 2 == 0 and d % 2 == 0}, reverse=True)
    while cells:
        x, y = min(cells, key=lambda p: (p[1], p[0]))
        for _, part, w, d, turn in shapes:
            footprint = {(x+dx, y+dy) for dx in range(0, w, 2) for dy in range(0, d, 2)}
            if footprint <= cells:
                cells -= footprint
                yield (part, y, x, not turn) if reverse else (part, x, y, turn)
                break
        else:
            raise ValueError('Cannot cover terrain raster with sourced parts')


def landscape(origin):
    model = Model()
    raster = [(x, y) for y in range(0, DEPTH, 2) for x in range(0, WIDTH, 2)]
    for kind, colour in [('grass', 288), ('path', 19), ('water', 321)]:
        cells = [(x, y) for x, y in raster if surface(x+1, y+1) == kind]
        parts = ['3068b'] if kind in ['path', 'water'] else [
            '41539', '3035', '3032', '3031', '3020', '3022', '3958', '3036']
        for part, x, y, turn in rectangles(cells, parts):
            model.part(part, colour, x, y, 0, turn)
    for level in range(24):
        for colour in [72, 71, 15, 19]:
            cells = []
            for x, y in raster:
                height = hill_height(x+1, y+1)
                if height <= level:
                    continue
                shade = 19 if surface(x+1, y+1) == 'path' and height == level+1 else (
                    15 if level >= 17 else 71 if height == level+1 and (x//8+y//6) % 5 == 0 else 72)
                if shade == colour:
                    cells.append((x, y))
            # Alternate long-brick direction between courses for running bond.
            parts = ['4204', '4201', '3007', '2456', '3001', '3003']
            for part, x, y, turn in rectangles(cells, parts, reverse=bool(level % 2)):
                model.part(part, colour, x, y, 1/3+level, turn)
    tree = Model()
    for course in range(5):
        tree.part('3003', 70, 0, 0, course)
    tree.part('3032', 288, -1, -2, 5)
    tree.part('3003', 288, 0, 0, 16/3)
    tree.part('3031', 2, -1, -1, 19/3)
    tree.part('3003', 2, 0, 0, 20/3)
    tree.part('3022', 288, 0, 0, 23/3)
    model.definitions['conifer'] = tree.definitions['main']
    for x, y in TREES:
        assert surface(x+1, y+1) == 'grass' and hill_height(x+1, y+1) == 0, (x, y)
        model.reference('conifer', x, y, 1/3)
    return model.export(origin)


def walking_route(origin):
    points = [(97, -3), (97, 27), (25, 27), (25, 77), (97, 77),
              (169, 77), (169, 27), (97, 27), (97, 77)]
    waypoints = []
    def add(x, y, name):
        level = 0 if y < 0 else 1/3+hill_height(x, y)
        waypoints.append(dict(name=name, feet=[x, y, level]))
    for index, (x, y) in enumerate(points):
        add(x, y, f'pond-loop-{index}')
    for cx, cy, _ in HILLS:
        add(cx+1, 77, f'mountain-{cx}-trailhead')
        for y in range(78, cy+1, 2):
            # Raster cells are evaluated at their centre, including the summit.
            add(cx+1, y+1, f'mountain-{cx}-ascent-{y}')
        for y in range(cy-2, 75, -2):
            add(cx+1, y+1, f'mountain-{cx}-descent-{y}')
        add(cx+1, 77, f'mountain-{cx}-return')
    add(97, 77, 'central-junction')
    add(97, -3, 'entrance-return')
    return dict(format='ergo-walk-route-1', schema=2, origin_world=origin,
                start=[97, -3, 0], eye_height=3.4, min_clearance=4,
                speed=8, waypoints=waypoints)


def check_walk(workspace, output, origin):
    write(output/'walking.route.json', walking_route(origin))
    walk = subprocess.run([str(ROOT/'target/debug/examples/check_walk'), str(workspace),
                           str(output/'walking.route.json')], capture_output=True, text=True)
    evidence = json.loads(walk.stdout) if walk.returncode == 0 else dict(passed=False, error=walk.stderr)
    write(output/'walking.result.json', evidence)
    return walk.returncode == 0


def capture(client, output, origin):
    for label, eye, target in [('overview', [250, -90, 180], [96, 78, 4]),
                                ('ponds', [110, -38, 30], [96, 50, 2]),
                                ('mountains', [205, 40, 65], [96, 112, 12])]:
        world = lambda point: [point[i]+origin[i] for i in range(3)]
        image = client.call('test_visual_appearance', width=1024, height=768,
                            lighting='studio', camera=dict(position_world=world(eye),
                            target_world=world(target), vertical_fov_degrees=45))
        encoded = next(c['data'] for c in image['content'] if c['type'] == 'image')
        (output/f'{label}.png').write_bytes(base64.b64decode(encoded))


def survey(url, output, origin):
    def read(path):
        with urllib.request.urlopen(url.rstrip('/')+path, timeout=300) as response:
            return json.load(response)
    config = read('/api/config')
    assert config['common_world'] == WORLD and config['block_size'] == 128
    centre = [(origin[0]+WIDTH//2)//128, (origin[1]+DEPTH//2)//128]
    packet = read(f'/api/shared-worlds/{WORLD}/view?x={centre[0]}&y={centre[1]}&z=0')
    low, high = origin, [origin[0]+WIDTH, origin[1]+DEPTH, 40]
    conflicts = [brick['id'] for brick in packet['scene']['bricks'] if all(
        brick['bounds']['max'][a] > low[a] and brick['bounds']['min'][a] < high[a]
        for a in range(3))]
    write(output/'site-survey.json', dict(revision=packet['revision'], origin=origin,
                                        nearby_pieces=len(packet['scene']['bricks']), conflicts=conflicts))
    if conflicts:
        raise RuntimeError('The park site contains accepted work; choose a clear site')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=pathlib.Path, required=True)
    parser.add_argument('--origin', nargs=3, type=int, default=[640, 0, 0])
    parser.add_argument('--binary', default=str(ROOT/'target/debug/ergo-mcp'))
    parser.add_argument('--url', default='https://ergosandbox.com')
    parser.add_argument('--token-file', type=pathlib.Path)
    parser.add_argument('--publish', action='store_true')
    parser.add_argument('--verify-walk-only', action='store_true',
                        help='Read the existing cache and check the current route without authoring')
    parser.add_argument('--inspect-existing', action='store_true',
                        help='Render and walk-check the existing cache without authoring')
    args = parser.parse_args()
    if args.origin[2] != 0:
        parser.error('This grounded landscape requires origin height zero')
    args.output.mkdir(parents=True, exist_ok=True)
    journal_path = args.output/'progress.json'
    workspace = args.output/'park.cache.json'
    if args.verify_walk_only or args.inspect_existing:
        origin = json.loads(journal_path.read_text())['origin']
        if args.inspect_existing:
            client = Mcp(args.binary, workspace, None, None)
            try:
                capture(client, args.output, origin)
            finally:
                client.close()
        passed = check_walk(workspace, args.output, origin)
        print(json.dumps(dict(walking_passed=passed)), flush=True)
        if not passed:
            raise RuntimeError('Walking route failed; inspect walking.result.json')
        return
    if journal_path.exists():
        raise RuntimeError('Existing journal: inspect/reconcile its outcome before another run')
    if args.publish and not args.token_file:
        parser.error('--publish requires --token-file')
    if workspace.exists():
        raise RuntimeError('Preserve the existing workspace; choose a fresh output directory')
    if args.publish:
        survey(args.url, args.output, args.origin)
    write(workspace, empty_workspace(NAME))
    client = Mcp(args.binary, workspace, args.url if args.publish else None,
                 args.token_file if args.publish else None)
    try:
        for index, ref in enumerate(REFERENCES):
            source = client.call('get_lego_models', model_id=ref['model_id'],
                                 submodel=ref['submodel'])['structuredContent']
            write(args.output/f'reference-{index}.json', source)
        write(args.output/'techniques.json', client.call('get_building_techniques', query='bond')['structuredContent'])
        model = landscape(args.origin)
        write(args.output/'landscape.model.json', model)
        write(args.output/'walking.route.json', walking_route(args.origin))
        parts = sorted({leaf['part'] for leaf in model_leaves(model)})
        for part in parts:
            description = client.call('get_catalog', part=part)['structuredContent']['requested_part']
            if not description.get('mass_kg') or description.get('quality') != 'source':
                raise RuntimeError('Genuine source geometry and sourced mass are required: '+part)
        journal: dict = dict(state='attempting' if args.publish else 'local-preview',
                       name=NAME, origin=args.origin, references=REFERENCES, stages=[],
                       pedestrians='Not authored: articulated legs require separate reviewed data')
        refs = model['definitions']['main']
        result = None
        for offset in range(0, len(refs), 600):
            stage: dict = dict(offset=offset, state='attempting')
            journal['stages'].append(stage)
            write(journal_path, journal)
            batch = dict(model, definitions=dict(model['definitions']))
            batch['definitions']['main'] = refs[offset:offset+600]
            result = client.call('place_submodel', model=batch)['structuredContent']
            write(args.output/f'placement-{offset:05}.json', result)
            if args.publish and not result.get('shared_commit'):
                raise RuntimeError('No confirmed shared receipt')
            stage.update(state='accepted' if result.get('shared_commit') else 'local-preview', result=result)
            write(journal_path, journal)
            print(json.dumps(dict(stage=offset//600, pieces=result['brick_count'],
                                  overlap_free=result['overlap_free'],
                                  static=result['static_under_current_joint_model'],
                                  failure=result.get('failure'))), flush=True)
            if not result['overlap_free']:
                raise RuntimeError('Landscape overlap: inspect the placement feedback')
            if args.publish:
                client.call('open_shared_construction', assembly_id=result['shared_commit']['assembly_id'])
        assert result is not None
        journal['state'] = 'accepted' if args.publish else 'local-preview'
        write(journal_path, journal)
        exported = client.export_path()
        with client.inspection_copy(exported) as inspector:
            capture(inspector, args.output, args.origin)
        passed = check_walk(exported, args.output, args.origin)
        print(json.dumps(dict(published=bool(result.get('shared_commit')), pieces=result['brick_count'],
                              overlap_free=result['overlap_free'],
                              static=result['static_under_current_joint_model'],
                              walking_passed=passed, pedestrians=False)), flush=True)
        if not passed:
            raise RuntimeError('Walking route failed; inspect walking.result.json')
    finally:
        client.close()


if __name__ == '__main__':
    main()
