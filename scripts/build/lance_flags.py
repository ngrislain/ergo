#!/usr/bin/env python3
"""Genuine two-colour lance-standard prototype; no implicit publication or invented grip strength."""
import argparse,json,pathlib,sys
ROOT=pathlib.Path(__file__).resolve().parents[2];sys.path.insert(0,str(ROOT))
from scripts.native_model import Model,instance,east_rotation,yaw,register_catalog_part
from scripts.build.paris_landmarks import GEO
from scripts.mcp_client import Mcp
from scripts.build.castle_refinements import Editor, multiply, translation
from scripts.build import castle_landscape as castle
from scripts.native_model import model_leaves

def standard(upper=1,lower=14,bearing_pinnacles=True,stone=19):
    model=Model();model.part('3003',stone,0,0,0);model.part('3068b',stone,0,0,1)
    model.definitions['main'].append(instance('part','3849.dat',[.5,.5,1+1/3+26/24],0,east_rotation(-__import__('math').pi/2)))
    for height,color,angle in [(7+1/6,lower,0),(8+1/6,upper,__import__('math').pi)]:
        model.definitions['main'].append(instance('part','4495a.dat',[.5,.5,height],color,yaw(angle)))
    if bearing_pinnacles:
        # As in the accepted standards, visible stonework actually bears under
        # the asymmetric cloth. Opposed pennants let the upper support miss the
        # lower flag; no imaginary clutch moment or hidden weld carries it.
        for n,height in [(2,6),(-2,7)]:
            for h in range(height):model.part('3062b',stone,0,n,h)
            model.part('3070b',stone,0,n,height)
    return model

def refine_castle(source,assets):
    """Replace only exactly matched current standards, retaining unrelated IDs."""
    if source['assembly_id']!=castle.CASTLE or source['external_dependencies']:
        raise ValueError('A complete current accepted castle workspace is required')
    editor=Editor(source['scene'])
    flags=[b for b in editor.scene['bricks'] if b['part']=='4495a.dat']
    if len(flags)!=16:raise ValueError('Expected sixteen accepted castle standards; inspect before editing')
    for flag in flags:
        frame=castle.brick_reference(flag)['transform']
        frame=multiply(frame,translation(z=-29/6))
        old=Model();old.construct('castle-standard-v1',color=flag['color'])
        # Exactly matching every original part prevents a partial inventory
        # from deleting unrelated architecture or a modified standard.
        before=Model()
        for leaf in model_leaves(old.export()):
            before.definitions['main'].append(dict(target=dict(kind='part',id=leaf['part']),color=leaf['color'],transform=multiply(frame,leaf['transform'])))
        editor.replace_model(before.export(),Model().export())
        editor.add_model(standard(stone=71).export(),multiply(frame,translation(-.5,-.5)))
    scene=editor.scene
    kept=[b for b in scene['bricks'] if b['id'] not in editor.removed]
    refs=[castle.brick_reference(b) for b in kept]
    pieces=[dict(id=b['id'],rgb=b['rgb'],step=b['step']) for b in kept]
    palette={b['color']:b['rgb'] for b in scene['bricks']}
    next_id=max(b['id'] for b in scene['bricks'])+1
    for leaf in model_leaves(editor.model.export()):
        refs.append(dict(target=dict(kind='part',id=leaf['part']),color=leaf['color'],transform=leaf['transform']))
        pieces.append(dict(id=next_id,rgb=palette[leaf['color']],step=0));next_id+=1
    # Retained canonical meshes may carry frozen physical metadata. Extra assets
    # fill missing part IDs only; they never replace accepted source records.
    meshes={**assets,**scene['meshes']}
    needed={ref['target']['id'] for ref in refs}
    if needed-meshes.keys():raise ValueError('Current source assets missing: '+str(sorted(needed-meshes.keys())))
    if (any(j['a'] in editor.removed or j['b'] in editor.removed for j in scene['joints'])
        or any(id in editor.removed for id in scene['fixed_bricks']) or scene['groups']):
        raise ValueError('Attached, anchored or grouped standards require an explicit metadata-preserving edit')
    document=castle.native(scene,refs,pieces,{part:meshes[part] for part in needed},scene['name'])
    document.update(joints=scene['joints'],fixed_bricks=scene['fixed_bricks'],groups=scene['groups'],ground=scene['ground'])
    return document,dict(published=False,assembly_id=source['assembly_id'],base_revision=source['base_revision'],
        standards_replaced=len(flags),removed_occurrences=len(editor.removed),added_occurrences=next_id-max(b['id'] for b in scene['bricks'])-1,
        retained_unrelated_ids=len(kept),pieces=len(pieces),flag_colours=[1,14],genuine_lance='3849.dat')

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--output',type=pathlib.Path,required=True);parser.add_argument('--assets-from',type=pathlib.Path,required=True)
    parser.add_argument('--castle-workspace',type=pathlib.Path,help='Complete accepted castle workspace for an identity-preserving local proposal')
    args=parser.parse_args();args.output.mkdir(parents=True,exist_ok=False)
    parts=['3003.dat','3068b.dat','3849.dat','4495a.dat','3062b.dat','3070b.dat'];assets=json.loads(args.assets_from.read_text())['meshes']
    missing=set(parts)-assets.keys()
    if missing:
        extra=args.output/'source-assets.native.json';client=Mcp(str(ROOT/'target/debug/ergo-mcp'),extra,None,None)
        try:
            for part in sorted(missing):client.call('get_catalog',part=part)
            client.call('place_brick',part='3003',color=19,min_corner=[0,0,0]);client.call('export_construction')
        finally:client.close()
        assets.update(json.loads(extra.read_text())['meshes'])
    for part in parts:register_catalog_part(part,assets[part])
    document=dict(format='ergo-construction-1',schema=2,name='Deux couleurs sur lance de tournoi — prototype',geo=GEO,
        model=standard().export(),pieces=[],meshes={part:assets[part] for part in parts},joints=[],fixed_bricks=[],groups=[],ground=[])
    report=dict(published=False,genuine_lance='3849.dat',flag_parts=['4495a.dat','4495a.dat'],flag_colours=[1,14],
        interpretation='Opposed blue/yellow genuine pennants on the long jousting lance, with visible stone bearing pinnacles; no changed source geometry',
        grip_capacity_not_assumed=True,bearing_pinnacles=True,publication_requires_validated_support=True)
    if args.castle_workspace:
        document,report=refine_castle(json.loads(args.castle_workspace.read_text()),assets)
    (args.output/'construction.native.json').write_text(json.dumps(document,separators=(',',':'))+'\n')
    (args.output/'proposal.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report),flush=True)

if __name__=='__main__':main()
