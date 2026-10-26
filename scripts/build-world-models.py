#!/usr/bin/env python3
"""Generate native world models and inspect them via the real Ergo MCP client."""
import argparse
import base64
import importlib.util
import json
import math
from pathlib import Path
import shutil

from native_model import Model,bounds,local_bounds,multiply,north_rotation,yaw

ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location("demo",ROOT/"scripts/demo-mcp.py")
assert spec and spec.loader
demo=importlib.util.module_from_spec(spec);spec.loader.exec_module(demo)


def chalet(wall,timber,roof,shutter):
    model=Model()
    for layer in range(12):
        z=layer/3;color=wall if layer<6 else timber
        model.strip(8,color,0,5,z)
        for x in [0,7]:model.strip(4,color,x,1,z,turn=True)
        for x in range(8):
            front=color
            if x in [3,4] and layer<6:front=70
            if x in [1,2,5,6] and layer in [3,4,5,8,9,10]:front=321
            if x in [0,7] and layer in [3,4,5,8,9,10]:front=shutter
            model.part("3070b",front,x,0,z)
    # Two topmost rigid pitched slabs: no assumed stud clutch above either slab.
    angle=math.radians(25)
    left=multiply(north_rotation(-angle),yaw(math.pi/2))
    right=multiply(north_rotation(angle),yaw(math.pi/2))
    lo,hi=local_bounds("3035",left);span=hi[0]-lo[0]
    model.part("3035",roof,4-span,-1,4,rotation=left)
    model.part("3035",roof,4,-1,4,rotation=right)
    # Supported balcony deck and independent grounded balustrade.
    for x in [1,5]:model.column("3068b",70,x,-2,7)
    for x in [0,4]:model.part("3020",70,x,-2,7/3)
    for x in range(8):model.column("3070b",70,x,-3,8)
    for x in [1,2,5,6]:
        model.part("3070b",2,x,-3,8/3)
        model.part("3070b",4 if x%2 else 14,x,-3,3)
    return model


def landscape():
    model=Model()
    for n in [11,13,21,23]:model.floor(50,2,71,-1,n,0)
    model.floor(22,6,7,13,15,0)
    for x in [4,18,32,46]:
        for n in [9,34]:model.floor(4,2,28,x,n,0)
    for x in [-5,9,23,37,51]:
        for n in [5,29]:
            model.column("3068b",70,x,n,3)
            for dx in [-1,1]:
                for dn in [-1,1]:model.part("3068b",288,x+dx,n+dn,1)
            model.column("3068b",2,x,n,2,4/3)
            model.column("3070b",288,x+.5,n+.5,2,2)
    # A low fountain and planter borders in the public square.
    model.floor(6,4,321,21,16,1/3)
    for n in [15,20]:
        for h in range(2):model.strip(8,71,20,n,1/3+h/3)
    for x in [20,27]:
        for h in range(2):model.strip(4,71,x,16,1/3+h/3,turn=True)
    model.column("3068b",71,23,17,3,2/3)
    model.column("3070b",321,23.5,17.5,2,5/3)
    return model


def village_layout():
    model=Model()
    styles=[(15,19,320,288),(19,70,72,4),(15,70,70,272)]
    for index,style in enumerate(styles):model.definitions[f'house-{index}']=chalet(*style).definitions['main']
    for row,north in enumerate([0,28]):
        for column,x in enumerate([0,14,28,42]):
            model.reference(f'house-{(column+row)%3}',x+(8 if row==0 else 0),north+(6 if row==0 else 0),0,
                            rotation=yaw(math.pi if row==0 else 0))
    model.definitions['main'].extend(landscape().definitions['main'])
    return model


def palace():
    model=Model()
    # Native tile layers form gravity-supported stone rather than guessed bonds.
    model.floor(64,48,19,0,0,0)
    for layer in range(15):
        z=1/3+layer/3;color=14 if layer in [1,12] else 19
        model.strip(64,color,0,47,z)
        for x in [0,63]:model.strip(46,color,x,1,z,turn=True)
        for x,length in [(0,26),(38,26)]:model.strip(length,color,x,0,z)
    # Recessed blue channels and repeated courtyard columns.
    for x in [20,42]:model.floor(2,16,272,x,8,1/3)
    for x in [22,28,34,40]:
        for n in [12,20,24]:
            for layer in range(21):model.part("3068b",14 if layer in [0,1,17,18] else 19,x,n,1/3+layer/3)
            model.part("3031",14,x-1,n-1,22/3)
    # Raised central sanctuary and grand twelve-step stairway.
    for layer in range(6):model.floor(20,12,19,22,34,1/3+layer/3)
    for step in range(1,7):
        for layer in range(step):model.floor(12,1,14 if layer==step-1 else 19,26,27+step,1/3+layer/3)
    # Maze wings: continuous stone columns with real walkable openings.
    for base in [3,47]:
        walls=[(base,5,13,False),(base,10,9,False),(base+4,15,9,False),
               (base,20,9,False),(base+4,25,9,False),(base,30,13,False),
               (base+12,6,8,True),(base,11,8,True),(base+12,16,8,True),
               (base,21,8,True),(base+6,31,4,True)]
        for x,n,length,turn in walls:
            for layer in range(12):model.strip(length,19 if layer<10 else 14,x,n,1/3+layer/3,turn)
    # Concealed rear chambers and three alternating entrances.
    for base in [3,47]:
        for layer in range(12):
            z=1/3+layer/3
            model.strip(12,19,base,45,z)
            model.strip(9,19,base,36,z,turn=True)
            model.strip(9,19,base+11,36,z,turn=True)
            model.strip(6,19,base+1,36,z)
    # Two raised trap chambers have recessed wells and low dark teeth.
    for base in [4,48]:
        for layer in range(6):
            for dn in range(6):
                if dn in [2,3]:
                    for dx in [0,6]:model.strip(2,19,base+dx,38+dn,1/3+layer/3)
                else:model.strip(8,19,base,38+dn,1/3+layer/3)
        for dx in [2,4]:
            for dn in [2,3]:model.column("3070b",0,base+dx,38+dn,2,1/3)
    # Portal pylons, obelisks and blue/gold facade decoration.
    for base in [23,36]:
        for layer in range(18):model.floor(5,3,14 if layer in [2,14] else 19,base,2,1/3+layer/3)
    for x,n in [(18,3),(44,3),(18,32),(44,32)]:
        model.column("3068b",19,x,n,18,1/3)
        model.part("3068b",14,x,n,19/3)
    for x in [24,30,36]:
        for n in [36,42]:
            model.column("3068b",19,x,n,18,7/3)
            model.part("3031",14,x-1,n-1,25/3)
    return model


