#!/usr/bin/env python3
"""Generate native world models and inspect them via the real Ergo MCP client."""
import argparse
import base64
import importlib.util
import json
import math
from pathlib import Path
import shutil
import chalet_model

from native_model import Model,bounds,east_rotation,local_bounds,multiply,north_rotation,yaw,snap_origin

ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location("demo",ROOT/"scripts/demo-mcp.py")
assert spec and spec.loader
demo=importlib.util.module_from_spec(spec);spec.loader.exec_module(demo)


def chalet(wall,timber,roof,shutter):
    return chalet_model.chalet(wall,timber,roof,shutter)


def landscape():
    return chalet_model.landscape()


def village_layout():
    model=Model()
    styles=[(15,19,320,288),(19,70,72,4),(15,70,70,272)]
    for index,style in enumerate(styles):model.definitions[f'house-{index}']=chalet(*style).definitions['main']
    for row,north in enumerate([0,56]):
        for column,x in enumerate([0,40,80,120]):
            model.reference(f'house-{(column+row)%3}',x+(24 if row==0 else 0),north+(16 if row==0 else 0),0,
                            rotation=yaw(math.pi if row==0 else 0))
    model.definitions['main'].extend(landscape().definitions['main'])
    return model


def palace_small():
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


PALACE_BRICK_COURSES=9
PALACE_CLEAR_HEIGHT=PALACE_BRICK_COURSES
PALACE_FLOOR_HEIGHT=2/3
PALACE_LEVELS=[1/3+level*(PALACE_CLEAR_HEIGHT+PALACE_FLOOR_HEIGHT) for level in range(3)]


def palace_walk_route(origin):
    origin=snap_origin(origin)
    ground,first,second=PALACE_LEVELS
    points=[(28,6,ground,'ground hall'),(28,38,first,'first-floor landing'),
            (20,38,first,'first-floor west gallery'),(36,38,first,'first-floor doorway gallery'),
            (36,6,first,'first-floor front balcony'),(28,6,first,'second-flight entrance'),(28,38,second,'second-floor landing'),
            (44,38,second,'second-floor hall'),(28,38,second,'return upper stairs'),
            (28,6,first,'descend to first floor'),(36,6,first,'return doorway gallery'),
            (36,38,first,'return rear gallery'),(28,38,first,'return lower stairs'),
            (28,6,ground,'descend to ground hall'),(28,-12,0,'exit palace')]
    return {'format':'ergo-walk-route-1','schema':2,'origin_world':origin,'start':[28,-12,0],
            'eye_height':4,'min_clearance':7,'speed':6,
            'waypoints':[{'name':name,'feet':[x,north,height],'min_clearance':9} for x,north,height,name in points]}


