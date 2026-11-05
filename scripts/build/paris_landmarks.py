#!/usr/bin/env python3
"""Current-source Paris landmark drafts, with explicit local validation and no implicit publication."""
import argparse
import base64
import copy
from collections import Counter
from typing import Any
import json
import math
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from scripts.native_model import Model, PART_LIBRARY, PARTS, CONSTRUCTS, SPECIAL_BOUNDS, instance, model_leaves, register_catalog_part, yaw
from scripts.build.monument_models import Masonry, rectangles, stair, tower_route, BASE, PITCH
from scripts.mcp_client import Mcp

GEO: dict[str,float] = dict(latitude=48.8566, longitude=2.3522, arcseconds_per_block=1)
SITES: dict[str,dict[str,Any]] = {
    'saint-sulpice': dict(name='Saint-Sulpice — Église des Lumières', gps=[48+51/60+4/3600, 2+20/60+5/3600],
        center=[144,64], references=['https://paroissesaintsulpice.paris/', 'https://en.wikipedia.org/wiki/Saint-Sulpice,_Paris',
            'https://brickset.com/article/109391/review-21061-notre-dame-de-paris',
            'https://brickset.com/sets/71043-1/Hogwarts-Castle']),
    'eiffel': dict(name='Tour Eiffel — Grande Charpente Technic', gps=[48.85822222,2.2945],
        center=[88,88], references=['https://en.wikipedia.org/wiki/Eiffel_Tower','https://library.ldraw.org/omr/sets/81']),
}


def write(path, value):
    path.write_text(json.dumps(value, separators=(',',':'))+'\n')


def register_profiles():
    for id, entry in PART_LIBRARY['parts'].items():
        nominal = entry['catalogue'].get('nominal')
        if not nominal: continue
        lo, hi = nominal['bounds_ldu']
        bounds = dict(min=[lo[0]/20,lo[2]/20,-hi[1]/24], max=[hi[0]/20,hi[2]/20,-lo[1]/24])
        register_catalog_part(id, dict(quality='source', body_bounds=bounds, bounds=bounds))
    # Current source-reviewed datums from part_profiles.rs, in public axes.
    for part,lo,hi in [('3062b',[-.5,-.5,-1],[.5,.5,0]),('3941',[-1,-1,-1],[1,1,0]),
                       ('87081',[-2,-2,-1],[2,2,0]),('3037',[-2,-1.5,-1],[2,.5,0]),
                       ('3039',[-1,-1.5,-1],[1,.5,0]),
                       ('3815c01',[-.975,-.55,-5/3],[.975,.5,0]),
                       ('76382p00',[-.95,-.5,-4/3],[.95,.5,0]),
                       ('3626ap01',[-.65,-.65,-1],[.65,.65,0]),
                       ('3901',[-.65,-.65,0],[.65,.65,.25])]:
        box=dict(min=lo,max=hi)
        register_catalog_part(part,dict(quality='source',body_bounds=box,bounds=box))


def mount(target, model, name, x:float=0, north:float=0, height:float=0, rotation=None):
    if name+'/main' not in target.definitions:
        for id, refs in model.definitions.items():
            renamed = copy.deepcopy(refs)
            for ref in renamed:
                if ref['target']['kind']=='definition':ref['target']['id']=name+'/'+ref['target']['id']
            target.definitions[name+'/'+id]=renamed
    target.reference(name+'/main',x,north,height,rotation=rotation)


