"""Minifigure-scale alpine houses using attributed, standard-size studded parts.

Patterns: https://brickbuildershandbook.com/basic-lego-techniques/
Running bond, interlocked L/T junctions, supported lintels, staggered slabs and
connected stepped roof courses. The plate-rise roof is a shallow-pitch adaptation.
"""
import math
from typing import Optional
from native_model import Model,yaw,snap_origin

WIDTH=24
DEPTH=16
COURSES=9
BASE=2/3
UPPER=BASE+COURSES+2/3
ROOF=UPPER+COURSES+2/3
TECHNIQUES=['running-bond','interlocked-corner','interlocked-t-junction','opening-lintels','staggered-slabs','stepped-roof']

def subtract(intervals,holes):
    for lo,hi in holes:
        intervals=[piece for a,b in intervals for piece in ([(a,b)] if hi<=a or lo>=b else [(a,min(b,lo)),(max(a,hi),b)]) if piece[1]>piece[0]]
    return intervals

def run(model,start,end,fixed,height,color,turn=False,phase:Optional[int]=0,plates=False,thin=False,holes=(),lintels=()):
    intervals=subtract([(start,end)],holes)
    for lo,hi in lintels:
        if any(a<=lo and hi<=b for a,b in intervals):
            run(model,lo,hi,fixed,height,color,turn,0,plates,thin)
            intervals=subtract(intervals,[(lo,hi)])
    parts=({16:'4282',8:'3034',6:'3795',4:'3020',3:'3021',2:'3022',1:'3023'} if plates else
           {8:'3008',6:'3009',4:'3010',2:'3004',1:'3005'} if thin else
           {8:'3007',6:'2456',4:'3001',3:'3002',2:'3003',1:'3004'})
    for a,b in intervals:
        while a<b:
            next_seam=b if phase is None else phase+8*(math.floor((a-phase)/8)+1)
            remaining=min(b,next_seam)-a
            size=next(size for size in parts if size<=remaining and (remaining-size!=1 or size==1))
            extra=(not thin and size==1)
            angle=(int(turn)+int(extra))*math.pi/2
            model.part(parts[size],color,fixed if turn else a,a if turn else fixed,height,rotation=yaw(angle))
            a+=size

def slab(model,top,lo_x,hi_x,lo_n,hi_n,color,landing=False,layers=2):
    for layer in range(layers):
        def cuts(lo,hi):return [lo]+list(range(lo+(8 if layer==0 else 4),hi,8))+[hi]
        xs=cuts(lo_x,hi_x);ns=cuts(lo_n,hi_n)
        for x,xhi in zip(xs,xs[1:]):
            for n,nhi in zip(ns,ns[1:]):
                cut=(max(x,20),min(xhi,24),max(n,10),min(nhi,14)) if landing else None
                if cut and cut[0]<cut[1] and cut[2]<cut[3]:
                    a,b,c,d=cut
                    rects=[(x,n,a-x,nhi-n),(b,n,xhi-b,nhi-n),(a,n,b-a,c-n),(a,d,b-a,nhi-d)]
                else:rects=[(x,n,xhi-x,nhi-n)]
                for px,pn,w,d in rects:model.plate_rectangle(w,d,color,px,pn,top-(layers-layer)/3)
        if landing:model.part('3035',color,20,10,top-(layers-layer)/3)

