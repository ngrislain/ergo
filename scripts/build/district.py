#!/usr/bin/env python3
"""Incremental district construction through MCP tools, with durable receipts.

No direct REST mutation or duplicate pre-validation/dynamics gate is used.
Uncertain outcomes stop the run and are never automatically retried.
"""
import argparse
from collections import Counter
import hashlib
import json
import os
from pathlib import Path
import shutil
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from scripts.build import district_models as models
from scripts.build import district_streets
from scripts.native_model import register_catalog_part
from scripts import mcp_client as transport


def write(path, value):
    temporary = path.with_suffix(path.suffix + '.tmp')
    with temporary.open('w') as stream:
        json.dump(value, stream, indent=2)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)


def empty_workspace(name):
    return {'format': 'ergo-construction-1', 'schema': 2, 'name': name,
            'geo': {'latitude': 48.8566, 'longitude': 2.3522, 'arcseconds_per_block': 1},
            'model': {'format': 'ergo-model-1', 'schema': 2, 'root': 'main', 'definitions': {'main': []}},
            'pieces': [], 'meshes': {}, 'joints': [], 'fixed_bricks': [], 'groups': [], 'ground': []}


def fingerprints(model):
    result = Counter()
    identity = [[int(row == column) for column in range(4)] for row in range(4)]
    def visit(name, parent, color=16):
        for instance in model['definitions'][name]:
            local = instance['transform']
            matrix = [[sum(parent[i][k] * local[k][j] for k in range(4)) for j in range(4)] for i in range(4)]
            inherited = instance.get('color', color)
            target = instance['target']
            if target['kind'] == 'definition':
                visit(target['id'], matrix, inherited)
            else:
                result[(target['id'], inherited, *[round(value, 6) for row in matrix for value in row])] += 1
    visit(model['root'], identity)
    return result


def combine(stages):
    definitions = {'batch': []}
    identity = [[int(row == column) for column in range(4)] for row in range(4)]
    for label, model in stages.items():
        prefix = label + '/'
        for name, instances in model['definitions'].items():
            copied = json.loads(json.dumps(instances))
            for instance in copied:
                if instance['target']['kind'] == 'definition':
                    instance['target']['id'] = prefix + instance['target']['id']
            definitions[prefix + name] = copied
        definitions['batch'].append({'target': {'kind': 'definition', 'id': prefix + model['root']}, 'transform': identity})
    return {'format': 'ergo-model-1', 'schema': 2, 'root': 'batch', 'definitions': definitions}


def batches(stages, limit):
    result = []
    for stage in stages:
        prefix=stage.split('-',1)[0]
        if result and prefix in ['floor','exterior'] and all(item.startswith(prefix+'-') for item in result[-1]) and len(result[-1]) < limit:
            result[-1].append(stage)
        else:
            result.append([stage])
    return result


