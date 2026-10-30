#!/usr/bin/env python3
"""Actual WASM checks for offline canonical retrofits and common car instances."""
import argparse,json,math,os,pathlib,tempfile
from mcp_client import Mcp

ROOT=pathlib.Path(__file__).resolve().parents[1]
parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--binary',type=pathlib.Path,default=ROOT/'target/debug/ergo-mcp')
args=parser.parse_args();BINARY=str(args.binary.resolve())
os.environ['ERGO_ASSET_FETCH']='0'

def legacy_box(part,width,depth,height):
    lo=[-width/2,-depth/2,-height];hi=[width/2,depth/2,0]
    p=[[x,y,z] for z in [lo[2],hi[2]] for y in [lo[1],hi[1]] for x in [lo[0],hi[0]]]
    vertices=[]
    for corners,normal in [([0,2,3,1],[0,0,-1]),([4,5,7,6],[0,0,1]),([0,1,5,4],[0,-1,0]),
                           ([2,6,7,3],[0,1,0]),([0,4,6,2],[-1,0,0]),([1,3,7,5],[1,0,0])]:
        for index in [0,1,2,0,2,3]:vertices.extend([*p[corners[index]],*normal,-1,-1,-1])
    return {'schema':2,'part':part,'quality':'parametric','vertices':vertices,
        'bounds':{'min':lo,'max':hi},'body_bounds':{'min':lo,'max':hi},'credits':[]}

def placed(kind,id,position,color):
    return {'target':{'kind':kind,'id':id},'color':color,
        'transform':[[1,0,0,position[0]],[0,1,0,position[1]],[0,0,1,position[2]],[0,0,0,1]]}

with tempfile.TemporaryDirectory(prefix='ergo-library-') as temporary:
    directory=pathlib.Path(temporary);workspace=directory/'legacy.ergo.json'
    old_alias='readback-variants/fixture/3003.dat'
    document={'format':'ergo-construction-1','schema':2,'name':'Legacy retrofit fixture',
        'geo':{'latitude':48.8566,'longitude':2.3522,'arcseconds_per_block':1},
        'model':{'format':'ergo-model-1','schema':2,'root':'main','definitions':{'main':[
            placed('part','3023.dat',[1,.5,1/3],4),placed('part',old_alias,[5,1,1],1)]}},
        'pieces':[{'id':41,'rgb':0x112233,'step':2},{'id':99,'rgb':0x123456,'step':3}],
        'meshes':{'3023.dat':legacy_box('3023.dat',2,1,1/3),old_alias:legacy_box(old_alias,2,2,1)},
        'joints':[],'fixed_bricks':[],'groups':[],'ground':[]}
    workspace.write_text(json.dumps(document))
    client=Mcp(BINARY,workspace,None,None)
    try:
        before=client.call('list_bricks')['structuredContent']['bricks']
        result=client.call('retrofit_library')['structuredContent'];assert result['piece_ids_preserved']
        after=client.call('list_bricks')['structuredContent']['bricks']
        assert {piece['id'] for piece in after}=={41,99}
        for original,updated in zip(before,after):
            assert original['position']==updated['position'] and original['bounds']==updated['bounds']
        assert {piece['part'] for piece in after}=={'3023b.dat','3003.dat'}
        client.call('export_construction');accepted=json.loads(workspace.read_text())
        assert accepted['pieces']==document['pieces']
        assert set(accepted['meshes'])=={'3023b.dat','3003.dat'}
        assert all(mesh['quality']=='source' and mesh['credits'] for mesh in accepted['meshes'].values())
    finally:client.close()

    workspace=directory/'cars.ergo.json';client=Mcp(BINARY,workspace,None,None)
    try:
        catalog=client.call('get_catalog')['structuredContent']['catalog']
        assert {entry['id'] for entry in catalog['constructs']}=={'minifigure-v1','classic-car-v1'}
        road=[placed('part','2431.dat',[offset+x+2,n+.5,1/3],72) for offset in [0,16] for x in [0,4] for n in range(4)]
        model={'format':'ergo-model-1','schema':2,'root':'main','definitions':{'main':[
            *road,placed('construct','classic-car-v1',[0,0,1/3],4),placed('construct','classic-car-v1',[16,0,1/3],1)]}}
        feedback=client.call('place_submodel',model=model)['structuredContent']
        assert feedback['brick_count']==58 and feedback['publication_ready'],feedback
        client.call('export_construction');accepted=json.loads(workspace.read_text())
        assert len(accepted['pieces'])==58
        assert all(mesh['quality']=='source' for mesh in accepted['meshes'].values())
        assert sum(item['target']['kind']=='construct' for items in accepted['model']['definitions'].values() for item in items)>=2
        before=client.call('list_bricks',limit=200)['structuredContent']['bricks']
        model['definitions']['main'][len(road)]['target']['id']='classic-car-v999'
        rejected=client.rpc('tools/call',{'name':'place_submodel','arguments':{'model':model}})
        assert rejected['isError']
        after=client.call('list_bricks',limit=200)['structuredContent']['bricks']
        assert len(after)==len(before)
        for old,new in zip(before,after):
            assert all(old[field]==new[field] for field in ['id','part','color','mass_kg'])
            assert old['position']==new['position']
            assert all(math.isclose(old['bounds'][edge][axis],new['bounds'][edge][axis],abs_tol=1e-12,rel_tol=0) for edge in ['min','max'] for axis in range(3))
    finally:client.close()
print('Canonical library WASM checks passed: offline source assets, legacy alias/proxy retrofit, stable IDs/poses/metadata, shared car references and unknown-version rollback.')