def chalet(wall,timber,roof,accent):
    m=Model();slab(m,BASE,-2,30,-10,18,71)
    for storey,base in enumerate([BASE,UPPER]):
        if storey:slab(m,base,0,WIDTH,0,DEPTH,timber,landing=True)
        for layer in range(COURSES):
            z=base+layer;even=layer%2==0;phase=0 if even else 4
            color=71 if storey==0 and layer<2 else timber if storey or layer in [6,8] else wall
            end_start,end_end=(0,WIDTH) if even else (2,WIDTH-2)
            windows=[(2,6),(18,22)] if 3<=layer<6 else []
            for n in [0,DEPTH-2]:
                holes=windows+([(10,14)] if n==0 and layer<6 else [])
                run(m,end_start,end_end,n,z,color,phase=phase,holes=holes)
            side_intervals=[(2,7),(9,14)] if even else [(0,DEPTH)]
            for x in [0,WIDTH-2]:
                holes=([(3,5)] if 3<=layer<6 else [])+([(10,14)] if storey and x==WIDTH-2 and layer<7 else [])
                lintels=[(2,6)] if layer==6 else [(8,16)] if storey and x==WIDTH-2 and layer==7 else []
                for a,b in side_intervals:run(m,a,b,x,z,color,True,phase,holes=holes,lintels=lintels)
            run(m,0 if even else 2,WIDTH if even else WIDTH-2,7,z,color,phase=phase,
                holes=[(10,14)] if layer<6 else [])
    # Freestanding porch piers are bonded to the shared foundation. Two
    # overlapping deck layers strap the piers together at the upper doorway.
    for x in [4,18]:
        for layer in range(COURSES):m.part('3001',timber,x,-4,BASE+layer,turn=True)
    for n in [-4,-2]:m.part('4282',timber,4,n,UPPER-2/3)
    for x in [4,12]:m.part('3035',timber,x,-4,UPPER-1/3)
    for layer in range(2):
        for a,b,n in ([(4,20,-4)] if layer==0 else [(5,19,-4)]):run(m,a,b,n,UPPER+layer,accent,thin=True,phase=4*layer)
        for x in [4,19]:run(m,-3 if layer==0 else -4,0,x,UPPER+layer,accent,True,thin=True)
    # A full-width, fully supported external stair reaches the side door.
    for step in range(1,10):
        n=-8+2*(step-1)
        for layer in range(step):m.part('3001',timber,25,n,BASE+layer)
        m.part('3020',accent,25,n,BASE+step)
    for layer in range(COURSES):
        for n in [10,12]:m.part('3001',timber,25,n,BASE+layer)
    slab(m,ROOF,0,WIDTH,0,DEPTH,timber)
    # Four-stud-deep stepped gables provide bearing beneath both halves of
    # each roof strip. Two-stud gables leave an eccentric loaded seam whose
    # accumulated weight falls outside the compressive support polygon.
    # The roof courses overlap by one stud and stagger longitudinal seams.
    for n in [0,12]:
        for layer in range(11):
            a=layer+1;b=WIDTH-layer-1
            intervals=subtract([(a,b)],[(10,14)] if 1<=layer<5 else [])
            if layer==5:
                intervals=[(8,16)]+subtract(intervals,[(8,16)])
            for lo,hi in intervals:
                m.plate_rectangle(hi-lo,4,timber if layer in [0,5,10] else wall,lo,n,ROOF+layer/3)
    for layer in range(12):
        for x in [-1+layer,WIDTH-1-layer]:
            # Both end pieces extend well inside their supporting gable;
            # no short eave piece balances outside its bearing polygon.
            # Alternating 10+8 / 8+10 bonds the longitudinal seam.
            lengths=[10,8] if layer%2==0 else [8,10]
            n=-1
            for length in lengths:
                part={10:'3832',8:'3034'}[length]
                m.part(part,roof,x,n,ROOF+layer/3,turn=True);n+=length
    n=-1
    for part,length in [('3035',8),('3032',6),('3031',4)]:
        m.part(part,roof,10,n,ROOF+4,turn=True);n+=length
    m.part('3832',roof,11,-1,ROOF+4+1/3,turn=True)
    m.part('3034',roof,11,9,ROOF+4+1/3,turn=True)
    for x in [0,20]:
        m.part('3001',288,x,-4,BASE)
        m.part('3024',4 if x==0 else 14,x+1,-3,BASE+1)
    return m

def walk_route(origin,facing_north=False):
    origin=snap_origin(origin)
    points=[(12,4,BASE,'ground front room'),(12,12,BASE,'ground rear room'),(12,4,BASE,'return foyer'),
        (12,-9,BASE,'front terrace'),(27,-9,BASE,'stair entrance'),(27,12,UPPER,'stair landing'),
        (18,12,UPPER,'upper rear room'),(12,12,UPPER,'upper passage'),(12,4,UPPER,'upper front room'),
        (12,-2,UPPER,'balcony'),(12,12,UPPER,'return upper passage'),(27,12,UPPER,'return stair landing'),
        (27,-9,BASE,'descend stairs'),(12,-9,BASE,'exit terrace')]
    def turn(p):
        x,n,z=p;return [WIDTH-x,DEPTH-n,z] if facing_north else [x,n,z]
    return {'format':'ergo-walk-route-1','schema':2,'origin_world':origin,'start':turn((12,-9,BASE)),
        'eye_height':4,'min_clearance':5.5,'speed':6,
        'waypoints':[{'name':name,'feet':turn((x,n,z)),'min_clearance':9} for x,n,z,name in points]}

def landscape():
    m=Model()
    for n in [30,40]:m.plate_rectangle(152,2,71,-6,n,0)
    # Reusable brick-built pines use centred studded canopies rather than loose
    # smooth-tile leaves. Landscape stays clear of every house foundation.
    for x in [4,36,76,116,144]:
        n=35
        for layer in range(2):m.part('3003',70,x,n,layer)
        m.part('2456',288,x-2,n,2);m.part('3001',2,x-1,n,3);m.part('3003',288,x,n,4);m.part('3005',2,x,n,5)
    for x in [20,60,100,132]:
        m.part('3001',70,x,35,0)
        m.part('3022',4,x,35,1);m.part('3022',14,x+2,35,1)
    return m