def arch_bay(pier_height=6, depth=2, glass=False, round_columns=False):
    m=Masonry();m.compact_short_rows=True
    for h in range(pier_height):
        if round_columns:
            for x in [0,7]:
                for n in range(depth):m.part('3062b',19,x,n,h)
        else:m.brickwork([(0,1,0,depth),(7,8,0,depth)],h,19,turn=True,phase=4*(h%2))
        if glass and h<pier_height-1:
            for x in range(1,7):
                # Genuine transparent bricks and a dark lead-like central came.
                color=0 if x==3 or h%6==0 else [33,36,46,34,47][(x+h//3)%5]
                m.part('3005',color,x,0,h)
    if glass:
        for x in range(1,7):
            h=pier_height-2;color=0 if x==3 or h%6==0 else [33,36,46,34,47][(x+h//3)%5]
            m.part('3070b',color,x,0,pier_height-1)
    for n in range(depth):m.part('3308a',19,0,n,pier_height)
    return m


def tower_storey(ground=False, open_belfry=False, band=False):
    m=Masonry();m.compact_short_rows=True
    well=[(10,14,6,16),(18,22,6,16),(10,22,16,20)]
    if not ground:
        # Joists bear on the outer wall and real stair stringers. The return
        # flight's cross-well stays open; rear joists bridge to the rear wall.
        for x in [0,6,8,14,16,22,24,30]:
            m.part('3034',19,x,0,-2/3,turn=True)
            m.part('3795',19,x,8,-2/3,turn=True)
            m.part('3023b',19,x,14,-2/3)
            if x in [14,16]:m.part('3023b',19,x,15,-2/3)
            else:
                m.part('4282',19,x,15,-2/3,turn=True)
                m.part('3023b',19,x,31,-2/3)
        for n in range(0,32,2):
            if n<6:
                for x in [0,16]:m.part('4282',19,x,n,-1/3)
            elif n<20:
                m.part('3034',19,0,n,-1/3);m.part('3022',19,8,n,-1/3)
                if n<16:m.part('3020',19,14,n,-1/3)
                m.part('3022',19,22,n,-1/3);m.part('3034',19,24,n,-1/3)
            else:
                m.part('3034',19,0,n,-1/3);m.part('4282',19,8,n,-1/3);m.part('3034',19,24,n,-1/3)
    window=arch_bay(4,2,not open_belfry)
    bays=[4,12,20] if ground or open_belfry else [12]
    for course in range(9):
        walls=[(0,32,0,2),(0,32,30,32),(0,2,2,30),(30,32,2,30)]
        holes=[(x,x+2,n,n+2) for x in [0,30] for n in [0,30]]
        if 2<=course<8:
            for x in bays:
                if not ground or x!=12:holes.append((x,x+8,0,2))
                holes.append((x,x+8,30,32))
                holes += [(0,2,x,x+8),(30,32,x,x+8)]
        if ground and course<8:holes.append((12,20,0,2))
        m.brickwork(walls,course,15 if course==8 and band else 19,holes,turn=bool(course%2),phase=4*(course%2))
        for x in [0,30]:
            for n in [0,30]:m.part('3941',15 if course==8 and band else 19,x,n,course)
        # Real rear-room columns carry the small floor plates; the stair
        # stringers end at north=20 and cannot support the whole rear bay.
        m.brickwork([(8,12,22,26),(16,20,22,26),(22,26,22,26)],course,19,turn=bool(course%2))
    for x in bays:
        if not ground or x!=12:mount(m,window,'window',x,0,2)
        mount(m,window,'window',x,30,2)
        mount(m,window,'window',2,x,2,yaw(math.pi/2))
        mount(m,window,'window',32,x,2,yaw(math.pi/2))
    if ground:mount(m,arch_bay(6,2,False),'door',12,0,0)
    stair(m,0)
    return m


def church_tower(levels):
    m=Masonry();storeys={}
    for level in range(levels):
        key=(level==0,level>=levels-2,level in [0,levels//2,levels-3,levels-1])
        if key not in storeys:storeys[key]=tower_storey(*key)
        mount(m,storeys[key],'tower-stage-'+''.join(str(int(v)) for v in key),height=level*PITCH)
    top=levels*PITCH
    # The crown closes the stairwell on a genuine two-way joist grillage,
    # rather than letting small tiled floor pieces cantilever over the rooms.
    for x in [0,6,8,14,16,22,24,30]:
        m.part('3034',15,x,0,top-2/3,turn=True)
        m.part('3795',15,x,8,top-2/3,turn=True)
        m.part('3023b',15,x,14,top-2/3)
        m.part('4282',15,x,15,top-2/3,turn=True)
        m.part('3023b',15,x,31,top-2/3)
    for n in range(0,32,2):
        for x in [0,16]:m.part('4282',15,x,n,top-1/3)
    for h,width in [(0,24),(1,24),(2,20),(3,20),(4,16),(5,16),(6,12)]:
        lo=16-width//2;m.brickwork([(lo,lo+width,lo,lo+width)],top+h,15,turn=bool(h%2))
    # Genuine sloped moulds replace the exposed two-stud rings between steps;
    # their full lower footprints bear on the previous stone course.
    for lo,width,level in [(4,24,2),(6,20,4),(8,16,6)]:
        for x in range(lo,lo+width,4):
            m.part('3037',15,x,lo,top+level)
            m.part('3037',15,x,lo+width-2,top+level,rotation=yaw(math.pi))
        for n in range(lo+2,lo+width-2,4):
            m.part('3037',15,lo,n,top+level,rotation=yaw(-math.pi/2))
            m.part('3037',15,lo+width-2,n,top+level,rotation=yaw(math.pi/2))
    for h in range(7,12):
        if h!=10:m.part('3003',19,15,15,top+h)
    m.part('3001',19,14,15,top+10)
    return m


def round_nave_arcade():
    """Bundled round piers/capitals under the same genuine repeated arch feet."""
    m=Masonry();m.compact_short_rows=True
    for x in range(8,192,8):
        for n in range(0,16,4):
            m.part('3031',19,x-2,n,0)
            m.part('3031',19,x-2,n,1/3)
            m.part('3031',19,x-2,n,2/3)
            for h in range(1,21):m.part('87081',19,x-2,n,h)
            for layer in range(2):m.part('3031',15,x-2,n,21+layer/3)
            for offset in range(4):m.part('2431',15,x-2,n+offset,21+2/3)
    # Boundary feet meet existing narthex/apse masonry without protruding into it.
    for h in range(22):m.brickwork([(0,1,0,16),(191,192,0,16)],h,19,turn=True)
    for x in range(0,192,8):
        for n in range(16):m.part('3308a',19,x,n,22)
    return m


def stone_statue():
    m=Model()
    for part in CONSTRUCTS['minifigure-v2']['parts']:
        position=[part['position'][0]-2,part['position'][1]-1.5,part['position'][2]]
        m.definitions['main'].append(instance('part',part['part'],position,72,
            part.get('rotation') or yaw(part.get('quarter_turns',0)*math.pi/2)))
    return m


def close_apse(m):
    """Actual bearing arcades and plate roofs close the formerly open choir."""
    arcade=arch_bay(12,1)
    for n in range(18,106,8):mount(m,arcade,'choir-west-arcade',241,n,1,yaw(math.pi/2))
    for h in range(15,37):m.brickwork([(240,241,18,110)],h,19,turn=True,phase=4*(h%2))
    for h in range(1,15):m.brickwork([(240,241,106,110)],h,19,turn=True)
    deeper=arch_bay(12,2)
    for n in range(40,88,8):mount(m,deeper,'choir-east-arcade',256,n,1,yaw(math.pi/2))
    for h in range(15,37):m.brickwork([(254,256,40,88)],h,19,turn=True,phase=4*(h%2))
    # Smooth ledges permit a half-stud-centred 14-stud plate span without forcing
    # studs through a wrong receiving grid. This is bearing, not an invented weld.
    for x in [240,254,255]:
        for n in range(18,110,4):m.part('2431',19,x,n,37,turn=True)
    for x in range(240,256,4):m.part('2431',19,x,110,37)
    for x in range(240,256,4):m.part('2431',19,x,110,37+1/3)
    for x in [270,271]:
        for n in range(40,88,4):m.part('2431',19,x,n,37,turn=True)
    for n in range(18,108,6):m.part('3456',72,240.5,n,37+1/3)
    m.part('3456',72,240.5,106,37+2/3)
    for n in range(40,88,2):m.part('4282',72,255,n,37+1/3)


def pointed_roof(m, x0, x1, height, deep=True):
    """Deep, bonded corbels form a closed pointed vault; actual slopes clad its edges."""
    if deep:
        # Full 24-stud transverse plates keep each corbel's load inside its
        # bearing polygon; short inner masonry strips need unmeasured bonds.
        for layer in range(3):
            for x in range(x0,x1,6):
                for n in [22,82]:m.part('3026',19,x,n,height-1+layer/3,turn=True)
                for n in [46,80]:m.part('3795',19,x,n,height-1+layer/3)
    for h in range(16):
        if deep:
            for layer in range(3):
                for x in range(x0,x1,6):
                    for n in [24+h,80-h]:m.part('3026',72,x,n,height+h+layer/3,turn=True)
        else:
            m.bulk((x0,x1,32+h,48+h),height+h,72)
            m.bulk((x0,x1,80-h,96-h),height+h,72)
        if not deep:continue
        x=x0
        while x<x1:
            part,width=('3037',4) if x+4<=x1 else ('3039',2)
            m.part(part,72,x,22+h,height+h)
            m.part(part,72,x,104-h,height+h,rotation=yaw(math.pi))
            x+=width
    for x in range(x0,x1,2):m.part('4282',72,x,56,height+16,turn=True)


def narthex(m):
    # Side doors remain real through-openings below a covered upper vestibule.
    for h in range(1,64):
        holes=[(38,48,n,n+8) for n in [40,80]] if h<15 else []
        m.brickwork([(38,48,32,48),(38,48,80,96)],h,19,holes,turn=bool(h%2),phase=4*(h%2))
    for n in [40,80]:mount(m,arch_bay(12,10),'narthex-side-door',48,n,1,yaw(math.pi/2))
    # Visible round vestibule piers carry the vault's inner springers. The old
    # narrow wall alone cannot counterbalance the complete high pointed roof.
    for n in [48,76]:
        for h in range(1,64):m.part('87081',19,44,n,h)
        for layer in range(2):m.part('3031',15,44,n,64+layer/3)
        for row in range(4):m.part('2431',15,44,n+row,64+2/3)
    # A solid west gable closes the vault end; the rose is actual coloured bricks.
    for h in range(16):
        low,high=48+h,80-h
        glass=[n for n in range(low,high) if ((n+.5-64)/6)**2+((64+h+.5-70)/6)**2<1]
        holes=[(32,38,n,n+1) for n in glass]
        m.brickwork([(32,38,low,high)],64+h,19,holes,turn=True)
        for n in glass:
            color=0 if n in [63,64] or h in [5,6] else [33,36,46,34,47][(n+h)%5]
            for x in range(32,38):m.part('3005',color,x,n,64+h)


def floor_tiles(m, area, structural_height=1):
    blocked=set()
    for leaf in model_leaves(m.export()):
        id=leaf['part'].removesuffix('.dat');matrix=leaf['transform']
        if id in SPECIAL_BOUNDS:bounds=SPECIAL_BOUNDS[id];lo,hi=bounds['min'],bounds['max']
        else:
            w,d,h=PARTS[id];lo,hi=[-w/2,-d/2,-h],[w/2,d/2,0]
        points=[[sum(matrix[i][j]*p[j] for j in range(3))+matrix[i][3] for i in range(3)]
                for p in __import__('itertools').product(*zip(lo,hi))]
        low=[min(p[i] for p in points) for i in range(3)];high=[max(p[i] for p in points) for i in range(3)]
        if low[2]<=structural_height+1e-6 and high[2]>structural_height+1e-6:
            blocked.update((x,n) for x in range(math.floor(low[0]+1e-6),math.ceil(high[0]-1e-6))
                           for n in range(math.floor(low[1]+1e-6),math.ceil(high[1]-1e-6)))
    x0,x1,n0,n1=area
    for n in range(n0,n1,2):
        for x in range(x0,x1,2):
            shade=82 if x==180 else 71 if (x//2+n//2)%2 else 72
            cells={(a,b) for a in range(x,x+2) for b in range(n,n+2)}-blocked
            if len(cells)==4:m.part('3068b',shade,x,n,structural_height)
            else:
                for a,b in sorted(cells):m.part('3070b',shade,a,b,structural_height)


def church():
    m=Masonry();m.compact_short_rows=True
    m.slab([(-16,288,-8,136)],1,71,layers=3)
    for levels,n,name in [(13,0,'south-tower'),(14,96,'north-tower')]:
        mount(m,church_tower(levels),name,16,n,1)
    # Two superposed orders across the west front, opening towards the square.
    portico=arch_bay(25,8,round_columns=True)
    for row,z in enumerate([1,32]):
        for n in range(36,92,8):mount(m,portico,'portico',24,n,z,yaw(math.pi/2))
        for h in range(27):m.brickwork([(16,24,32,36),(16,24,92,96)],z+h,19,turn=bool(h%2))
        for h in range(3):m.brickwork([(16,24,32,96)],z+27+h,15 if h==2 else 19,turn=bool(h%2))
        if row==0:
            for layer in range(3):
                for n in range(32,96,2):
                    m.part('3022',19,16,n,31+layer/3)
                    m.part('4282',19,18,n,31+layer/3)
                    m.part('3020',19,34,n,31+layer/3)
    for h in range(63):
        if h==30:continue # Three supported plate courses occupy this loggia floor.
        holes=[(32,38,n,n+8) for n in [40,60,80]] if h<14 else []
        if 37<=h<55:holes += [(32,38,n,n+8) for n in [44,60,76]]
        m.brickwork([(32,38,32,96)],1+h,19,holes,turn=True,phase=4*(h%2))
    for n in [40,60,80]:mount(m,arch_bay(12,6),'entrance',38,n,1,yaw(math.pi/2))
    for n in [44,60,76]:mount(m,arch_bay(16,6,True),'facade-glass',38,n,38,yaw(math.pi/2))
    # Side chapel walls with deep arched stained-glass windows.
    windows=list(range(56,224,16))
    for h in range(36):
        holes=[(x,x+8,n,n+2) for x in windows for n in [0,126]] if 8<=h<28 else []
        m.brickwork([(48,240,0,2),(48,240,126,128)],1+h,15 if h in [0,34,35] else 19,holes,turn=bool(h%2),phase=4*(h%2))
    pane=arch_bay(18,2,True)
    for x in windows:
        mount(m,pane,'chapel-glass',x,0,9)
        mount(m,pane,'chapel-glass',x,126,9)
    side_arcade=arch_bay(22,2)
    for n in [16,110]:
        for x in range(48,240,8):mount(m,side_arcade,'aisle-arcade',x,n,1)
        for h in range(25,37):m.brickwork([(48,240,n,n+2)],h,19,turn=bool(h%2),phase=4*(h%2))
    # Nave arcades and clerestory: repeated real eight-stud arch moulds.
    arcade=round_nave_arcade()
    high_window=arch_bay(16,16,True)
    for n in [32,80]:
        mount(m,arcade,'round-nave-arcade',48,n,1)
        for h in range(25,64):
            if h in [36,63]:continue
            cuts=[(x,x+8,n,n+16) for x in range(56,232,16)] if 38<=h<56 else []
            m.bulk((48,240,n,n+16),h,15 if h in [25,26,62,63] else 19,cuts,turn=bool(h%2))
        for x in range(56,232,16):mount(m,high_window,'clerestory-glass',x,n,38)
    # Flat aisle roofs use genuine 16-stud beams across fourteen-stud clear spans.
    ledge=Masonry();ledge.brickwork([(0,192,0,18)],0,19,turn=True,phase=0)
    mount(m,ledge,'aisle-bearing-ledge',48,30,36)
    mount(m,ledge,'aisle-bearing-ledge',240,98,36,yaw(math.pi))
    for x in range(48,240,2):
        for n in [1,111]:m.part('4282',72,x,n,37,turn=True)
        for n in [30,96]:m.part('3022',19,x,n,37)
        for n in [16,96]:m.part('4282',72,x,n,37+1/3,turn=True)
    narthex(m)
    pointed_roof(m,32,48,64,deep=False)
    pointed_roof(m,48,240,64)
    # Close the eastern barrel-vault end while retaining an arched sanctuary
    # portal and three high stained-glass lights; fit inside each roof step.
    for h in range(1,80):
        inset=max(0,h-64)
        cuts=[(238,240,60,68)] if h<15 else []
        if 43<=h<61:cuts += [(238,240,n,n+8) for n in [48,60,72]]
        m.brickwork([(238,240,48+inset,80-inset)],h,19,cuts,turn=True,phase=4*(h%2))
    mount(m,arch_bay(12,2),'sanctuary-portal',240,60,1,yaw(math.pi/2))
    for n in [48,60,72]:mount(m,arch_bay(16,2,True),'apse-glass',240,n,43,yaw(math.pi/2))
    # East sanctuary/apse, stepped in plan rather than a scaled curved mould.
    for h in range(36):
        south_end=30 if h==35 else 32;north_start=98 if h==35 else 96
        m.brickwork([(238,240,2,16),(238,240,18,south_end),(238,240,north_start,110),(238,240,112,126),(240,256,16,18),(240,256,110,112),
                      (254,256,18,40),(254,256,88,110),(256,272,38,40),(256,272,88,90),(270,272,40,88)],1+h,19,turn=bool(h%2),phase=4*(h%2))
    close_apse(m)
    statue=stone_statue()
    for n in range(36,96,8):
        m.part('3022',19,18,n,62)
        mount(m,statue,'stone-statue',18.5,n+1,62+1/3,yaw(-math.pi/2))
    # Small supported altar and organ cases inside the axial route.
    for h in range(3):m.brickwork([(216,224,56,72)],1+h,15,turn=bool(h%2))
    for n in [50,70]:
        for h in range(8):m.brickwork([(40,44,n,n+8)],1+h,70,turn=bool(h%2))
        for x in [40,42]:
            for offset in [0,2,4,6]:
                for h in range(5+offset//2):m.part('3062b',80,x,n+offset,9+h)
    # Finishing tiles are absent under every structural footprint and stair.
    floor_tiles(m,(-8,280,0,128))
    pew=Masonry()
    for n in [0,7]:pew.part('3004',70,0,n,0)
    pew.part('3034',70,0,0,1,turn=True)
    pew.part('3008',70,0,0,4/3,turn=True)
    for n in range(8):pew.part('3070b',70,1,n,4/3)
    for x in range(72,192,8):
        for n in [52,68]:mount(m,pew,'nave-pew',x,n,4/3)
    route=[(-8,64),(12,64),(28,64),(36,64),(48,64),(80,64),(112,64),(144,64),(176,64),(204,64),
           (204,24),(188,24),(156,24),(124,24),(92,24),(60,24),(52,24),(52,64),(36,64),(28,64),(12,64)]
    return m,dict(height_bricks=1+14*PITCH+12,unequal_towers=True,arched_chapel_windows=22,
                   clerestory_windows=22,source_arch_mould='3308a.dat',pews=30,walk_height=4/3,walk_start=[-20,64,0],
                   closed_narthex_and_choir=True,pointed_vault=True,roof_slopes=True,round_nave_piers=True,monochrome_statues=8,
                   style='Original Saint-Sulpice interpretation with Notre-Dame 21061 roof/arcade/statue and Hogwarts 71043 sloped-roof inspiration; not a replica'),route


TECHNIC=[(n,id) for n,id in [(16,'3703'),(12,'3895'),(8,'3702'),(6,'3894'),(4,'3701'),(2,'3700'),(1,'6541')] if id+'.dat' in PART_LIBRARY['parts']]


def tech_line(m,length,x,n,z,turn=False,color=28):
    offset=0
    for size,id in TECHNIC:
        while length>=size:
            m.part(id,color,x if turn else x+offset,n+offset if turn else n,z,turn=turn)
            length-=size;offset+=size
    if length:raise ValueError('Unfilled Technic row')


def eiffel():
    m=Masonry();m.compact_short_rows=True
    m.slab([(-8,184,-8,184)],1,71,layers=3)
    cache={}
    # Fixed, fully exposed Technic post lines carry gravity straight down.
    # Dropping outer post lines creates the flared silhouette without hiding
    # solid cores or making inclined joints consume unmeasured clutch strength.
    post_grid=[0,12,24,36,48,60,72,77]
    def frame_course(posts,phase):
        phase=phase if phase>=6 else 0;width=posts[-1]+4
        key=('open-frame',tuple(posts),phase)
        if key not in cache:
            body=Masonry()
            if phase<6:
                for x in posts:
                    for n in posts:
                        for row in range(4):tech_line(body,4,x,n+row,0)
            else:
                def girder_line(x,n,turn):
                    tech_line(body,2,x,n,0,turn)
                    for a,b in zip(posts,posts[1:]):tech_line(body,b-a,x if turn else x+a+2,n+a+2 if turn else n,0,turn)
                    tech_line(body,2,x if turn else x+posts[-1]+2,n+posts[-1]+2 if turn else n,0,turn)
                if phase==6:
                    for post in posts:
                        for x in range(post,post+4):girder_line(x,0,True)
                else:
                    for n in range(width):girder_line(0,n,False)
            cache[key]=body
        return cache[key]
    def four_frame_legs(frames,lo0,lo1,base):
        for level in range(frames):
            t=level/(frames-1);desired_lo=round(lo0+(lo1-lo0)*t)
            posts=[p for p in post_grid if p<=77-desired_lo];width=posts[-1]+4
            for phase in range(8):
                body=frame_course(posts,phase)
                for x,n,angle in [(81,81,math.pi),(95,81,-math.pi/2),(95,95,0),(81,95,math.pi/2)]:
                    mount(m,body,f'frame-{width}-{phase if phase>=6 else 0}',x,n,base+level*8+phase,yaw(angle))
    four_frame_legs(14,0,49,1)
    def deck(low,high,gap0,gap1,z):
        # Grillage: first bridge north/south over the corner piers, then east/west.
        for x in [*range(low,gap0),*range(gap1,high)]:
            tech_line(m,gap0-low-1,x,low,z,True)
            tech_line(m,gap1-gap0+2,x,gap0-1,z,True)
            tech_line(m,high-gap1-1,x,gap1+1,z,True)
        for n in range(low,high):
            tech_line(m,gap0-low-1,low,n,z+1)
            tech_line(m,gap1-gap0+2,gap0-1,n,z+1)
            tech_line(m,high-gap1-1,gap1+1,n,z+1)
    deck(53,123,81,95,113)
    # Keep the inner bearing edge fixed through the second taper. Every upper
    # course is contained in its predecessor rather than cantilevering a heavy
    # observation deck across an unmeasured clutch joint.
    four_frame_legs(15,49,73,115)
    deck(77,99,81,95,235)
    # Upper open-lattice shaft, with real supported cross-girders at each tier.
    z=237
    def girder(width,x,n,z,turn=False):
        if width==18:
            tech_line(m,1,x,n,z,turn)
            tech_line(m,16,x if turn else x+1,n+1 if turn else n,z,turn)
            tech_line(m,1,x if turn else x+17,n+17 if turn else n,z,turn)
        else:tech_line(m,width,x,n,z,turn)
    for width,levels in [(16,9),(16,7),(12,6),(8,5)]:
        low=88-width//2
        for level in range(levels):
            for h in range(9):
                for x in [low,low+width-2]:
                    for n in [low,low+width-2]:
                        tech_line(m,2,x,n,z+h);tech_line(m,2,x,n+1,z+h)
            # Two orthogonal girder layers preserve an open stage below.
            for x in [low,low+1,low+width-2,low+width-1]:girder(width,x,low,z+9,True)
            for n in range(low,low+width):girder(width,low,n,z+10)
            z+=11
        # A solid studded transition supports the next narrower lattice stage.
        m.plate_rectangle(width,width,28,low,low,z);z+=1/3
    # A slender, registered Technic mast makes the tower exceed 550 brick heights.
    for h in range(32):
        for x in [87,88]:tech_line(m,2,x,87,z+h,True)
    height=z+32
    return m,dict(height_bricks=height,technics=True,flared_legs=4,observation_decks=[115,237],walk_height=1,walk_start=[-12,-4,0],
                  reference_model=dict(id='ldraw:81:87',author='Damien Roux [Darats]',license='CCAL version 2.0',adaptation='Original larger Technic-brick frame, informed by the four rotated legs and tiered silhouette; no scaled model parts'),
                  open_upper_lattice=True,open_lower_lattice=True,structural_core='Exposed pin-holed Technic posts and girder grillages throughout; no hidden solid pylons or invented pinned-joint capacities'),[(-4,-4),(180,-4),(180,180),(-4,180),(-4,-4)]


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('kind',choices=SITES);parser.add_argument('--output',type=pathlib.Path,required=True)
    parser.add_argument('--prepare-only',action='store_true');parser.add_argument('--render',action='store_true')
    parser.add_argument('--assets-from',type=pathlib.Path,help='Reuse a current native source-asset cache')
    parser.add_argument('--native-validation',action='store_true',help='Use the identical native static kernel for faster local iterations')
    args=parser.parse_args();args.output.mkdir(parents=True,exist_ok=False);register_profiles()
    local,notes,route=(church if args.kind=='saint-sulpice' else eiffel)()
    site=SITES[args.kind];east=round((site['gps'][1]-GEO['longitude'])*3600*128);north=round((site['gps'][0]-GEO['latitude'])*3600*128)
    origin=[east-site['center'][0],north-site['center'][1],0]
    model=Model();mount(model,local,'landmark',*origin)
    document=model.export();leaves=list(model_leaves(document));parts=sorted({leaf['part'] for leaf in leaves})
    if len(leaves)>100000:raise ValueError('Keep a complete landmark inside the current maximum browser piece budget')
    write(args.output/'model.json',document)
    write(args.output/'proposal.json',dict(published=False,name=site['name'],gps=site['gps'],origin_world=origin,
        coordinate_reference=GEO,pieces=len(leaves),parts=parts,piece_counts=dict(Counter(leaf['part'] for leaf in leaves)),architectural_sources=site['references'],**notes))
    if args.kind=='saint-sulpice':
        write(args.output/'roof-coverage.plan.json',dict(origin_world=origin,rooms=[
            dict(name='narthex',bounds=[38,48,48,80],minimum_roof_height=63.75),
            dict(name='nave',bounds=[48,238,48,80],minimum_roof_height=63.75),
            dict(name='south-aisle',bounds=[48,238,2,32],minimum_roof_height=36.75),
            dict(name='north-aisle',bounds=[48,238,96,126],minimum_roof_height=36.75),
            dict(name='choir',bounds=[241,254,18,110],minimum_roof_height=36.75),
            dict(name='apse',bounds=[256,270,40,88],minimum_roof_height=36.75)]))
    print(json.dumps(dict(model=args.kind,pieces=len(leaves),model_bytes=len(json.dumps(document,separators=(',',':'))),**notes)),flush=True)
    if args.prepare_only:return
    seed=args.assets_from or args.output/'assets.native.json'
    if not args.assets_from:
        client=Mcp(str(ROOT/'target/debug/ergo-mcp'),seed,None,None)
        try:
            for part in parts:client.call('get_catalog',part=part)
            # Persist the locally resolved source assets through an ordinary local
            # update; the seed's sole brick is not part of either landmark.
            client.call('place_brick',part='3005',color=71,min_corner=[0,0,0])
            client.call('export_construction')
        finally:client.close()
    sources=json.loads(seed.read_text())
    if sources.get('format')!='ergo-construction-1' or sources.get('schema')!=2:raise ValueError('Only current native assets are supported')
    missing=set(parts)-sources['meshes'].keys()
    if missing:
        extra=args.output/'extra-assets.native.json';resolver=Mcp(str(ROOT/'target/debug/ergo-mcp'),extra,None,None)
        try:
            for part in sorted(missing):resolver.call('get_catalog',part=part)
            resolver.call('place_brick',part='3005',color=71,min_corner=[0,0,0]);resolver.call('export_construction')
        finally:resolver.close()
        sources['meshes'].update(json.loads(extra.read_text())['meshes'])
    meshes={part:sources['meshes'][part] for part in parts}
    if any(mesh['quality']!='source' or mesh['part']!=part for part,mesh in meshes.items()):raise ValueError('Genuine current source meshes are required')
    native=dict(format='ergo-construction-1',schema=2,name=site['name'],geo=GEO,model=document,pieces=[],meshes=meshes,joints=[],fixed_bricks=[],groups=[],ground=[])
    workspace=args.output/'construction.native.json';write(workspace,native)
    client=None if args.native_validation else Mcp(str(ROOT/'target/debug/ergo-mcp'),workspace,None,None)
    try:
        if args.native_validation:
            checked=subprocess.run([str(ROOT/'target/debug/examples/validate_native'),str(workspace)],capture_output=True,text=True,check=True)
            feedback=json.loads(checked.stdout)
        else:
            assert client is not None
            feedback=client.call('validate_construction')['structuredContent']
        write(args.output/'validation.json',feedback)
        print(json.dumps(feedback),flush=True)
        if client:client.call('export_construction')
        if route:
            walk_height=notes.get('walk_height',4/3)
            walking=dict(format='ergo-walk-route-1',schema=2,origin_world=origin,start=notes.get('walk_start',[route[0][0],route[0][1],walk_height]),eye_height=4,min_clearance=5,speed=6,
                waypoints=[dict(name=f'landmark-route-{i}',feet=[x,n,walk_height],min_clearance=5) for i,(x,n) in enumerate(route)])
            write(args.output/'interior.route.json',walking)
            result=subprocess.run([str(ROOT/'target/debug/examples/check_walk'),str(workspace),str(args.output/'interior.route.json')],capture_output=True,text=True)
            write(args.output/'walking.json',json.loads(result.stdout) if result.returncode==0 else dict(passed=False,error=result.stderr))
        if args.render:
            if client is None:client=Mcp(str(ROOT/'target/debug/ergo-mcp'),workspace,None,None)
            views=[('west-front',[-240,-170,160],[100,64,65]),('nave',[54,64,9],[220,64,16]),('stained-glass',[90,19,12],[132,2,18])] if args.kind=='saint-sulpice' else [('tower',[-650,-850,520],[88,88,280]),('legs',[88,-230,100],[88,88,110])]
            for name,eye,target in views:
                image=client.call('test_visual_appearance',width=1024,height=768,lighting='studio',camera=dict(position_world=[a+b for a,b in zip(eye,origin)],target_world=[a+b for a,b in zip(target,origin)],vertical_fov_degrees=45))
                (args.output/(name+'.png')).write_bytes(base64.b64decode(next(item['data'] for item in image['content'] if item['type']=='image')))
    finally:
        if client:client.close()


if __name__=='__main__':main()
