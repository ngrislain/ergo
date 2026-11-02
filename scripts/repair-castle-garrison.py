#!/usr/bin/env python3
"""Prepare and verify a local castle garrison repair using the actual WASM MCP.

Input is a complete accepted workspace response. Architecture, flags and their
piece IDs are retained. Elevated old guards become archers; courtyard guards get
reposed spear/shield hands. This command does not publish or invent capacities.
"""
import argparse
import base64
import json
import math
import pathlib
from collections import defaultdict

from mcp_client import Mcp
from native_model import CONSTRUCTS, instance, yaw

ROOT = pathlib.Path(__file__).resolve().parents[1]
IDENTITY = [[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]]


def multiply(a, b):
    return [[sum(a[i][k]*b[k][j] for k in range(4)) for j in range(4)] for i in range(4)]


def placements(model) -> list[dict]:
    """Walk definition instances, retaining every occurrence's world pose."""
    def visit(name, matrix, color, active):
        if name in active: raise ValueError('Cyclic model definition')
        for ref in model['definitions'][name]:
            target = ref['target']
            pose = multiply(matrix, ref['transform'])
            tint = ref.get('color', color)
            if target['kind'] == 'construct' and target['id'] == 'medieval-guard-v1':
                yield dict(transform=pose, color=tint)
            elif target['kind'] == 'definition':
                yield from visit(target['id'], pose, tint, (*active, name))
    return list(visit(model['root'], IDENTITY, 16, ()))


def geometry_key(part, color, matrix):
    return (part, color, *(round(matrix[i][j], 7) for i in range(3) for j in range(4)))


def brick_key(brick):
    # Scene matrices are public column-major 3x3; model matrices are row-major 4x4.
    rotation = brick['transform']
    matrix = [[rotation[3*j+i] for j in range(3)]+[brick['position'][i]] for i in range(3)]
    return geometry_key(brick['part'], brick['color'], matrix)


def proposal(scene, archer_inset=2.0, archer_lateral=-1.0) -> tuple[dict, list[int], dict[str, int]]:
    old = placements(scene['model'])
    if not old: raise ValueError('No medieval-guard-v1 occurrences found in the complete model')
    index = defaultdict(list)
    for brick in scene['bricks']: index[brick_key(brick)].append(brick['id'])
    replacement = []
    removed = []
    counts = defaultdict(int)
    lowest = min(p['transform'][2][3] for p in old)
    highest = max(p['transform'][2][3] for p in old)
    for placement in old:
        model_id = 'medieval-archer-v1' if placement['transform'][2][3] > lowest+1e-6 else 'medieval-guard-v3'
        revised = dict(placement)
        if model_id == 'medieval-archer-v1':
            inset = [[float(v) for v in row] for row in IDENTITY]
            # The keep's dense rooftop rows already face each other across a
            # narrow platform; retain those foot slots rather than converging.
            if placement['transform'][2][3] < highest-1e-6:
                inset[0][3] = archer_lateral
                inset[1][3] = archer_inset
            revised['transform'] = multiply(placement['transform'], inset)
        replacement.append(dict(target=dict(kind='construct', id=model_id), **revised))
        counts[model_id] += 1
        for part in CONSTRUCTS['medieval-guard-v1']['parts']:
            pose = instance('part', part['part'], part['position'], part.get('color', placement['color']),
                            part.get('rotation') or yaw(part.get('quarter_turns', 0)*math.pi/2))
            key = geometry_key(part['part'], pose['color'], multiply(placement['transform'], pose['transform']))
            matches = index.get(key)
            if not matches: raise ValueError(f'Cannot match original guard component exactly: {part["part"]}')
            removed.append(matches.pop())
    if len(removed) != len(set(removed)): raise ValueError('Repeated guard component identity')
    return dict(format='ergo-model-1', schema=2, root='repaired-castle-garrison',
                definitions={'repaired-castle-garrison': replacement}), sorted(removed), dict(counts)


def complete_bricks(client):
    bricks = []
    while True:
        page = client.call('list_bricks', offset=len(bricks), limit=200)['structuredContent']
        bricks.extend(page['bricks'])
        if len(bricks) == page['total']: return bricks
        if not page['bricks']: raise ValueError('Incomplete MCP readback')


