"""Original walkable monuments, built with nominal standard-size parts.

Technique reference: https://brickbuildershandbook.com/basic-lego-techniques/
Visual references (not replicas):
https://en.wikipedia.org/wiki/Great_Pyramid_of_Giza
https://en.wikipedia.org/wiki/Main_building_of_Moscow_State_University
"""
import math
from scripts.native_model import Model, SPECIAL_BOUNDS, snap_origin, yaw

BASE=2/3
COURSES=9
PITCH=COURSES+2/3
TECHNIQUES=['running-bond','interlocked-corner','interlocked-t-junction','opening-lintels','staggered-slabs','stepped-roof']
TOWER_PARTS=['60594','60592']

def merge(intervals):
    result=[]
    for lo,hi in sorted(intervals):
        if hi<=lo:continue
        if result and lo<=result[-1][1]:result[-1]=(result[-1][0],max(hi,result[-1][1]))
        else:result.append((lo,hi))
    return result

def subtract(intervals,holes):
    for lo,hi in holes:
        intervals=[piece for a,b in intervals for piece in ([(a,b)] if hi<=a or lo>=b else [(a,min(b,lo)),(max(a,hi),b)]) if piece[1]>piece[0]]
    return intervals

def rectangles(areas,holes=()):
    edges=sorted({v for r in [*areas,*holes] for v in r[2:]})
    for n,hi_n in zip(edges,edges[1:]):
        intervals=merge([(a,b) for a,b,c,d in areas if c<=n and hi_n<=d])
        cuts=[(a,b) for a,b,c,d in holes if c<=n and hi_n<=d]
        for x,hi_x in subtract(intervals,cuts):yield x,hi_x,n,hi_n

class Masonry(Model):
    compact_short_rows=False
    def brickwork(self,areas,height,color,holes=(),turn=False,phase=0):
        if turn:
            areas=[(c,d,a,b) for a,b,c,d in areas];holes=[(c,d,a,b) for a,b,c,d in holes]
        for lo,hi,start,end in rectangles(areas,holes):
            n=start
            while n<end:
                depth=min(2,end-n);length=hi-lo
                p=(phase-lo)%8 if length>8 or not self.compact_short_rows else 0
                name=f'bond-{length}-{depth}-{p}-{color}'
                if name not in self.definitions:
                    self.definitions[name]=[];offset=0
                    parts=({8:'3007',6:'2456',4:'3001',3:'3002',2:'3003',1:'3004'} if depth==2 else
                           {8:'3008',6:'3009',4:'3010',2:'3004',1:'3005'})
                    while offset<length:
                        seam=p+8*(math.floor((offset-p)/8)+1)
                        remaining=min(length,seam)-offset
                        size=next(size for size in parts if size<=remaining and (remaining-size!=1 or size==1))
                        self.part(parts[size],color,offset,0,0,turn=depth==2 and size==1,definition=name)
                        offset+=size
                self.reference(name,n+depth if turn else lo,lo if turn else n,height,rotation=yaw(math.pi/2 if turn else 0))
                n+=depth

    def bulk(self,area,height,color,holes=(),turn=False):
        def fill(lo,hi,n,hn):
            w,d=hi-lo,hn-n
            choices=[(pw*pd,int(spin==turn),part,pw,pd,spin) for part,bw,bd in [('4204',16,8),('4201',8,8)]
                for pw,pd,spin in [(bw,bd,False),(bd,bw,True)] if pw<=w and pd<=d]
            if not choices:
                self.brickwork([(lo,hi,n,hn)],height,color,turn=turn,phase=4*int(turn));return
            _,_,part,pw,pd,spin=max(choices)
            self.part(part,color,lo,n,height,turn=spin)
            if lo+pw<hi:fill(lo+pw,hi,n,n+pd)
            if n+pd<hn:fill(lo,hi,n+pd,hn)
        for rectangle in rectangles([area],holes):fill(*rectangle)

    def slab(self,areas,top,color,holes=(),layers=2,close_well=False):
        for layer in range(layers):
            phase=4 if layer==0 else 0
            cuts=list(holes)
            if close_well and layer==1:cuts.append((8,24,4,28))
            for lo,hi,start,end in rectangles(areas,cuts):
                xs=[lo]+[v for v in range(lo+1,hi) if v%8==phase]+[hi]
                ns=[start]+[v for v in range(start+1,end) if v%8==phase]+[end]
                for x,xhi in zip(xs,xs[1:]):
                    for n,nhi in zip(ns,ns[1:]):self.plate_rectangle(xhi-x,nhi-n,color,x,n,top-(layers-layer)/3)
            if close_well and layer==1:
                for n in range(4,28,2):self.part('4282',color,8,n,top-1/3)

