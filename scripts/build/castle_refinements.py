#!/usr/bin/env python3
"""Refine the accepted v0.5.20 castle in a fresh local draft; never publish implicitly."""
import argparse
import base64
import collections
import json
import math
import pathlib
import subprocess
import sys
from functools import cache
from typing import cast

ROOT=pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT))
from scripts.build import castle_landscape as baseline,park
from scripts.build.district import write
from scripts.build.monument_models import Masonry,stair,tower_route
from scripts.native_model import Model,PARTS,PART_LIBRARY,instance,model_leaves,yaw
from scripts.mcp_client import Mcp

RAISE=baseline.RAISE
KEEP=(224,168)
OLD_KEEP_ROOF=RAISE+82
KEEP_PITCH=29/3
NEW_KEEP_ROOF=RAISE+1+9*KEEP_PITCH
TOWERS=[(168+x,96+y) for y in [0,112] for x in [0,40,88,128]]+[(168+x,96+y) for x in [0,128] for y in [36,76]]


def multiply(a,b):return baseline.multiply(a,b)
def translation(x:float=0,y:float=0,z:float=0):return instance('part','3003.dat',[x,y,z])['transform']
def transform(point,m):return [sum(m[i][j]*point[j] for j in range(3))+m[i][3] for i in range(3)]
def key(part,colour,m):return baseline.pose_key(part,colour,m)
def brick_key(brick):return key(brick['part'],brick['color'],baseline.brick_reference(brick)['transform'])


def cells(low,high):
    return {(x,n,z) for x in range(low[0],high[0]) for n in range(low[1],high[1]) for z in range(low[2],high[2])}


def plate_bounds(brick):
    low=brick['bounds']['min'];high=brick['bounds']['max']
    lo=[low[0],low[1],low[2]*3];hi=[high[0],high[1],high[2]*3]
    if any(abs(v-round(v))>1e-6 for v in lo+hi):raise ValueError('Structural edit is not on the stud/plate grid')
    return list(map(round,lo)),list(map(round,hi))


def pack_cells(model,remaining,colour):
    shapes=[]
    for part,(w,d,h) in PARTS.items():
        if not PART_LIBRARY['parts'][part+'.dat']['catalogue']['studs']:continue
        height=round(h*3)
        if height not in [1,3] or abs(h*3-height)>1e-8:continue
        for a,b,turn in [(round(w),round(d),False),(round(d),round(w),True)]:
            shapes.append((a*b*height,height,part,a,b,turn))
    shapes.sort(reverse=True)
    remaining=set(remaining)
    ordered=iter(sorted(remaining,key=lambda p:(p[2],p[1],p[0])))
    while remaining:
        anchor=next(ordered)
        while anchor not in remaining:anchor=next(ordered)
        x,n,z=anchor
        for _,height,part,w,d,turn in shapes:
            if (x+w-1,n+d-1,z+height-1) not in remaining:continue
            if all((x+a,n+b,z+c) in remaining for c in range(height) for b in range(d) for a in range(w)):
                model.part(part,colour,x,n,z/3,turn);remaining-=cells([x,n,z],[x+w,n+d,z+height]);break
        else:raise ValueError('Cannot cover structural remnant with catalogue parts')


class Editor:
    def __init__(self,scene):
        self.scene=json.loads(json.dumps(scene));self.removed=set();self.model=Model();self.routes=[];self.notes={}

    def add_model(self,model,matrix=None):
        for leaf in model_leaves(model):
            pose=leaf['transform'] if matrix is None else multiply(matrix,leaf['transform'])
            self.model.definitions['main'].append(dict(target=dict(kind='part',id=leaf['part']),color=leaf['color'],transform=pose))

    def cut(self,cut):
        if not cut:return
        low=[min(p[a] for p in cut) for a in range(3)];high=[max(p[a] for p in cut)+1 for a in range(3)]
        remnants=collections.defaultdict(set)
        for b in self.scene['bricks']:
            if b['id'] in self.removed or b['part'].removesuffix('.dat') not in PARTS or b['color'] not in [71,72]:continue
            lo,hi=plate_bounds(b)
            if any(hi[a]<=low[a] or lo[a]>=high[a] for a in range(3)):continue
            body=cells(lo,hi)
            if body&cut:self.removed.add(b['id']);remnants[b['color']]|=body-cut
        for colour,body in remnants.items():pack_cells(self.model,body,colour)

    def replace_model(self,old,new):
        index=collections.defaultdict(list)
        for b in self.scene['bricks']:
            if b['id'] not in self.removed:index[brick_key(b)].append(b['id'])
        before=collections.Counter(key(v['part'],v['color'],v['transform']) for v in model_leaves(old))
        after=collections.Counter(key(v['part'],v['color'],v['transform']) for v in model_leaves(new))
        for pose,count in (before-after).items():
            if len(index[pose])<count:raise ValueError('Accepted bridge/terrain baseline changed; inspect before applying the recipe')
            for _ in range(count):self.removed.add(index[pose].pop())
        extra=after-before
        for leaf in model_leaves(new):
            pose=key(leaf['part'],leaf['color'],leaf['transform'])
            if extra[pose]:
                self.model.definitions['main'].append(dict(target=dict(kind='part',id=leaf['part']),color=leaf['color'],transform=leaf['transform']));extra[pose]-=1


