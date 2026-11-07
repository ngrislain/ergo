#!/usr/bin/env python3
"""Articulated Technic Eiffel Tower: free-angle beams on cross axles in round bearings,
pinned and braced into a rigid truss; genuine parts, no
implicit publication (scripts/publish-landmark.py validates and publishes)."""
import argparse
import json
import math
import pathlib
import sys
from typing import Any

ROOT=pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT))
from scripts.native_model import Model,instance,model_leaves,yaw,canonical_part
from scripts.build.paris_landmarks import GEO,SITES,register_profiles
from scripts.mcp_client import Mcp

UP=(0.,0.,1.)
BEAMS={3:'32523',5:'32316',7:'32524',9:'40490',11:'32525',13:'41239',15:'32278'}
AXLES={2:'3704',3:'4519',4:'3705',5:'32073',6:'3706',7:'44294',8:'3707',10:'3737',12:'3708'}
def add(a,b):return tuple(x+y for x,y in zip(a,b))
def sub(a,b):return tuple(x-y for x,y in zip(a,b))
def times(a,s):return tuple(x*s for x in a)
def dot(a,b):return sum(x*y for x,y in zip(a,b))
def cross(a,b):return (a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0])
def length(a):return math.sqrt(dot(a,a))
def columns(*vectors):return [[vector[row] for vector in vectors] for row in range(3)]


class Truss:
    def __init__(self):
        self.model=Model();self.hardware={};self.members=[]

    def part(self,part,p,rotation,color=28):
        # Coordinates here are isotropic 8-mm stud units. instance converts the
        # proper rotation to anisotropic public brick units without scaling parts.
        self.model.definitions['main'].append(instance('part',canonical_part(part)+'.dat',[p[0],p[1],p[2]*5/6],color,rotation))

    def joint(self,p,axis,layers,primary=False):
        key=tuple(round(v,8) for v in [*p,*axis])
        entry=self.hardware.setdefault(key,dict(p=p,axis=axis,layers=set(),primary=False))
        entry['layers'].update(layers);entry['primary'] |= primary

    def member(self,p0,p1,axis,base,label):
        vector=sub(p1,p0);span=round(length(vector))
        if span<2 or abs(length(vector)-span)>1e-6:
            raise ValueError('Member endpoints must fit genuine integer hole spacing')
        direction=times(vector,1/span)
        if abs(dot(direction,axis))>1e-8:raise ValueError('Beam holes must be transverse')
        rotation=columns(cross(direction,axis),direction,axis)
        for lead in [14,12,10,8,6]:
            starts=[0]
            while starts[-1]+(lead if len(starts)==1 else 14)<span:
                maximum=(lead if len(starts)==1 else 14)-2
                # Three shared holes leave two studs between splice axles, so
                # real bushes do not strike each other at inclined members.
                advance=min(maximum,span-starts[-1]-8)
                if advance<1:raise ValueError('Cannot splice this member without endpoint interference')
                starts.append(starts[-1]+advance)
            if label!='horizontal-cross-tie' or all(abs(start+offset-span/2)>=2 for start in starts[1:] for offset in [0,2]):break
        else:raise ValueError('No splice phase clears the central bearing housing')
        for i,start in enumerate(starts):
            remaining=span-start
            size=lead+1 if i==0 and remaining>=lead else 15 if remaining>=14 else next(n for n in BEAMS if n-1>=remaining)
            centre=add(add(p0,times(direction,start+(size-1)/2)),times(axis,base+i%2))
            self.part(BEAMS[size],centre,rotation)
            if i:
                for offset in [0,2]:self.joint(add(p0,times(direction,start+offset)),axis,[base+(i-1)%2,base+i%2])
        self.joint(p0,axis,[base],True)
        self.joint(p1,axis,[base+(len(starts)-1)%2],True)
        if label=='horizontal-cross-tie':
            middle=span/2
            index=max(i for i,start in enumerate(starts) if start<=middle)
            self.joint(add(p0,times(direction,middle)),axis,[base+index%2],True)
        self.members.append(dict(kind=label,span_studs=span,beams=len(starts)))

    def hub(self,p,radial,layer=0):
        # A bearing brick on a node axle. Its centre of mass lies just below
        # the hole, so a central one hangs at rest without a stud connection.
        axis=cross(UP,radial)
        rotation=columns(radial,axis,UP)
        self.part('3700',add(add(p,times(axis,layer)),times(UP,.5)),rotation,color=72)
        self.joint(p,axis,[layer],True)

    def footing(self,foot,radial,courses):
        """Bearing bricks on every free layer of a foot axle, resting on one
        foundation block: loads entering the axle along its whole length
        reach the ground directly instead of bending a one-stud bearing."""
        axis=cross(UP,radial)
        entry=self.hardware[tuple(round(v,8) for v in [*foot,*axis])]
        low,high=min(entry['layers']),max(entry['layers'])
        free=[layer for layer in range(low,high+1) if layer not in entry['layers']]
        for layer in sorted(set(free)|{0}):self.hub(foot,radial,layer)
        corners=[add(foot,add(times(radial,r),times(axis,l))) for r in [-2,2] for l in [low-.5,high+.5]]
        lo=[math.floor(min(c[i] for c in corners)+1e-9) for i in range(2)]
        hi=[math.ceil(max(c[i] for c in corners)-1e-9) for i in range(2)]
        hi=[h+(h-l)%2 for l,h in zip(lo,hi)]
        for h in range(courses):
            for x in range(lo[0],hi[0],2):
                for n in range(lo[1],hi[1],2):self.model.part('3003',72,x,n,h)
        # Plates bring the block up to the bearing bricks' undersides.
        bottom=(foot[2]-.7)*5/6
        h=courses
        while h<bottom-1e-9:
            self.model.plate_rectangle(hi[0]-lo[0],hi[1]-lo[1],72,lo[0],lo[1],h);h+=1/3
        if abs(h-bottom)>1e-6:raise ValueError('Foundation does not reach the bearing bricks')

    def finish(self):
        for entry in self.hardware.values():
            low,high=min(entry['layers']),max(entry['layers'])
            half=entry['primary'];width=.5 if half else 1.
            span=high-low+1+2*width
            size=next((n for n in AXLES if n>=span-1e-8),None)
            if size is None:raise ValueError('Node requires an unselected longer axle')
            p,axis=entry['p'],entry['axis']
            self.part(AXLES[size],add(p,times(axis,(low+high)/2)),columns(axis,cross(UP,axis),UP),color=0)
            for offset in [low-.5-width/2,high+.5+width/2]:
                self.part('4265c' if half else '3713',add(p,times(axis,offset)),columns(UP,axis,cross(UP,axis)),color=71)