def run_model(binary,output,name,model,origin,token_file=None,publish=False):
    output.mkdir(parents=True,exist_ok=True);workspace=output/"construction.ergo.json"
    if workspace.exists():raise RuntimeError("Choose a fresh output directory: "+str(output))
    native=model.export(tuple(origin));(output/"model.ergo-model.json").write_text(json.dumps(native,separators=(',',':'))+'\n')
    mcp=demo.Mcp(binary,workspace,'https://ergosandbox.com',None)
    try:
        mcp.call('place_submodel',model=native)
        validation=mcp.call('validate_construction')['structuredContent']
        lo,hi=bounds(native);target=[(a+b)/2 for a,b in zip(lo,hi)];span=max(b-a for a,b in zip(lo,hi))
        camera={'position_world':[target[0]+span*.9,target[1]-span*1.2,target[2]+span*.8],
                'target_world':target,'vertical_fov_degrees':42}
        image=mcp.call('test_visual_appearance',camera=camera,width=1024,height=768,lighting='studio')
        picture=next(c for c in image['content'] if c['type']=='image')
        (output/'preview.png').write_bytes(base64.b64decode(picture['data']))
        report={'name':name,'validation':validation,'origin':origin,'shared_committed':False}
        (output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    finally:mcp.close()
    if publish:
        if not validation['publication_ready']:raise RuntimeError('Not publishable: '+json.dumps(validation))
        mcp=demo.Mcp(binary,workspace,'https://ergosandbox.com',token_file)
        try:
            current=mcp.call('validate_construction')['structuredContent']
            report['publication_attempted']=True;(output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
            receipt=mcp.call('publish_construction',name=name,expected_local_revision=current['revision'])['structuredContent']
            report['publication']=receipt;report['shared_committed']=receipt.get('published') is True
            (output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
            if not report['shared_committed']:raise RuntimeError('Shared publication was not confirmed')
            readback=mcp.call('open_shared_construction',assembly_id=receipt['assembly_id'])['structuredContent']
            report['readback']=readback
            (output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
            if not readback['external_dependencies']:
                assert readback['brick_count']==validation['brick_count'],'Accepted piece count differs'
            accepted=mcp.call('test_visual_appearance',camera=camera,width=1024,height=768,lighting='studio')
            picture=next(c for c in accepted['content'] if c['type']=='image')
            (output/'accepted-preview.png').write_bytes(base64.b64decode(picture['data']))
        finally:mcp.close()
    print(json.dumps({'name':name,'pieces':validation['brick_count'],'ready':validation['publication_ready'],
                      'failure':validation.get('failure'),'calibration':validation.get('calibration_issue'),
                      'shared_committed':report['shared_committed'],'output':str(output)}))
    return report


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('kind',choices=['village','palace','chalet'])
    parser.add_argument('--binary',default='ergo-mcp');parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--origin',nargs=3,type=float,default=[0,0,0])
    parser.add_argument('--publish',action='store_true');parser.add_argument('--token-file',type=Path)
    args=parser.parse_args();binary=shutil.which(args.binary)
    if not binary:parser.error('Install the released client or pass --binary')
    if args.publish and not args.token_file:parser.error('--publish requires --token-file')
    if args.kind=='palace':run_model(binary,args.output,'Palace of the Hidden Sun',palace(),args.origin,args.token_file,args.publish)
    elif args.kind=='chalet':run_model(binary,args.output,'Alpine chalet',chalet(15,70,320,288),args.origin,args.token_file,args.publish)
    else:
        layout=run_model(binary,args.output/'layout-preflight','Alpine village layout preflight',village_layout(),args.origin)
        if not layout['validation']['publication_ready']:raise RuntimeError('Combined village layout is not publishable')
        styles=[(15,19,320,288),(19,70,72,4),(15,70,70,272)]
        for row,north in enumerate([0,28]):
            for column,x in enumerate([0,14,28,42]):
                model=chalet(*styles[(column+row)%3])
                if row==0:
                    placed=Model();placed.definitions={**{('house' if k=='main' else k):v for k,v in model.definitions.items()},'main':[]}
                    placed.reference('house',8,6,0,rotation=yaw(math.pi));model=placed
                origin=[args.origin[0]+x,args.origin[1]+north,args.origin[2]]
                run_model(binary,args.output/f'chalet-{row*4+column+1}',f'Alpine chalet {row*4+column+1}',model,origin,args.token_file,args.publish)
        run_model(binary,args.output/'square','Alpine village square',landscape(),args.origin,args.token_file,args.publish)


if __name__=='__main__':main()