@cache
def bridge_spans():
    # An 8-stud mould has six studs of clear opening. Two-stud piers eight
    # studs apart leave exactly that opening; each foot uses the inner stud.
    return tuple(start for start in range(73,169,8)
                 if max(baseline.natural_height(x,n) for x in range(start,start+9,2) for n in [157,159,161,163])<=21)


def refined_height(x,n):
    result=baseline.natural_height(x,n)
    if 224<=x<256 and 80<=n<96:result=max(result,25)
    if 156<=n<164 and 72<=x<170:
        pier=72+8*int((x-72)//8)
        active_piers={start-1 for start in bridge_spans()}|{start+7 for start in bridge_spans()}
        if pier in active_piers and pier<=x<pier+2:result=max(result,21)
        elif pier+1 not in bridge_spans() and x<168:result=max(result,24)
    return result


def refined_terrain():
    m=Model();raster=[(x,n) for n in range(0,320,2) for x in range(0,384,2)]
    for wet,colour in [(False,288),(True,321)]:
        allowed=[(x,n) for x,n in raster if baseline.water(x+1,n+1)==wet]
        for part,x,n,turn in park.rectangles(allowed,['3068b'] if wet else ['41539','3035','3032','3031','3020','3022','3958','3036']):m.part(part,colour,x,n,0,turn)
    for level in range(28):
        allowed=[(x,n) for x,n in raster if refined_height(x+1,n+1)>level]
        for part,x,n,turn in park.rectangles(allowed,['4204','4201','3007','2456','3001','3003'],reverse=bool(level%2)):m.part(part,72 if level<20 else 71,x,n,1/3+level,turn)
    for start in bridge_spans():
        for north in range(156,164):m.definitions['main'].append(instance('part','3308a.dat',[start+4,north+.5,24+1/3],71))
    for x in sorted({start for start in bridge_spans()}|{start+7 for start in bridge_spans()}):m.part('3008',71,x,156,21+1/3,True)
    for start in range(72,168,16):m.part('4204',71,start,156,24+1/3)
    m.part('4201',71,64,156,24+1/3)
    for north in [156,163]:
        for start in range(72,168,8):m.part('3008',71,start,north,25+1/3);m.strip(8,71,start,north,26+1/3)
    return m.export()


def panel_joins(editor):
    additions=Model();cut=set();count=0
    for b in editor.scene['bricks']:
        if b['part']!='4444.dat':continue
        parent=baseline.brick_reference(b)['transform']
        for offset,height in [(x,h) for h in [0,-5] for x in [-2.5,2.5]]:
            ref=instance('part','3004.dat',[offset,0,height],b['color'])
            pose=multiply(parent,ref['transform']);additions.definitions['main'].append(dict(ref,transform=pose))
            corners=[transform([x,n,z],pose) for x in [-1,1] for n in [-.5,.5] for z in [-1,0]]
            lo=[round(min(p[a] for p in corners)*(3 if a==2 else 1)) for a in range(3)]
            hi=[round(max(p[a] for p in corners)*(3 if a==2 else 1)) for a in range(3)]
            cut|=cells(lo,hi);count+=1
        # The rear stud row sits one plate below the main header, at source
        # Z=20/Y=8. A real 1x3 plate fills it without closing the arrowslit.
        ref=instance('part','3623.dat',[0,1,0],b['color'])
        additions.definitions['main'].append(dict(ref,transform=multiply(parent,ref['transform'])))
    editor.cut(cut);editor.add_model(additions.export());editor.notes.update(panel_join_bricks=count,panel_shelf_plates=count//4)


def figures(scene):
    result=[]
    for leg in scene['bricks']:
        if leg['part']!='3815c01.dat':continue
        x,n,h=leg['position']
        def find(parts,low,high):
            found=[b for b in scene['bricks'] if b['part'] in parts and abs(b['position'][0]-x)<1e-6 and abs(b['position'][1]-n)<1e-6 and low-1e-6<=b['position'][2]-h<=high+1e-6]
            return found
        torso=find(['76382p4f.dat'],4/3,4/3);head=find(['3626ap01.dat'],2.3,2.5);helmet=find(['3844.dat','3896.dat','89520.dat'],2.3,2.5)
        cape=find(['4524.dat','50231c01.dat'],1.3,1.5)
        if len(torso)!=1 or len(head)!=1 or len(helmet)!=1 or len(cape)>1:raise ValueError('Ambiguous accepted figure composition')
        result.append(dict(leg=leg,torso=torso[0],head=head[0],helmet=helmet[0],cape=cape[0] if cape else None))
    if len(result)!=300:raise ValueError('Expected the accepted 300 plate-free standing figures')
    return result


def move_figure(group,x,n,feet):
    leg=group['leg'];rotation=cast(list[list[float]],baseline.brick_reference(leg)['transform'])
    first=[-.5*rotation[0][0],-.5*rotation[1][0]]
    x+=round(x+first[0]-.5)+.5-(x+first[0])
    n+=round(n+first[1]-.5)+.5-(n+first[1])
    delta=[x-leg['position'][0],n-leg['position'][1],feet+5/3-leg['position'][2]]
    for b in group.values():
        if b is not None:b['position']=[a+d for a,d in zip(b['position'],delta)]


def small_capes(groups):
    replaced=0
    for g in groups:
        cape=g['cape']
        if cape and cape['part']=='50231c01.dat':
            cape['part']='4524.dat';cape['position'][2]+=2/24
            for b in [g['head'],g['helmet']]:b['position'][2]+=(2-.6)/24
            replaced+=1
    return replaced


def keep_model():
    m=Masonry();well=[(10,14,6,16),(18,22,6,16),(10,22,16,20)];main=[(0,32,0,32)]
    for level in range(9):
        base=level*KEEP_PITCH
        bays=[8,20] if level%2==0 else [4,14,24]
        if level:m.slab(main,base,71,well)
        for course in range(9):
            holes=[]
            if 3<=course<6:
                for x in bays:holes += [(x,x+4,0,2),(x,x+4,30,32),(0,2,x,x+4),(30,32,x,x+4)]
            if level==0 and course<8:holes.append((12,20,0,2))
            for north in [0,30]:m.brickwork([(0,32,north,north+2)],base+course,72 if course==8 else 71,holes,phase=4*(course%2))
            for east in [0,30]:m.brickwork([(east,east+2,2,30)],base+course,72 if course==8 else 71,holes,turn=True,phase=4*(course%2))
            for east in [8,16,24]:m.part('3001',71,east-2,22,base+course)
        for side in range(4):
            for x in bays:
                if level==0 and side==0 and 8<x<20:continue
                for depth in [0,1]:
                    if side==0:px,pn,angle=x,depth+.5,0
                    elif side==1:px,pn,angle=x+4,31.5-depth,math.pi
                    elif side==2:px,pn,angle=depth+.5,x,math.pi/2
                    else:px,pn,angle=31.5-depth,x+4,-math.pi/2
                    parent=instance('part','3003.dat',[px,pn,base],rotation=yaw(angle))['transform']
                    # Window-local x ranges 0..4, with two upright end pillars.
                    local=Model()
                    for edge in [0,3]:
                        for h in [3,4]:local.part('3005',71,edge,-.5,h)
                    local.definitions['main'].append(instance('part','3659.dat',[2,0,6],71))
                    for leaf in model_leaves(local.export()):m.definitions['main'].append(dict(target=dict(kind='part',id=leaf['part']),color=leaf['color'],transform=multiply(parent,leaf['transform'])))
        if level==0:
            for edge in [12,19]:
                for h in range(6):m.part('3004',71,edge,0,base+h,True)
            for north in [.5,1.5]:m.definitions['main'].append(instance('part','3308a.dat',[16,north,base+8],71))
        stair(m,base,True)
    m.slab(main,9*KEEP_PITCH,71,well)
    return m.export([KEEP[0],KEEP[1],RAISE+1])


def rebuild_keep(editor,groups):
    x,n=KEEP;low=round((RAISE+1)*3);high=round(OLD_KEEP_ROOF*3)
    editor.cut(cells([x,n,low],[x+32,n+32,high]))
    for b in editor.scene['bricks']:
        if b['id'] in editor.removed:continue
        if b['part']=='60594.dat' and x-1<=b['position'][0]<=x+33 and n-1<=b['position'][1]<=n+33:editor.removed.add(b['id'])
        elif x-1e-6<=b['position'][0]<=x+32+1e-6 and n-1e-6<=b['position'][1]<=n+32+1e-6 and b['bounds']['min'][2]>=OLD_KEEP_ROOF-1e-6:b['position'][2]+=NEW_KEEP_ROOF-OLD_KEEP_ROOF
    editor.add_model(keep_model())
    keep_figures=[g for g in groups if x<=g['leg']['position'][0]<=x+32 and n<=g['leg']['position'][1]<=n+32 and g['leg']['position'][2]>RAISE+80]
    slots=[(a,26.5) for a in [8,12,16,20,24]]+[(4,a) for a in [8.5,12.5,16.5,20.5,24.5]]+[(28,a) for a in [8.5,12.5,16.5,20.5,24.5]]+[(a,22.5) for a in [8,12,16,24]]+[(8,a) for a in [8.5,12.5,16.5,20.5]]
    if len(keep_figures)!=len(slots):raise ValueError('Unexpected keep rooftop garrison')
    for g,(a,b) in zip(keep_figures,slots):move_figure(g,x+a,n+b,NEW_KEEP_ROOF)
    route=tower_route([x,n,RAISE+1-2/3],9)
    insert=next(i for i,v in enumerate(route['waypoints']) if v['name'].startswith('descend from level'))
    floor=2/3+8*KEEP_PITCH;roof=2/3+9*KEEP_PITCH
    excursion=[('crown first flight',[12,18,floor+4]),('crown half landing',[20,18,floor+4]),
               ('crown arrival',[20,3,roof]),('crown south access',[24,3,roof]),('crown east viewing',[24,18,roof]),
               ('crown return',[24,3,roof]),('crown stair return',[20,3,roof]),
               ('crown descent landing',[20,18,floor+4]),('crown descent first flight',[12,18,floor+4]),('crown descent arrival',[12,3,floor])]
    route['waypoints'][insert:insert]=[dict(name=name,feet=point,min_clearance=5.5) for name,point in excursion]
    # The shared stair module has nine risers/flight; this keep has nine levels
    # and an open, accessible crown instead of a closed decorative roof.
    editor.routes.append(('keep',route))
    editor.notes.update(modern_keep_frames_removed=72,medieval_keep_storeys=9,keep_roof_height=NEW_KEEP_ROOF)


def transformed_cells(low,high,matrix):
    corners=[transform([x,n,z],matrix) for x in [low[0],high[0]] for n in [low[1],high[1]] for z in [low[2],high[2]]]
    lo=[round(min(p[a] for p in corners)*(3 if a==2 else 1)) for a in range(3)]
    hi=[round(max(p[a] for p in corners)*(3 if a==2 else 1)) for a in range(3)]
    return cells(lo,hi)


def tower_frame(x,n):
    if n==96:angle=math.pi
    elif n==208:angle=0
    elif x==168:angle=math.pi/2
    else:angle=-math.pi/2
    return multiply(translation(x+8,n+8,RAISE+1),multiply(instance('part','3003.dat',[0,0,0],rotation=yaw(angle))['transform'],translation(-8,-8,0)))


def tower_access(editor,groups):
    all_cut=set();models=[];displaced=[]
    for index,(x,n) in enumerate(TOWERS):
        corner=x in [168,296] and n in [96,208]
        levels=3 if corner else 4
        frame=tower_frame(x,n);m=Model();columns=set();treads=[]
        for level in range(levels):
            base=level*9
            for step,north in enumerate([3,5,7,9],1):
                m.part('3034',70,1,north,base+step-1/3)
                treads.append(([1,north,base+step-1/3],[9,north+2,base+step]))
            m.part('4282',70,0,11,base+5-1/3)
            treads.append(([0,11,base+5-1/3],[16,13,base+5]))
            for step,north in [(6,9),(7,7),(8,5)]:
                m.part('3034',70,7,north,base+step-1/3)
                treads.append(([7,north,base+step-1/3],[15,north+2,base+step]))
            # A notched masonry stringer bears each radial tread. Its low
            # front stub leaves a traversable crossing on the floor landing.
            columns|=cells([7,3,base*3],[9,5,base*3+2])
            columns|=cells([7,5,base*3],[9,11,base*3+25])
            floor=(level+1)*9
            all_cut|=transformed_cells([2,2,floor-1],[14,14,floor],frame)
            for layer in range(2):
                h=floor-2/3+layer/3
                for north in [2,4]:m.part('4282',71,0,north,h)
                m.part('3034',71,1,6,h)
                m.part('4477',71,0,8,h)
                m.part('3022',71,7,9,h)
        for low,high in treads:
            columns-=cells([round(low[0]),round(low[1]),round(low[2]*3)],
                           [round(high[0]),round(high[1]),round(high[2]*3)])
        pack_cells(m,columns,71)
        # Every new beam's finite slot replaces the intersected old masonry;
        # no tread floats through a wall or gets a concealed support pedestal.
        for leaf in model_leaves(m.export()):
            part=leaf['part'].removesuffix('.dat');w,d,h=PARTS[part]
            if h!=1/3:continue
            parent=multiply(frame,leaf['transform'])
            all_cut|=transformed_cells([-w/2,-d/2,-h],[w/2,d/2,0],parent)
        all_cut|=transformed_cells([4,0,0],[12,2,8],frame)
        for edge in [4,11]:
            for h in range(6):m.part('3004',71,edge,0,h,True)
        for north in [.5,1.5]:m.definitions['main'].append(instance('part','3308a.dat',[8,north,8],71))
        models.append((m.export(),frame))
        points=[('entry',[8,-2,0]),('arched entrance',[8,1,0])]
        climb=[]
        for level in range(levels):
            base=level*9
            climb.extend([(f'floor {level+1} tread {i}',[5,north,base+i]) for i,north in enumerate([4,6,8,10],1)])
            climb.extend([(f'floor {level+1} north landing',[5,12,base+5]),(f'floor {level+1} landing crossing',[11,12,base+5])])
            climb.extend([(f'floor {level+1} return tread {i}',[11,north,base+i]) for i,north in [(6,10),(7,8),(8,6.5)]])
            climb.append((f'floor {level+2} arrival',[11,3.5,base+9]))
        points+=climb
        points += [('upper landing',[6.5,3.5,levels*9]),('upper gallery',[6.5,6,levels*9]),('gallery return',[11,3.5,levels*9])]
        points += [('descending '+name,point) for name,point in reversed(climb[:-1])]
        points += [('exit',[8,1,0]),('courtyard return',[8,-2,0])]
        editor.routes.append((f'tower-{index+1:02}',dict(format='ergo-walk-route-1',schema=2,origin_world=[0,0,0],
            start=transform(points[0][1],frame),eye_height=4,min_clearance=5,speed=6,
            waypoints=[dict(name=name,feet=transform(point,frame),min_clearance=5) for name,point in points])))
        if not corner:
            roof=[g for g in groups if x<=g['leg']['position'][0]<x+16 and n<=g['leg']['position'][1]<n+16 and abs(g['leg']['position'][2]-5/3-(RAISE+37))<1e-6]
            if len(roof)!=4:raise ValueError('Unexpected turret rooftop garrison')
            for g,point in zip(roof[:2],[[4,3.5,36],[4,7.5,36]]):
                position=transform(point,frame);move_figure(g,*position)
            displaced+=roof[2:]
    editor.cut(all_cut)
    for model,frame in models:editor.add_model(model,frame)
    slots=[(x,n) for x in [232,248] for n in [118.5,124.5,130.5,136.5,142.5,148.5,154.5,160.5]]
    if len(displaced)!=len(slots):raise ValueError('Unexpected number of figures displaced by roof hatches')
    for g,(x,n) in zip(displaced,slots):move_figure(g,x,n,RAISE+1)
    editor.notes.update(visitable_defensive_towers=12,tower_stair_riser=1,tower_minimum_headroom=5,roof_figures_repositioned=len(displaced))


def assemble(editor,url):
    import urllib.request
    scene=editor.scene;meshes=dict(scene['meshes'])
    leaves=list(model_leaves(editor.model.export()))
    needed={b['part'] for b in scene['bricks'] if b['id'] not in editor.removed}|{p['part'] for p in leaves}
    for part in sorted(needed-meshes.keys()):
        with urllib.request.urlopen(url.rstrip('/')+'/api/parts/'+part,timeout=180) as response:asset=json.load(response)
        if asset['mesh']['quality']!='source':raise ValueError('Genuine canonical source required: '+part)
        meshes[part]=asset['mesh']
    with urllib.request.urlopen(url.rstrip('/')+'/api/catalog',timeout=180) as response:palette={c['code']:c['rgb'] for c in json.load(response)['colors']}
    kept=[b for b in scene['bricks'] if b['id'] not in editor.removed]
    refs=[baseline.brick_reference(b) for b in kept];pieces=[dict(id=b['id'],rgb=b['rgb'],step=b['step']) for b in kept]
    next_id=max(b['id'] for b in scene['bricks'])+1
    for leaf in leaves:
        refs.append(dict(target=dict(kind='part',id=leaf['part']),color=leaf['color'],transform=leaf['transform']))
        pieces.append(dict(id=next_id,rgb=palette[leaf['color']],step=0));next_id+=1
    return baseline.native(scene,refs,pieces,meshes,scene['name'])


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--input',type=pathlib.Path,required=True);p.add_argument('--output',type=pathlib.Path,required=True)
    p.add_argument('--url',default='https://ergosandbox.com');p.add_argument('--validate',action='store_true');p.add_argument('--keep',action='store_true');p.add_argument('--towers',action='store_true')
    args=p.parse_args();source=json.loads(args.input.read_text())
    if source['assembly_id']!=baseline.CASTLE or source['external_dependencies']:raise ValueError('A complete accepted castle workspace is required')
    args.output.mkdir(parents=True,exist_ok=False)
    editor=Editor(source['scene']);groups=figures(editor.scene)
    editor.replace_model(baseline.terrain(),refined_terrain());panel_joins(editor)
    editor.notes.update(short_capes_replaced=small_capes(groups),bridge_pier_width=2,bridge_clear_opening=6,bridge_arch_width=8)
    if args.keep:rebuild_keep(editor,groups)
    if args.towers:tower_access(editor,groups)
    document=assemble(editor,args.url);workspace=args.output/'construction.native.json';write(workspace,document)
    write(args.output/'proposal.json',dict(published=False,base=source['base_revision'],assembly_id=baseline.CASTLE,removed_occurrences=len(editor.removed),added_occurrences=len(editor.model.definitions['main']),pieces=len(document['pieces']),**editor.notes))
    write(args.output/'removed-ids.json',sorted(editor.removed))
    routes=[('bridge',baseline.bridge_route()),*editor.routes]
    for name,route in routes:write(args.output/(name+'.route.json'),route)
    if args.validate:
        client=Mcp(str(ROOT/'target/debug/ergo-mcp'),workspace,None,None)
        try:
            feedback=client.call('validate_construction')['structuredContent'];write(args.output/'validation.json',feedback);print(json.dumps(feedback),flush=True)
            if not feedback['publication_ready']:raise ValueError('Inspect failed local static certificate')
            client.call('export_construction')
            for name,route in routes:
                check=subprocess.run([str(ROOT/'target/debug/examples/check_walk'),str(workspace),str(args.output/(name+'.route.json'))],capture_output=True,text=True)
                write(args.output/(name+'.walking.json'),json.loads(check.stdout) if check.returncode==0 else dict(passed=False,error=check.stderr))
                if check.returncode:raise ValueError(name+': '+check.stderr)
            for name,eye,target in [('bridge',[-20,105,75],[120,160,23]),('wall-joins',[203,62,54],[194,100,42]),('keep',[325,80,160],[240,184,75])]:
                image=client.call('test_visual_appearance',width=1024,height=768,lighting='studio',camera=dict(position_world=eye,target_world=target,vertical_fov_degrees=40))
                (args.output/(name+'.png')).write_bytes(base64.b64decode(next(item['data'] for item in image['content'] if item['type']=='image')))
        finally:client.close()
    print(json.dumps(dict(published=False,pieces=len(document['pieces']),**editor.notes)),flush=True)


if __name__=='__main__':main()