def reconcile(args, binary, spec, journal, journal_path):
    progress = journal['decorations' if args.site else 'buildings'][spec['key']]
    stage = args.reconcile
    operation = progress['stages'][stage]
    group = operation.get('batch_stages', [stage])
    if operation['state'] == 'accepted':
        raise RuntimeError('Stage is already accepted; no reconciliation is needed')
    workspace = args.output / (spec['key'] + '.cache.json')
    mcp = transport.Mcp(binary, workspace, args.url, args.token_file)
    try:
        accepted = mcp.call('open_shared_construction', assembly_id=progress['assembly_id'])['structuredContent']
        actual = json.loads(workspace.read_text())
        before = Counter()
        for name, recorded in progress['stages'].items():
            if name not in group and recorded.get('state') == 'accepted':
                before.update(fingerprints(json.loads((args.output / (spec['key'] + '-' + name + '.model.json')).read_text())))
        expected = before.copy()
        for name in group:
            attempted = json.loads((args.output / (spec['key'] + '-' + name + '.model.json')).read_text())
            assert hashlib.sha256(json.dumps(attempted, sort_keys=True).encode()).hexdigest() == progress['stages'][name]['model_sha256']
            expected.update(fingerprints(attempted))
        observed = fingerprints(actual['model'])
        applied = observed == expected
        if not applied and observed != before:
            raise RuntimeError(f'Accepted geometry differs: {sum((expected-observed).values())} missing, {sum((observed-expected).values())} unexpected leaves; no placement repeated')
        for name in group:
            progress['stages'][name].update(state='accepted' if applied else 'not_applied', reconciled=True, accepted_revision=accepted['base_revision'], pieces=accepted['brick_count'])
            if applied and name.startswith('floor-'):
                progress['published_storeys'] = max(progress.get('published_storeys', 0), int(name.removeprefix('floor-')))
        write(args.output / (spec['key'] + '-' + stage + '.reconciliation.json'),
              {'read_only_mcp': True, 'accepted': accepted, 'exact_leaf_fingerprints_match': True, 'stage_applied': applied})
        write(journal_path, journal)
        print(json.dumps({'key': spec['key'], 'stage': stage, 'state': 'reconciled accepted' if applied else 'confirmed not applied',
                          'pieces': accepted['brick_count'], 'revision': accepted['base_revision']}), flush=True)
    finally:
        mcp.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT / 'target/district-mcp')
    parser.add_argument('--binary', default='ergo-mcp')
    parser.add_argument('--url', default='https://ergosandbox.com')
    parser.add_argument('--token-file', type=Path, default=Path.home() / '.config/ergo/ergo-mcp-token')
    parser.add_argument('--site', action='store_true', help='Author street/bridge assemblies separately from the eleven buildings')
    stage_choices=[*models.STAGES,*district_streets.STAGES]
    selection = parser.add_mutually_exclusive_group()
    selection.add_argument('--stage', choices=stage_choices)
    selection.add_argument('--through', choices=[*stage_choices,'finished'], help='Publish missing applicable stages in order, preserving each assembly')
    parser.add_argument('--only', help='One exact building key; omitted selects all eleven')
    parser.add_argument('--batch-floors', type=int, choices=[1, 2, 3], default=3, help='At most three self-supporting storeys per published edit')
    parser.add_argument('--reconcile', choices=[*stage_choices, 'roof-bearing'], help='Read-only MCP reopen and exact geometry comparison for an uncertain stage; requires --only')
    parser.add_argument('--publish', action='store_true')
    args = parser.parse_args()
    recipes=district_streets if args.site else models
    def selected_stages(spec):
        plan=recipes.stages(spec)
        if args.through=='finished':return plan
        if args.through:return plan[:plan.index(args.through)+1] if args.through in plan else []
        requested=args.stage or ('roads' if args.site else 'foundation')
        return [requested] if requested in plan else []
    binary = shutil.which(args.binary)
    if not binary:
        parser.error('Installed MCP executable is required')
    selected = [spec for spec in recipes.SPECS if not args.only or spec['key'] == args.only]
    if not selected:
        parser.error('Unknown building key')
    if not args.publish and not args.reconcile:
        print(json.dumps({'published': False, 'buildings': [
            {'key': spec['key'], 'name': spec['name'], 'origin': recipes.origin(spec), 'planned_storeys': 0 if args.site else 10,'stages':selected_stages(spec)}
            for spec in selected]}, indent=2))
        return
    if not args.token_file.is_file():
        parser.error('A delegated token file is required')
    os.environ['ERGO_API_URL'] = args.url
    args.output.mkdir(parents=True, exist_ok=True)
    journal_path = args.output / 'progress.json'
    journal = json.loads(journal_path.read_text()) if journal_path.exists() else {
        'format': 'ergo-district-progress-1', 'origin': args.url, 'centre_tower': models.CENTRE_TOWER, 'buildings': {}}
    if journal['origin'] != args.url or journal['centre_tower'] != models.CENTRE_TOWER:
        parser.error('Journal belongs to another district/origin')
    if args.reconcile:
        if not args.only or len(selected) != 1:
            parser.error('Reconciliation requires one exact --only key')
        reconcile(args, binary, selected[0], journal, journal_path)
        return
    for spec in selected:
        plan=recipes.stages(spec)
        stages=selected_stages(spec)
        progress = journal.setdefault('decorations' if args.site else 'buildings',{}).setdefault(spec['key'], {'name': spec['name'], 'stages': {}})
        pending = []
        for stage in stages:
            previous = progress['stages'].get(stage)
            if previous and previous['state'] not in ['accepted', 'not_applied']:
                raise RuntimeError(f"Reconcile {spec['key']} / {stage} before retrying an attempted publication")
            if not previous or previous['state'] == 'not_applied':
                pending.append(stage)
        if not pending:
            print(json.dumps({'key': spec['key'], 'state': 'already accepted', 'assembly_id': progress.get('assembly_id')}), flush=True)
            continue
        workspace = args.output / (spec['key'] + '.cache.json')
        if not workspace.exists():
            if progress.get('assembly_id'):
                raise RuntimeError('Bound workspace missing; reopen its recorded assembly instead of importing again')
            write(workspace, empty_workspace('District ' + ('' if args.site else spec['key'][:2]+' ') + '· ' + spec['name']))
        mcp = transport.Mcp(binary, workspace, args.url, args.token_file)
        try:
            allowance = mcp.call('get_edit_allowance')['structuredContent']
            if not allowance.get('god_mode'):
                raise RuntimeError('This district run requires the authorized god-mode account')
            if not (args.output / 'techniques.json').exists():
                write(args.output / 'techniques.json', mcp.call('get_building_techniques')['structuredContent'])
            if progress.get('assembly_id'):
                # Native startup already reloads a bound cache authoritatively.
                # Reopen only if a missing/different binding needs repair.
                sidecar=workspace.with_suffix('.shared.json')
                binding=json.loads(sidecar.read_text()) if sidecar.exists() else {}
                if binding.get('assembly_id')!=progress['assembly_id']:
                    mcp.call('open_shared_construction', assembly_id=progress['assembly_id'])
            resolved_parts = set()
            for group in batches(pending, args.batch_floors):
                if (args.output / 'stop-requested').exists():
                    print(json.dumps({'state': 'stopped at supported checkpoint'}), flush=True)
                    return
                individual = {}
                for stage in group:
                    prerequisites = plan[:plan.index(stage)]
                    if any(prior not in group and progress['stages'].get(prior, {}).get('state') != 'accepted' for prior in prerequisites):
                        raise RuntimeError('Complete earlier supported stages first')
                    for part in recipes.required_parts(stage):
                        canonical=part+'.dat'
                        if canonical not in resolved_parts:
                            description=mcp.call('get_catalog',part=canonical)['structuredContent']['requested_part']
                            if not description.get('mass_kg') or 'body_bounds' not in description:
                                raise RuntimeError('Use the updated MCP client with sourced nominal profiles for '+canonical)
                            register_catalog_part(part,description)
                            write(args.output/('resolved-'+part+'.json'),description)
                            resolved_parts.add(canonical)
                    individual[stage] = recipes.stage(spec, stage).export(recipes.origin(spec))
                    write(args.output / (spec['key'] + '-' + stage + '.model.json'), individual[stage])
                model = combine(individual)
                if len(json.dumps({'name': 'place_submodel', 'arguments': {'model': model}}).encode()) > 1_000_000:
                    raise RuntimeError('Reduce --batch-floors to stay within the MCP input budget')
                parts = {instance['target']['id'] for definition in model['definitions'].values()
                         for instance in definition if instance['target']['kind'] == 'part'}
                for part in sorted(parts - resolved_parts):
                    resolved = mcp.call('get_catalog', part=part)['structuredContent']['requested_part']
                    if resolved['quality'] == 'proxy':
                        raise RuntimeError('Unresolved catalogue geometry: ' + part)
                resolved_parts.update(parts)
                for stage in group:
                    operation: dict[str, object] = {'state': 'attempted', 'tool': 'place_submodel', 'batch_stages': group,
                        'model_sha256': hashlib.sha256(json.dumps(individual[stage], sort_keys=True).encode()).hexdigest()}
                    previous = progress['stages'].get(stage)
                    if previous:
                        operation['previous_attempts'] = [*previous.get('previous_attempts', []), previous]
                        prior_receipt = args.output / (spec['key'] + '-' + stage + '.receipt.json')
                        if prior_receipt.exists():
                            write(prior_receipt.with_name(prior_receipt.stem + '-before-repair.json'), json.loads(prior_receipt.read_text()))
                    progress['stages'][stage] = operation
                write(journal_path, journal)
                result = mcp.rpc('tools/call', {'name': 'place_submodel', 'arguments': {'model': model}})
                for stage in group:
                    write(args.output / (spec['key'] + '-' + stage + '.receipt.json'), result)
                if result.get('isError'):
                    raise RuntimeError('MCP rejected or could not confirm the stage; inspect its saved receipt')
                receipt = result['structuredContent']['shared_commit']
                if not receipt.get('published'):
                    raise RuntimeError('MCP did not confirm shared publication')
                if progress.get('assembly_id') and receipt['assembly_id'] != progress['assembly_id']:
                    raise RuntimeError('Publication changed the assembly identity')
                progress['assembly_id'] = receipt['assembly_id']
                for stage in group:
                    progress['stages'][stage].update(state='accepted', accepted_revision=receipt['accepted_revision'], pieces=len(receipt['pieces']))
                    if stage.startswith('floor-'):
                        progress['published_storeys'] = int(stage.removeprefix('floor-'))
                write(journal_path, journal)
                print(json.dumps({'key': spec['key'], 'stages': group, 'state': 'accepted', 'pieces': len(receipt['pieces']),
                                  'assembly_id': receipt['assembly_id'], 'revision': receipt['accepted_revision']}), flush=True)
        finally:
            mcp.close()


if __name__ == '__main__':
    main()