def verify_unchanged(before, after, removed):
    after = {b['id']: b for b in after}
    retained = [b for b in before if b['id'] not in removed]
    for brick in retained:
        updated = after[brick['id']]
        if any(updated[k] != brick[k] for k in ['part', 'color']): raise ValueError('Untouched appearance changed')
        if any(abs(a-b) > 1e-6 for a, b in zip(updated['position'], brick['position'])):
            raise ValueError('Untouched position changed')
        if any(abs(a-b) > 1e-6 for edge in ['min', 'max'] for a, b in zip(updated['bounds'][edge], brick['bounds'][edge])):
            raise ValueError('Untouched bounds changed')
    if set(removed) & after.keys(): raise ValueError('An old guard component was not replaced')
    return len(retained)


def native_cache(scene):
    result = {k: scene[k] for k in ['schema', 'name', 'geo', 'model', 'meshes', 'joints', 'fixed_bricks', 'groups', 'ground']}
    result.update(format='ergo-construction-1', pieces=[{k: b[k] for k in ['id', 'rgb', 'step']} for b in scene['bricks']])
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input', type=pathlib.Path, required=True, help='Complete accepted workspace JSON')
    parser.add_argument('--output', type=pathlib.Path, required=True, help='New local native construction draft')
    parser.add_argument('--binary', type=pathlib.Path, default=ROOT/'target/debug/ergo-mcp')
    parser.add_argument('--artifacts', type=pathlib.Path)
    parser.add_argument('--archer-inset', type=float, default=2.0, help='Step archers inward from the old parapet-edge placement, in studs')
    parser.add_argument('--archer-lateral', type=float, default=-1.0, help='Shift archers along the parapet to leave room for bows, in studs')
    parser.add_argument('--allow-uncertified-preview', action='store_true', help='Retain/render an overlap-free local draft even if its static load path is uncertified; never enables publication')
    args = parser.parse_args()
    if args.output.exists(): raise ValueError('Refusing to replace an existing working copy')
    source = json.loads(args.input.read_text())
    scene = source['scene']
    model, removed, counts = proposal(scene, args.archer_inset, args.archer_lateral)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(native_cache(scene)))
    args.output.with_suffix('.proposal.json').write_text(json.dumps(dict(model=model, replace_ids=removed)))
    client = Mcp(str(args.binary.resolve()), args.output.resolve(), None, None)
    try:
        result = client.call('place_submodel', model=model, replace_ids=removed)['structuredContent']
        after = complete_bricks(client)
        unchanged = verify_unchanged(scene['bricks'], after, set(removed))
        client.call('export_construction')
        report = dict(preview_only=True, published=False, source_revision=source['base_revision'],
                      source_assembly=source['assembly_id'], replaced_old_components=len(removed),
                      unchanged_pieces_verified=unchanged, counts=counts, feedback=result)
        args.output.with_suffix('.report.json').write_text(json.dumps(report, indent=2))
        if not result['overlap_free']:
            raise ValueError('Draft is not overlap-free')
        if not result['static_under_current_joint_model'] and not args.allow_uncertified_preview:
            raise ValueError('Draft static certificate failed: '+str(result.get('failure')))
        if args.artifacts:
            args.artifacts.mkdir(parents=True, exist_ok=True)
            views = [('castle', [385, -80, 210], [240, 160, 32], 40)]
            for label, target_id in [('guards', 'medieval-guard-v3'), ('archers', 'medieval-archer-v1')]:
                ref = next(r for r in model['definitions'][model['root']] if r['target']['id'] == target_id)
                pose = ref['transform']
                point = lambda v: [sum(pose[i][j]*v[j] for j in range(3))+pose[i][3] for i in range(3)]
                views.append((label, point([8, -11, 9]), point([2, 1.5, 3.4]), 40))
            for label, eye, target, fov in views:
                image = client.call('test_visual_appearance', width=1024, height=768, lighting='studio',
                                    camera=dict(position_world=eye, target_world=target, vertical_fov_degrees=fov))
                png = next(c['data'] for c in image['content'] if c['type'] == 'image')
                (args.artifacts/f'{label}.png').write_bytes(base64.b64decode(png))
        print(json.dumps({k: v for k, v in report.items() if k != 'feedback'} | dict(
            pieces=result['brick_count'], overlap_free=result['overlap_free'],
            static_load_path_verified=result['static_under_current_joint_model'],
            publication_ready=result['publication_ready'], calibration_issue=result.get('calibration_issue'),
            static_failure=result.get('failure'))), flush=True)
    finally: client.close()


if __name__ == '__main__': main()