def build():
    # Align the foundation bearing bricks with the actual baseplate stud grid.
    # The four inclined legs remain in two orthogonal planes; rotating their
    # smooth plinths by 45 degrees did not create a registered stud connection.
    tower=Truss();root=(88.,88.5,3.5)
    r=(1.,0.,0.);a=cross(UP,r)
    widths=[336,168,70,14]
    leg_lengths=[196,196,244]
    diagonal_lengths=[308,224,246]
    heights=[root[2]]
    for b,t,l,d in zip(widths,widths[1:],leg_lengths,diagonal_lengths):
        run=(b-t)/2;rise=math.sqrt(l*l-run*run)
        if abs(math.hypot((b+t)/2,rise)-d)>1e-7:raise ValueError('Truss diagonal is off the hole grid')
        heights.append(heights[-1]+rise)
    for plane,radial in enumerate([r,a]):
        axis=cross(UP,radial)
        centre=root if plane==0 else add(root,add(times(r,.5),times(a,.5)))
        offset=0. if plane==0 else 1.6
        vertices=[]
        for width,height in zip(widths,heights):
            pair=[add((centre[0],centre[1],height+offset),times(radial,sign*width/2)) for sign in [-1,1]]
            vertices.append(pair)
            tower.member(pair[0],pair[1],axis,-2,'horizontal-cross-tie')
            tower.hub((centre[0],centre[1],height+offset),radial)
        for stage in range(3):
            for side in range(2):tower.member(vertices[stage][side],vertices[stage+1][side],axis,1 if stage%2==0 else 3,'inclined-pillar')
            tower.member(vertices[stage][0],vertices[stage+1][1],axis,5,'cross-brace')
            tower.member(vertices[stage][1],vertices[stage+1][0],axis,-4,'cross-brace')
        for foot in vertices[0]:tower.footing(foot,radial,2 if plane==0 else 3)
    tower.finish()
    angles=[math.degrees(math.asin((heights[i+1]-heights[i])/leg_lengths[i])) for i in range(3)]
    return tower,dict(height_bricks=(heights[-1]+2.6)*5/6,floor_heights_bricks=[h*5/6 for h in heights[1:3]],
        pillar_angles_degrees=angles,horizontal_cross_widths_studs=widths,main_pillars=4,
        beam_splices='Two freely rotating axles at distinct holes rigidly splice each long member geometrically',
        node_connections='Round-hole bearings permit axle rotation; keyed half/full bushes retain the axles',
        footings='Bearing bricks on every free layer of each foot axle rest on one foundation block',
        joint_capacities='Reviewed connector estimates (estimate:ergo-connector-review-v1), not laboratory measurements',
        member_counts=dict(__import__('collections').Counter(m['kind'] for m in tower.members)))


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=pathlib.Path,required=True)
    parser.add_argument('--assets-from',type=pathlib.Path,required=True)
    args=parser.parse_args();args.output.mkdir(parents=True,exist_ok=False);register_profiles()
    tower,notes=build();site=SITES['eiffel']
    east=round((site['gps'][1]-GEO['longitude'])*3600*128);north=round((site['gps'][0]-GEO['latitude'])*3600*128)
    origin=[east-88,north-88,0];model=tower.model.export(origin)
    parts={leaf['part'] for leaf in model_leaves(model)}
    assets=json.loads(args.assets_from.read_text())['meshes']
    missing=parts-assets.keys()
    if missing:
        extra=args.output/'extra-assets.native.json';client=Mcp(str(ROOT/'target/debug/ergo-mcp'),extra,None,None)
        try:
            for part in sorted(missing):client.call('get_catalog',part=part)
            client.call('place_brick',part='3005',color=72,min_corner=[0,0,0]);client.call('export_construction')
        finally:client.close()
        assets.update(json.loads(extra.read_text())['meshes'])
    native:dict[str,Any]=dict(format='ergo-construction-1',schema=2,name='Tour Eiffel — Prototype de treillis articulé',geo=GEO,
        model=model,pieces=[],meshes={part:assets[part] for part in parts},joints=[],fixed_bricks=[],groups=[],ground=[])
    if any(mesh['quality']!='source' for mesh in native['meshes'].values()):raise ValueError('Genuine source assets are required')
    (args.output/'construction.native.json').write_text(json.dumps(native,separators=(',',':'))+'\n')
    proposal=dict(published=False,origin_world=origin,pieces=sum(1 for _ in model_leaves(model)),**notes)
    (args.output/'proposal.json').write_text(json.dumps(proposal,indent=2)+'\n')
    print(json.dumps(proposal),flush=True)


if __name__=='__main__':main()