def palace():
    """64x64, three storeys of nine upright brick courses, with real stairs.

    Reference: LEGO 5988, The Temple of Anubis (Henrik Rubin Saaby), catalogued at
    https://brickset.com/sets/5988-1 — Egyptian pylons, columned rooms and concealed
    chambers inform this original layout; it is not a reproduction of that set.
    Nine native height units equal 27 plate-height units. Bricks have studs up;
    wall courses are staggered. Gravity support does not assume clutch strength.
    """
    model=Model();rise=1;width=64;depth=64
    def panel(x,north,z,courses,turn=False):
        name=f'wall-{courses}-'+('north' if turn else 'east')
        if name not in model.definitions:
            model.definitions[name]=[]
            rotation=yaw(math.pi/2 if turn else 0)
            for layer in range(courses):
                color=14 if layer in [0,courses-1] else (272 if layer==courses-2 and courses>3 else 19)
                long,half,span=('2456','3002',3) if turn else ('3001','3003',2)
                for offset,part in ([(0,long)] if layer%2==0 else [(0,half),(span,half)]):
                    model.part(part,color,0 if turn else offset,offset if turn else 0,layer*rise,
                               rotation=rotation,definition=name)
            if courses==3:model.part('3795' if turn else '3020',14,0,0,3,rotation=rotation,definition=name)
        model.reference(name,x,north,z)
    model.definitions['pillar']=[]
    for layer in range(PALACE_BRICK_COURSES):
        model.part('3001',14 if layer==8 else (28 if layer==4 else 19),0,0,layer,definition='pillar')
    def balcony_rail(length,x,north,z,turn=False,lift_base=False):
        rotation=yaw(math.pi/2 if turn else 0)
        if lift_base:
            for offset in range(0,length,8):
                size=min(8,length-offset)
                model.part('3034' if size==8 else '3020',14,x+(0 if turn else offset),north+(offset if turn else 0),z,rotation=rotation)
            z+=1/3
        for layer in range(4):
            remaining=length;offset=0
            sizes=[]
            if layer==1:
                initial=2 if length==5 else 4;sizes.append(initial);remaining-=initial
            for size in [8,6,4,3,2]:
                while remaining>=size:
                    if remaining-size==1:break
                    sizes.append(size);remaining-=size
            assert remaining==0
            for size in sizes:
                part=({8:'3034',6:'3795',4:'3020',3:'3021',2:'3022'} if layer==3 else {8:'3007',6:'2456',4:'3001',3:'3002',2:'3003'})[size]
                model.part(part,14 if layer==3 else 19,x+(0 if turn else offset),north+(offset if turn else 0),z+min(layer,3),rotation=rotation)
                offset+=size
    full=[{(0,8,False),(1,8,False),(6,8,False),(7,8,False),
           (5,4,False),(6,4,False),(6,6,True),(7,5,True)},
          {(1,7,False),(2,7,False),(5,7,False),(6,7,False),(5,4,False),(6,4,False),(6,6,True),(7,5,True)},
          {(2,6,False),(3,6,False),(4,6,False),(5,6,False),(2,2,False),(5,2,False),(5,5,True),(6,5,True)}]
    full[0].update((i,0,False) for i in [1,2,5,6])
    full[1].update([(6,6,True),(7,6,True),(6,7,False)])
    full[2].update([(5,5,True),(6,5,True),(5,6,False)])
    def plate_rectangle(x,north,w,d,z,color):
        if not w or not d:return
        sizes=[('41539',8,8),('3035',8,4),('3032',6,4),('3031',4,4),
               ('3034',8,2),('3795',6,2),('3020',4,2),('3021',3,2),('3022',2,2),
               ('3710',4,1),('3023',2,1),('3024',1,1)]
        options=[(pw*pd,part,pw,pd,turn) for part,pw,pd in sizes for pw,pd,turn in [(pw,pd,False),(pd,pw,True)] if pw<=w and pd<=d]
        _,part,pw,pd,turn=max(options,key=lambda v:v[0])
        model.part(part,color,x,north,z,turn=turn)
        plate_rectangle(x+pw,north,w-pw,pd,z,color)
        plate_rectangle(x,north+pd,w,d-pd,z,color)
    def centered_slab(level,lo,hi,roof=False,foundation=False):
        # Plate seams fall halfway between column centres, preserving complete
        # 4x2 engagement at column feet instead of four weak 2x1 corner joints.
        cuts=[lo]+[v for v in range(lo+1,hi) if v%8==4]+[hi]
        for x,xhi in zip(cuts,cuts[1:]):
            for north,nhi in zip(cuts,cuts[1:]):
                color=19 if foundation else (14 if x==lo or xhi==hi or north==lo or nhi==hi else 19)
                if roof and 20<=x<44 and 20<=north<44:color=272
                cut=None if roof or foundation else (max(x,24),min(xhi,32),max(north,8),min(nhi,32))
                if cut and cut[0]<cut[1] and cut[2]<cut[3]:
                    a,b,c,d=cut
                    rectangles=[(x,north,a-x,nhi-north),(b,north,xhi-b,nhi-north),
                                (a,north,b-a,c-north),(a,d,b-a,nhi-d)]
                else:rectangles=[(x,north,xhi-x,nhi-north)]
                for px,pn,w,d in rectangles:plate_rectangle(px,pn,w,d,level-1/3,color)
        if not roof and not foundation:
            for north in range(8,32,8):
                for edge in [24,30]:model.part('3034',14,edge,north,level-1/3,turn=True)
    def slab(level,roof=False):
        # The lower layer retains broad compression-bearing support from
        # surrounding columns; the staggered upper layer strengthens engagement.
        inset=16 if roof else (8 if level==PALACE_LEVELS[2] else 0)
        for x in range(inset,width-inset,8):
            for north in range(inset,depth-inset,8):
                if level>0 and not roof and x==24 and 8<=north<32:
                    for edge in [24,30]:model.part('3034',14,edge,north,level-PALACE_FLOOR_HEIGHT,turn=True)
                else:
                    color=14 if x in [inset,width-inset-8] or north in [inset,depth-inset-8] else 19
                    if roof and x in [24,32] and north in [24,32]:color=272
                    model.part('41539',color,x,north,level-PALACE_FLOOR_HEIGHT)
        # Staggered upper layer gives each upper column a full stud footprint,
        # while the lower layer distributes compression to several columns.
        centered_slab(level,inset,width-inset,roof=roof)
    # Extended ground terrace supports full perimeter-column footprints.
    centered_slab(1/3,-8,width+8,foundation=True)
    for storey,base in enumerate(PALACE_LEVELS):
        if storey:slab(base)
        # Each upper colonnade steps back one bay, so its feet bear fully on
        # the floor below instead of balancing on a floor's outer edge.
        posts={(i,j) for i in range(storey,9-storey) for j in range(storey,9-storey)}
        for i,j in sorted(posts):model.reference('pillar',i*8-2,j*8-1,base)
        # Open colonnades and tall enclosed maze/chamber bays leave clear walking routes.
        panels={}
        for i in range(storey,8-storey):
            for j in [storey,8-storey]:
                if j==storey and i in [3,4]:continue
                panels[(i,j,False)]=3
        for j in range(storey,8-storey):
            for i in [storey,8-storey]:panels[(i,j,True)]=3
        for key in full[storey]:panels[key]=PALACE_BRICK_COURSES
        for (i,j,turn),height in panels.items():
            panel(i*8-1 if turn else i*8+2,j*8+1 if turn else j*8-1,base,height,turn)
        if storey:
            edge=(storey-1)*8;span=width-2*edge
            for n in [edge,depth-edge-2]:balcony_rail(span,edge,n,base,lift_base=storey==2 and n==edge)
            for x in [edge,width-edge-2]:balcony_rail(span-4,x,edge+2,base,True)
            if storey==2:
                for x in [22,32]:balcony_rail(5,x,10,base,True)
            for north in range(17 if storey==2 else 9,32,8):
                for x in [22,32]:panel(x,north,base,3,True)
            # The lower balcony remains outside the set-back colonnade.
    slab(PALACE_LEVELS[-1]+PALACE_CLEAR_HEIGHT+PALACE_FLOOR_HEIGHT,roof=True)
    crown=PALACE_LEVELS[-1]+PALACE_CLEAR_HEIGHT+PALACE_FLOOR_HEIGHT
    for layer in range(2):
        for north in [16,depth-18]:
            runs=[(x,'3007') for x in range(16,width-16,8)] if layer==0 else [(16,'3001')]+[(x,'3007') for x in range(20,width-20,8)]+[(width-20,'3001')]
            for x,part in runs:model.part(part,14,x,north,crown+layer*rise)
        for x in [16,width-18]:
            rotation=yaw(math.pi/2)
            runs=[(n,'3007') for n in range(18,depth-22,8)]+[(depth-22,'3001')] if layer==0 else [(18,'3001')]+[(n,'3007') for n in range(22,depth-18,8)]
            for north,part in runs:model.part(part,14,x,north,crown+layer*rise,rotation=rotation)
    # Nine one-brick risers per flight, then a landing flush with the next floor.
    # Upright 1x1 side posts leave a two-unit clear central walking lane.
    for flight,base in enumerate(PALACE_LEVELS[:2]):
        for step in range(1,10):
            north=8+(step-1)*2
            for x in [26.5,29.5]:
                for n in [north,north+1]:
                    z=base if flight==0 else PALACE_LEVELS[0]+step
                    for _ in range(step-1 if flight==0 else 9):model.part('3005',19,x-.5,n,z);z+=1
                    if flight==0:
                        for _ in range(2):model.part('3024',19,x-.5,n,z);z+=1/3
                    else:model.part('3024',19,x-.5,n,z)
            model.part('3020',14,26,north,base+step*rise-1/3)
        for x in [26.5,29.5]:
            for north in [26.5,31.5]:
                for layer in range(9):model.part('3005',19,x-.5,north-.5,base+layer)
        model.part('3032',14,26,26,base+9,turn=True)
        model.part('3032',14,26,26,base+9+1/3,turn=True)
    # Raised chamber trap wells retain solid bearing floors under every column.
    # These are physical decorations, not scripted gameplay triggers.
    for base,north,xbase in [(PALACE_LEVELS[0],48,48),(PALACE_LEVELS[1],40,40)]:
        for layer in range(2):
            for n in [north+2,north+5]:model.part('2456',14 if layer else 19,xbase+2,n,base+layer)
            for x in [xbase+2,xbase+6]:model.part('3004',14 if layer else 19,x,north+4,base+layer)
        model.part('3004',0,xbase+4,north+4,base)
    return model


