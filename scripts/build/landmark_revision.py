#!/usr/bin/env python3
"""Prepare a local landmark revision with unchanged accepted occurrence IDs."""
import argparse
import collections
import copy
import json
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from scripts.build.castle_landscape import brick_reference, native, pose_key


def preserve_unchanged(source, desired):
    before = source['scene']
    if before['geo'] != desired['geo']:
        raise ValueError('The proposed revision must retain the geographic reference')
    for scene in [before, desired]:
        if scene['joints'] or scene['fixed_bricks'] or scene['groups']:
            raise ValueError('Authored joints, anchors and groups need an explicit metadata-preserving edit')
    ids = [brick['id'] for brick in before['bricks']]
    if not ids or len(set(ids)) != len(ids) or not desired['bricks']:
        raise ValueError('A complete accepted scene with unique IDs is required')
    matches = collections.defaultdict(list)
    for brick in sorted(before['bricks'], key=lambda brick: brick['id'], reverse=True):
        ref = brick_reference(brick)
        matches[pose_key(brick['part'], brick['color'], ref['transform'])].append(brick)
    retained = set()
    next_id = max(ids) + 1
    references, metadata = [], []
    for brick in desired['bricks']:
        ref = brick_reference(brick)
        candidates = matches[pose_key(brick['part'], brick['color'], ref['transform'])]
        if candidates:
            old = candidates.pop()
            # Keep exact accepted floats and per-occurrence appearance/steps,
            # rather than replacing them with the recipe's rounded equivalents.
            ref = brick_reference(old)
            value = dict(id=old['id'], rgb=old['rgb'], step=old['step'])
            retained.add(old['id'])
        else:
            if next_id > 0xffffffff:
                raise ValueError('No fresh u32 occurrence IDs remain')
            value = dict(id=next_id, rgb=brick['rgb'], step=brick['step'])
            next_id += 1
        references.append(ref)
        metadata.append(value)
    needed = {reference['target']['id'] for reference in references}
    # Existing source records, credits and frozen physical metadata win over a
    # recipe's optional cache; only missing part IDs come from the new draft.
    meshes = {**desired['meshes'], **before['meshes']}
    if needed - meshes.keys() or any(meshes[part]['quality'] != 'source' for part in needed):
        raise ValueError('Every part needs genuine current source geometry')
    document = native(before, references, metadata, {part: copy.deepcopy(meshes[part]) for part in needed}, before['name'])
    document['ground'] = copy.deepcopy(before['ground'])
    report = dict(published=False, assembly_id=source['assembly_id'], base_revision=source['base_revision'],
                  pieces=len(metadata), retained_unmodified_ids=len(retained),
                  removed_occurrences=len(ids)-len(retained), added_occurrences=next_id-max(ids)-1,
                  matching='part, colour and pose rounded to 7 decimal places; duplicate occurrences matched one-to-one',
                  external_dependencies=source.get('external_dependencies'),
                  publication_requires_fresh_preflight_and_complete_domain_validation=True)
    return document, report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input', type=pathlib.Path, required=True, help='Complete accepted workspace')
    parser.add_argument('--draft', type=pathlib.Path, required=True, help='Desired current native construction')
    parser.add_argument('--output', type=pathlib.Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    desired = args.output/'desired.scene.json'
    subprocess.run([str(ROOT/'target/release/examples/materialize_native'), str(args.draft), str(desired)], check=True)
    document, report = preserve_unchanged(json.loads(args.input.read_text()), json.loads(desired.read_text()))
    (args.output/'construction.native.json').write_text(json.dumps(document, separators=(',', ':'))+'\n')
    (args.output/'proposal.json').write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps(report), flush=True)


if __name__ == '__main__':
    main()
