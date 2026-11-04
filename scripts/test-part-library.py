#!/usr/bin/env python3
"""Actual WASM checks for current-only offline canonical parts and shared constructs."""
import argparse,base64,json,math,os,pathlib,tempfile
from mcp_client import Mcp

ROOT=pathlib.Path(__file__).resolve().parents[1]
parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--binary',type=pathlib.Path,default=ROOT/'target/debug/ergo-mcp')
parser.add_argument('--artifacts',type=pathlib.Path,help='Optional directory for actual plate-free car/guard WebGL captures')
args=parser.parse_args();BINARY=str(args.binary.resolve())
os.environ['ERGO_ASSET_FETCH']='0'

def placed(kind,id,position,color):
    return {'target':{'kind':kind,'id':id},'color':color,
        'transform':[[1,0,0,position[0]],[0,1,0,position[1]],[0,0,1,position[2]],[0,0,0,1]]}

with tempfile.TemporaryDirectory(prefix='ergo-library-') as temporary:
    directory=pathlib.Path(temporary);workspace=directory/'current-only.ergo.json'
    client=Mcp(BINARY,workspace,None,None)
    try:
        assert 'retrofit_library' not in {tool['name'] for tool in client.rpc('tools/list',{})['tools']}
        for retired in ['minifigure-v1','classic-car-v1','medieval-guard-v1','medieval-guard-v2']:
            model={'format':'ergo-model-1','schema':2,'root':'main','definitions':{'main':[placed('construct',retired,[0,0,0],4)]}}
            result=client.rpc('tools/call',{'name':'place_submodel','arguments':{'model':model}})
            assert result['isError'],retired
        assert not client.call('list_bricks')['structuredContent']['bricks']
    finally:client.close()

    workspace=directory/'cars.ergo.json';client=Mcp(BINARY,workspace,None,None)
    try:
        catalog=client.call('get_catalog')['structuredContent']['catalog']
        assert {entry['id'] for entry in catalog['constructs']}=={'minifigure-v2','classic-car-v2','medieval-guard-v3','medieval-archer-v1','castle-standard-v1'}
        road=[]
        model={'format':'ergo-model-1','schema':2,'root':'main','definitions':{'main':[
            *road,placed('construct','classic-car-v2',[0,0,0],4),placed('construct','classic-car-v2',[16,0,0],1)]}}
        feedback=client.call('place_submodel',model=model)['structuredContent']
        assert feedback['brick_count']==54 and feedback['overlap_free'] and feedback['static_under_current_joint_model'],feedback
        assert not feedback['publication_ready'] and feedback['calibration_issue'],feedback
        client.call('export_construction');accepted=json.loads(workspace.read_text())
        assert len(accepted['pieces'])==54
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

    workspace=directory/'castle.ergo.json';client=Mcp(BINARY,workspace,None,None)
    try:
        model={'format':'ergo-model-1','schema':2,'root':'main','definitions':{'main':[
            placed('part','4201.dat',[4,4,1],71),
            placed('construct','minifigure-v2',[1,1,1],4),
            placed('construct','castle-standard-v1',[6.5,2.5,1],1)]}}
        feedback=client.call('place_submodel',model=model)['structuredContent']
        assert feedback['brick_count']==16 and feedback['publication_ready'],feedback
        assert feedback['stability_report']['joint_strength_required'] is False
        before=client.call('list_bricks',limit=100)['structuredContent']['bricks']
        assert {'76382p00.dat','3957a.dat','4495a.dat'} <= {b['part'] for b in before}
        flag=next(b for b in before if b['part']=='4495a.dat')
        replacement={'format':'ergo-model-1','schema':2,'root':'flag','definitions':{'flag':[
            placed('part','4495a.dat',[flag['position'][0]+.01,*flag['position'][1:]],1)]}}
        rejected=client.rpc('tools/call',{'name':'place_submodel','arguments':{
            'model':replacement,'replace_ids':[flag['id']]}})
        assert rejected['isError']
        after=client.call('list_bricks',limit=100)['structuredContent']['bricks']
        assert after==before,'Incorrect flag-bore fit changed the working copy'
        client.call('export_construction');accepted=json.loads(workspace.read_text())
        assert all(mesh['quality']=='source' and mesh['credits'] for mesh in accepted['meshes'].values())
        support=[b['id'] for b in before if math.isclose(b['position'][0],6.5) and math.isclose(b['position'][1],4.5)]
        assert len(support)==6
        feedback=client.call('remove_bricks',ids=support)['structuredContent']
        assert not feedback['publication_ready'],'Unsupported flag gained invented attachment capacity'
    finally:client.close()
    workspace=directory/'plate-free.ergo.json';client=Mcp(BINARY,workspace,None,None)
    try:
        model={'format':'ergo-model-1','schema':2,'root':'main','definitions':{'main':[
            placed('construct','classic-car-v2',[0,0,0],4),placed('construct','minifigure-v2',[14,4,0],1)]}}
        feedback=client.call('place_submodel',model=model)['structuredContent']
        assert feedback['brick_count']==31 and feedback['overlap_free'] and feedback['static_under_current_joint_model'],feedback
        assert not feedback['publication_ready'] and feedback['calibration_issue'],feedback
        bricks=client.call('list_bricks',limit=100)['structuredContent']['bricks']
        grounded=[b for b in bricks if abs(b['bounds']['min'][2])<1e-6]
        assert len(grounded)==5 and all(b['part'] in ['3815c01.dat','4624c01.dat'] for b in grounded),grounded
        for holder in [b for b in bricks if b['part']=='4600.dat']:
            detail=client.call('inspect_brick',id=holder['id'])['structuredContent']
            assert sum(c['kind']=='wheel pin / bearing connection' for c in detail['contacts'])==2,detail
        if args.artifacts:
            args.artifacts.mkdir(parents=True,exist_ok=True)
            for name,eye,target in [('car',[11,-10,8],[4,2,2]),('guard',[21,-5,7],[16,4,2.3])]:
                image=client.call('test_visual_appearance',width=900,height=650,lighting='studio',
                    camera={'position_world':eye,'target_world':target,'vertical_fov_degrees':35})
                png=next(item['data'] for item in image['content'] if item['type']=='image')
                (args.artifacts/f'plate-free-{name}.png').write_bytes(base64.b64decode(png))
    finally:client.close()
    workspace=directory/'reposed-garrison.ergo.json';client=Mcp(BINARY,workspace,None,None)
    try:
        model={'format':'ergo-model-1','schema':2,'root':'main','definitions':{'main':[
            placed('construct','medieval-guard-v3',[0,0,0],4),placed('construct','medieval-archer-v1',[6,0,0],4)]}}
        feedback=client.call('place_submodel',model=model)['structuredContent']
        assert feedback['brick_count']==19 and feedback['overlap_free'] and feedback['static_under_current_joint_model'],feedback
        assert not feedback['publication_ready'] and feedback['calibration_issue'],feedback
        bricks=client.call('list_bricks',limit=100)['structuredContent']['bricks']
        assert sum(b['part']=='4499.dat' for b in bricks)==1
        grips=0
        for hand in [b for b in bricks if b['part']=='3820.dat']:
            detail=client.call('inspect_brick',id=hand['id'])['structuredContent']
            assert sum(c['kind']=='minifigure wrist connection' for c in detail['contacts'])==1,detail
            grips+=sum(c['kind']=='minifigure grip / bar connection' for c in detail['contacts'])
        assert grips==3,'Spear, shield and bow must connect to the actual individual hands'
        if args.artifacts:
            args.artifacts.mkdir(parents=True,exist_ok=True)
            for name,eye,target in [('reposed-guard',[7,-9,6],[2,1.5,3]),('archer',[13,-9,6],[8,1.5,3])]:
                image=client.call('test_visual_appearance',width=900,height=700,lighting='studio',
                    camera={'position_world':eye,'target_world':target,'vertical_fov_degrees':35})
                png=next(item['data'] for item in image['content'] if item['type']=='image')
                (args.artifacts/f'{name}.png').write_bytes(base64.b64decode(png))
    finally:client.close()
    workspace=directory/'scenic-garrison.ergo.json';client=Mcp(BINARY,workspace,None,None)
    try:
        references=[]
        for index,(cape,helmet,lift) in enumerate([('4524.dat','3896.dat',2/24),('50231c01.dat','89520.dat',.6/24),(None,'3844.dat',0)]):
            x=index*10
            parts=[('3815c01.dat',[x+2,1.5,5/3],0),('76382p4f.dat',[x+2,1.5,3],1),
                   ('3626ap01.dat',[x+2,1.5,4+lift],14),(helmet,[x+2,1.5,4+lift],72)]
            if cape:parts.append((cape,[x+2,1.5,3+(2/24 if cape=='4524.dat' else 0)],4))
            references.extend(placed('part',part,position,colour) for part,position,colour in parts)
        references.extend(placed('part',part,position,colour) for part,position,colour in [
            ('41539.dat',[34,1,1/3],288),('41539.dat',[42,1,1/3],288),
            ('41539.dat',[54,6,1/3],288),('41539.dat',[70,6,1/3],288),
            ('3470.dat',[34,1,2/3],2),('3471.dat',[42,1,2/3],2),
            ('4493c01.dat',[54,5.75,65/24],0),('4493c02.dat',[70,5.75,65/24],15),
            ('4491a.dat',[70,5.75,73/24],4)])
        # A genuine stepped wall panel sits directly on ordinary floor studs.
        # Its visual wings must not create a fictional solid five-stud foot.
        references.extend(placed('part',part,position,colour) for part,position,colour in [
            ('41539.dat',[86,6,1/3],288),('4444.dat',[86.5,5.5,6+1/3],71)])
        model={'format':'ergo-model-1','schema':2,'root':'main','definitions':{'main':references}}
        feedback=client.call('place_submodel',model=model)['structuredContent']
        assert feedback['brick_count']==25 and feedback['overlap_free'],feedback
        assert feedback['static_under_current_joint_model'] and feedback['publication_ready'],feedback
        client.call('export_construction');accepted=json.loads(workspace.read_text())
        assert all(mesh['quality']=='source' for mesh in accepted['meshes'].values())
    finally:client.close()
print('Canonical library WASM checks passed: current-only offline source assets, retired-version rejection, genuine fits, plate-free figures/cars, sourced trees, capes and horses without invented capacities.')