def run_model(binary,output,name,model,origin,token_file=None,publish=False,estimated_connectors=False):
    origin=snap_origin(origin)
    output.mkdir(parents=True,exist_ok=True);workspace=output/"construction.ergo.json"
    if workspace.exists():raise RuntimeError("Choose a fresh output directory: "+str(output))
    native=model.export(tuple(origin));(output/"model.ergo-model.json").write_text(json.dumps(native,separators=(',',':'))+'\n')
    mcp=demo.Mcp(binary,workspace,'https://ergosandbox.com',None)
    try:
        technique_plan=mcp.call('get_building_techniques',query='palace' if 'palace' in name.lower() else 'chalet')['structuredContent']
        mcp.call('place_submodel',model=native)
        validation=mcp.call('validate_construction')['structuredContent']
        lo,hi=bounds(native);target=[(a+b)/2 for a,b in zip(lo,hi)];span=max(b-a for a,b in zip(lo,hi))
        camera={'position_world':[target[0]+span*.9,target[1]-span*1.2,target[2]+span*.8],
                'target_world':target,'vertical_fov_degrees':42}
        image=mcp.call('test_visual_appearance',camera=camera,width=1024,height=768,lighting='studio')
        picture=next(c for c in image['content'] if c['type']=='image')
        (output/'preview.png').write_bytes(base64.b64decode(picture['data']))
        report={'name':name,'validation':validation,'origin':origin,'shared_committed':False,'technique_plan':technique_plan}
        (output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    finally:mcp.close()
    if publish or estimated_connectors:
        if publish and not validation['publication_ready']:raise RuntimeError('Not publishable: '+json.dumps(validation))
        # A gravity compression certificate alone does not establish robustness.
        mcp=demo.Mcp(binary,workspace,'https://ergosandbox.com',None)
        try:
            stability=mcp.call('test_small_earthquake_stability',include_images=False,
                connection_model='estimated' if estimated_connectors else 'measured')['structuredContent']
            report['stability']=stability
            (output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
            accepted_model='stable_under_declared_estimates' if estimated_connectors else 'calibrated_stability_verified'
            if publish and not stability[accepted_model]:
                raise RuntimeError('Publication withheld: the selected connector model did not pass gravity/small-shake checks; inspect report.json')
        finally:mcp.close()
    if publish:
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
    parser.add_argument('--estimated-connectors',action='store_true',help='Explicit ballpark connector diagnostics, not measured calibration')
    args=parser.parse_args();binary=shutil.which(args.binary)
    args.origin=snap_origin(args.origin)
    if not binary:parser.error('Install the released client or pass --binary')
    if args.publish and not args.token_file:parser.error('--publish requires --token-file')
    if args.kind=='palace':
        run_model(binary,args.output,'Palace of the Hidden Sun',palace(),args.origin,args.token_file,args.publish,args.estimated_connectors)
        (args.output/'walk-route.json').write_text(json.dumps(palace_walk_route(args.origin),indent=2)+'\n')
    elif args.kind=='chalet':
        run_model(binary,args.output,'Alpine chalet',chalet(15,70,320,288),args.origin,args.token_file,args.publish,args.estimated_connectors)
        (args.output/'walk-route.json').write_text(json.dumps(chalet_model.walk_route(args.origin),indent=2)+'\n')
    else:
        layout=run_model(binary,args.output/'layout-preflight','Alpine village layout preflight',village_layout(),args.origin)
        if not layout['validation']['publication_ready']:raise RuntimeError('Combined village layout is not publishable')
        styles=[(15,19,320,288),(19,70,72,4),(15,70,70,272)]
        for row,north in enumerate([0,56]):
            for column,x in enumerate([0,40,80,120]):
                model=chalet(*styles[(column+row)%3])
                if row==0:
                    placed=Model();placed.definitions={**{('house' if k=='main' else k):v for k,v in model.definitions.items()},'main':[]}
                    placed.reference('house',24,16,0,rotation=yaw(math.pi));model=placed
                origin=[args.origin[0]+x,args.origin[1]+north,args.origin[2]]
                run_model(binary,args.output/f'chalet-{row*4+column+1}',f'Alpine chalet {row*4+column+1}',model,origin,args.token_file,args.publish,args.estimated_connectors)
                (args.output/f'chalet-{row*4+column+1}'/'walk-route.json').write_text(json.dumps(chalet_model.walk_route(origin,row==0),indent=2)+'\n')
        run_model(binary,args.output/'square','Alpine village square',landscape(),args.origin,args.token_file,args.publish,args.estimated_connectors)


if __name__=='__main__':main()