def pyramid():
    m=Masonry();m.slab([(-8,72,-8,72)],BASE,28)
    voids=[(28,36,-2,50),(10,54,28,36),(10,22,26,38),(26,38,26,38),(42,54,26,38)]
    for layer in range(32):
        # Two-course/two-stud tiers retain even-stud bearing footprints rather
        # than accidental one-stud corner engagements between heavy infill.
        lo=2*(layer//2);hi=64-lo
        color=14 if layer>=29 else 19 if layer%8 else 28
        height=BASE+layer+(2/3 if layer>=9 else 0)
        holes=voids if layer<9 else []
        if hi-lo<=4:
            m.brickwork([(lo,hi,lo,hi)],height,color,holes,turn=bool(layer%2),phase=4*(layer%2))
        else:
            ring=[(lo,hi,lo,lo+2),(lo,hi,hi-2,hi),(lo,lo+2,lo+2,hi-2),(hi-2,hi,lo+2,hi-2)]
            m.brickwork(ring,height,color,holes,turn=bool(layer%2),phase=4*(layer%2))
            m.bulk((lo+2,hi-2,lo+2,hi-2),height,color,holes,turn=bool(layer%2))
    # Orthogonal bearing beams cover the chambers and intersecting corridors.
    # The centre crossing is bridged at the second plate layer, not suspended.
    z=BASE+9
    for n in [*range(8,24,2),*range(40,56,2)]:
        for x in [8,24,40]:m.part('4282',19,x,n,z)
    for x in [*range(8,28,2),*range(36,56,2)]:m.part('4282',19,x,24,z,turn=True)
    for n in range(8,56,2):
        for x in [8,24,40]:m.part('4282',19,x,n,z+1/3)
    # Entrance pylons and a supported lintel retain full corridor headroom.
    for layer in range(9):
        for x in [24,36]:m.part('3001',14 if layer in [0,8] else 19,x,-2,BASE+layer)
    m.part('4282',14,24,-2,z)
    m.part('4282',14,24,-2,z+1/3)
    m.part('3003',14,31,31,BASE+32+2/3)
    return m

def pyramid_route(origin):
    points=[(32,12,'entrance corridor'),(32,24,'central approach'),(32,32,'central chamber'),
        (16,32,'west chamber'),(12,32,'west chamber far end'),(32,32,'return crossing'),
        (48,32,'east chamber'),(52,32,'east chamber far end'),(32,32,'return central chamber'),
        (32,47,'north corridor'),(32,32,'return crossing'),(32,-5,'exit pyramid')]
    return route(origin,(32,-5,BASE),[(x,n,BASE,name) for x,n,name in points],9)

def tower_walls(m,wings,course,height,color,ground,extra_holes=(),window_pattern=None):
    def ring(lo,hi,south,north,holes):
        # Three aligned header courses retain full engagements over openings;
        # the lower running bond and staggered slabs strap these beam segments.
        headers=course>=6
        a,b=(lo,hi) if headers or course%2==0 else (lo+2,hi-2)
        for n in [south,north-2]:m.brickwork([(a,b,n,n+2)],height,color,holes,phase=lo+4 if headers else 4*(course%2))
        a,b=(south+2,north-2) if headers or course%2==0 else (south,north)
        for x in [lo,hi-2]:m.brickwork([(x,x+2,a,b)],height,color,holes,turn=True,phase=4 if headers else 4*(course%2))
    def main_holes(layer):
        holes=[]
        if window_pattern is None:
            if 3<=layer<6:
                for x in [6,14,22]:
                    holes.append((x,x+4,0,2))
                    holes.append((x,x+4,30,32))
                for n in [6,14,22]:holes += [(0,2,n,n+4),(30,32,n,n+4)]
        else:
            for part,x,bottom in window_pattern:
                box=SPECIAL_BOUNDS[part];width=round(box['max'][0]-box['min'][0]);height=round(box['max'][2]-box['min'][2])
                if bottom<=layer<bottom+height:holes += [(x,x+width,0,2),(x,x+width,30,32),(0,2,x,x+width),(30,32,x,x+width)]
        if ground and layer<6:holes.append((14,18,0,2))
        if wings and layer<6:holes += [(0,2,14,18),(30,32,14,18)]
        return holes
    ring(0,32,0,32,[*main_holes(course),*extra_holes])
    # Interior piers keep their full 4x2 footprint; the floor layers strap them
    # into the bonded perimeter. Do not fragment every pier into tiny end parts.
    for x in [8,24]:m.part('3001',color,x-2,22,height)
    m.part('3001',color,14,22,height)
    if wings:
        for lo,hi in [(-18,-2),(34,50)]:
            def wing_holes(layer):
                cuts=[]
                if 3<=layer<6:
                    cuts += [(lo+6,lo+10,6,8),(lo+6,lo+10,24,26)]
                    for x in [lo,hi-2]:cuts.append((x,x+2,14,18))
                if layer<6:cuts.append((hi-2,hi,14,18) if lo<0 else (lo,lo+2,14,18))
                return cuts
            ring(lo,hi,6,26,wing_holes(course))
        for x in [-8,40]:m.part('3001',color,x-2,15,height)

def stair(m,base,flights=True):
    # Open treads bear on notched full-height masonry stringers. Solid wedges
    # repeated on each floor would block the flight below or float in its void.
    walls=[(6,10,4,20),(14,18,4,16),(22,26,4,20)]
    beams=[]
    if flights:
        beams.append((8,16,6,8,0,'3034'))
        beams += [(8,16,6+2*i,8+2*i,3*i-1,'3034') for i in range(1,5)]
        beams += [(8,24,n,n+2,11,'4282') for n in [16,18]]
        beams += [(16,24,16-2*(i-4),18-2*(i-4),3*i-1,'3034') for i in range(5,10)]
    for course in range(9):
        local=[b for b in beams if b[4]//3==course]
        cuts=[]
        for a,b,c,d in walls:
            for x,hi,n,hn,_,_ in local:
                r=(max(a,x),min(b,hi),max(c,n),min(d,hn))
                if r[0]<r[1] and r[2]<r[3]:cuts.append(r)
        m.brickwork(walls,base+course,19,cuts,turn=True,phase=4*(course%2))
        for plate in range(3):
            occupied=[b[:4] for b in local if b[4]%3==plate]
            for x,hi,n,hn in rectangles(cuts,occupied):m.plate_rectangle(hi-x,hn-n,19,x,n,base+course+plate/3)
    for x,_,n,_,plate,part in beams:m.part(part,71,x,n,base+plate/3)

def tower_window_pattern(level):
    if level<3:return [(part,x+offset,bottom) for x in [6,14,22] for part,offset,bottom in [('60594',0,2),('60592',0,5),('60592',2,5)]]
    if level<7:return [('60594',x,3) for x in [6,14,22]]
    return [('60594',x,bottom) for x in [6,14,22] for bottom in [1,4]]

def tower_wall_colour(level,course):
    if level==0:return 72 if course<2 else 71 if course<8 else 15
    if level in [3,6] and course<2:return 28 if course==0 else 15
    if level==9 and course==7:return 14
    if course==8:return 15
    if level>=7:return 19 if course==0 else 15
    return 28 if level in [2,5] and course==0 else 19

def tower_frames(m,level,base):
    colour=72 if level==0 else 28 if level>=7 else 15
    for part,x,bottom in tower_window_pattern(level):
        if not (level==0 and 14<=x<18):
            m.part(part,colour,x,1,base+bottom)
            m.part(part,colour,1,x,base+bottom,rotation=yaw(math.pi/2))
            m.part(part,colour,30,x,base+bottom,rotation=yaw(-math.pi/2))
        m.part(part,colour,x,30,base+bottom,rotation=yaw(math.pi))
    if level==0:
        for lo in [-18,34]:
            m.part('60594',15,lo+6,7,base+3)
            m.part('60594',15,lo+6,24,base+3,rotation=yaw(math.pi))
            m.part('60594',15,lo+1 if lo<0 else lo+14,14,base+3,rotation=yaw(math.pi/2 if lo<0 else -math.pi/2))

def tower(facade=True):
    m=Masonry();m.compact_short_rows=True;m.slab([(-24,56,-8,40)],BASE,71)
    main=[(0,32,0,32)]
    well=[(10,14,6,16),(18,22,6,16),(10,22,16,20)]
    for level in range(10):
        base=BASE+level*PITCH;wings=level<1
        if level:m.slab(main,base,15,well)
        if level==1:
            for lo in [-18,34]:
                for n in range(6,26,2):m.part('4282',15,lo,n,base-2/3)
                for x in range(lo,lo+16,2):
                    m.part('4282',15,x,6,base-1/3,turn=True)
                    m.part('3020',15,x,22,base-1/3,turn=True)
        for course in range(COURSES):
            color=tower_wall_colour(level,course) if facade else 15 if course==8 else 71 if level==0 and course<2 else 19
            tower_walls(m,wings,course,base+course,color,level==0,window_pattern=tower_window_pattern(level) if facade else None)
        if facade:tower_frames(m,level,base)
        stair(m,base,level<9)
    top=BASE+10*PITCH
    # Close the stairwell with real supported beams before adding the crown.
    m.slab(main,top,15,well,close_well=True)
    for stage,width in enumerate([20,12,8]):
        lo=16-width//2
        for course in range(3):m.brickwork([(lo,lo+width,lo,lo+width)],top+3*stage+course,15 if course==2 else 19,turn=bool(course%2),phase=4*(course%2))
    z=top+9
    for stage,width in enumerate([8,6,4,2]):
        lo=16-width//2
        for course in range(3):m.brickwork([(lo,lo+width,lo,lo+width)],z+3*stage+course,320,turn=bool(course%2))
    for course in range(3):m.part('3003',14,15,15,z+12+course)
    # Paired lower rooftop turrets echo the source's subsidiary towers.
    wing_top=BASE+PITCH
    for x in [-12,44]:
        for course in range(4):m.plate_rectangle(8,8,15,x-4,12,wing_top+course/3)
        for course in range(3):m.part('3031',320,x-2,14,wing_top+4/3+course/3)
        m.part('3003',14,x-1,15,wing_top+7/3)
    return m

def tower_route(origin,storeys=10):
    points=[(16,3,BASE,'ground entrance')]
    for level in range(storeys):
        base=BASE+level*PITCH
        if level:
            points += [(12,18,BASE+(level-1)*PITCH+4,f'level {level} first flight'),
                       (20,18,BASE+(level-1)*PITCH+4,f'level {level} half landing'),
                       (20,3,base,f'level {level+1} stair arrival')]
        points += [(4,3,base,f'level {level+1} front hall'),(4,16,base,f'level {level+1} west room'),
                   (4,28,base,f'level {level+1} rear passage'),(28,28,base,f'level {level+1} east room'),
                   (28,3,base,f'level {level+1} return hall'),(12,3,base,f'level {level+1} stair entrance')]
    for level in reversed(range(storeys-1)):
        base=BASE+level*PITCH
        points += [(20,3,base+PITCH,f'descend from level {level+2}'),(20,18,base+4,'descending half landing'),
                   (12,18,base+4,'descending first flight'),(12,3,base,f'back to level {level+1}')]
    points += [(16,3,BASE,'ground lobby'),(16,-5,BASE,'exit tower')]
    return route(origin,(16,-5,BASE),points,5.5)

def route(origin,start,points,clearance):
    return {'format':'ergo-walk-route-1','schema':2,'origin_world':snap_origin(origin),'start':list(start),
        'eye_height':4,'min_clearance':clearance,'speed':6,
        'waypoints':[{'name':name,'feet':[x,n,z],'min_clearance':9 if ('room' in name or 'hall' in name) else clearance} for x,n,z,name in points]}
