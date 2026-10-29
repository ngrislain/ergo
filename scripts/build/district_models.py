"""Original minifigure-scale district, adapting the accepted tower recipe.

Bond/floor source: https://brickbuildershandbook.com/basic-lego-techniques/
"""
from scripts.build.monument_models import BASE, COURSES, PITCH, Masonry, rectangles, stair, tower_route, tower_walls
from scripts.native_model import yaw
import math

CORE_STAGES = ['foundation', *[f'floor-{level:02}' for level in range(1, 11)], 'roof']
STAGES = [*CORE_STAGES, *[f'exterior-{level:02}' for level in range(1, 11)], 'columns', 'windows', 'roof-details','terraces']

CENTRE_TOWER = '88c288ddd45cda14e5b51cbbd116f5243610b1b08f883923ce0dabda050deb2f'
ORIGIN = (19968, 64, 0)
SPECS = [
    {'key': '01-azure-terrace', 'name': 'Azure Terrace Pyramid', 'cell': (-2, -1), 'kind': 'pyramid', 'wall': 321, 'trim': 15, 'storeys':8},
    {'key': '02-copper-needle', 'name': 'Copper Needle Tower', 'cell': (-1, -1), 'kind': 'tower', 'wall': 320, 'trim': 14, 'storeys':10},
    {'key': '03-white-slab', 'name': 'White Slab House', 'cell': (0, -1), 'kind': 'slab', 'wall': 15, 'trim': 72, 'storeys':8, 'wing_storeys':[4,6]},
    {'key': '04-emerald-court', 'name': 'Emerald Courtyard Tower', 'cell': (1, -1), 'kind': 'court', 'wall': 288, 'trim': 19, 'storeys':5},
    {'key': '05-ochre-cascade', 'name': 'Ochre Cascade Pyramid', 'cell': (-2, 0), 'kind': 'pyramid', 'wall': 19, 'trim': 28, 'storeys':7},
    {'key': '06-civic-bell', 'name': 'Civic Bell Tower', 'cell': (-1, 0), 'kind': 'tower', 'wall': 71, 'trim': 15, 'storeys':9},
    {'key': '07-crimson-gallery', 'name': 'Crimson Gallery House', 'cell': (1, 0), 'kind': 'slab', 'wall': 320, 'trim': 19, 'storeys':6, 'wing_storeys':[2,4]},
    {'key': '08-sandstone-ziggurat', 'name': 'Sandstone Ziggurat', 'cell': (-2, 1), 'kind': 'pyramid', 'wall': 28, 'trim': 14, 'storeys':9},
    {'key': '09-colonnade-exchange', 'name': 'Colonnade Exchange', 'cell': (-1, 1), 'kind': 'columns', 'wall': 15, 'trim': 19, 'storeys':7},
    {'key': '10-north-arch', 'name': 'North Arch Court', 'cell': (0, 1), 'kind': 'slab', 'wall': 70, 'trim': 15, 'storeys':8, 'wing_storeys':[3,5]},
    {'key': '11-silver-observatory', 'name': 'Silver Observatory', 'cell': (1, 1), 'kind': 'round', 'wall': 72, 'trim': 71, 'storeys':6},
]
assert len(SPECS) == 11 and len({spec['cell'] for spec in SPECS}) == 11


def stages(spec):
    count=storeys(spec)
    extra = [f'exterior-{level:02}' for level in range(1, min(count,9)+1 if spec['kind']=='pyramid' else count+1)] if spec['kind'] in ['pyramid', 'slab'] else ['columns'] if spec['kind'] in ['court','columns','round'] else []
    return ['foundation',*[f'floor-{level:02}' for level in range(1,count+1)],'roof', *extra, 'windows', 'roof-details', *(['terraces'] if spec.get('wing_storeys') else [])]

def storeys(spec):return spec.get('storeys',10)
def wing_storeys(spec):return spec.get('wing_storeys',[storeys(spec),storeys(spec)])


def required_parts(name):
    if name == 'windows': return ['60594']
    if name == 'columns': return ['87081']
    if name == 'roof-details': return ['3037','3039','3633','3062b','3941','87580','4740']
    if name == 'terraces': return ['3633']
    return []


def origin(spec):
    east, north = spec['cell']
    return [ORIGIN[0] + east * 96, ORIGIN[1] + north * 96, ORIGIN[2]]


def walking_route(spec):
    """Visit every retained storey and accessible intermediate terrace."""
    route = tower_route(origin(spec),storeys(spec))
    if spec.get('wing_storeys'):
        points=[]
        for point in route['waypoints']:
            points.append(point)
            for side,height in enumerate(wing_storeys(spec)):
                marker=f'level {height+1} '+('west room' if side==0 else 'east room')
                if point['name']!=marker:continue
                base=BASE+height*PITCH
                corridor=4 if side==0 else 28;centre=-8 if side==0 else 40
                coordinates=[(corridor,16),(centre,16),(centre,5),(centre,28),(centre,16),(corridor,16)]
                if side:coordinates.append((28,28))
                points.extend({'name':f'{"west" if side==0 else "east"} terrace {index+1}','feet':[x,n,base],'min_clearance':5.5} for index,(x,n) in enumerate(coordinates))
        route['waypoints']=points
    north = -18 if spec['kind'] in ['pyramid', 'court'] else -3
    route['start'] = [16, north, BASE]
    route['waypoints'][-1]['feet'] = [16, north, BASE]
    route['waypoints'][-1]['name'] = 'exit building'
    return route


def footprint(spec):
    if spec['kind'] in ['pyramid', 'court']:
        return (-16, 48, -16, 48)
    if spec['kind'] == 'slab':
        return (-16, 48, 0, 32)
    return (0, 32, 0, 32)


def foundation(spec):
    x, hi_x, north, hi_north = footprint(spec)
    model = Masonry()
    model.slab([(x - 4, hi_x + 4, north - 4, hi_north + 4)], BASE, 71)
    return model


def stage(spec, name):
    if name == 'foundation':
        return foundation(spec)
    model = Masonry()
    model.compact_short_rows = True
    main = [(0, 32, 0, 32)]
    well = [(10, 14, 6, 16), (18, 22, 6, 16), (10, 22, 16, 20)]
    if name.startswith('floor-'):
        level = int(name.removeprefix('floor-')) - 1
        assert 0 <= level < storeys(spec)
        base = BASE + level * PITCH
        if level:
            model.slab(main, base, spec['trim'], well)
        for course in range(COURSES):
            color = spec['trim'] if course == 8 else spec['wall']
            doors=terrace_doors(spec,level)
            cuts=doors if course<6 else [(lo,hi,12,20) for lo,hi,_,_ in doors] if course==6 else []
            tower_walls(model, False, course, base + course, color, level == 0,extra_holes=cuts)
            if course==6:
                for lo,_,_,_ in doors:model.part('3007',color,lo,12,base+course,turn=True)
        stair(model, base, level < storeys(spec)-1)
        return model
    if name == 'roof':
        top = BASE + storeys(spec) * PITCH
        # Close only the actual stairwell, on its three masonry stringers.
        # The monument's wider 4..28 closure depends on its heavy crown to
        # counterbalance rear plates; it is unsuitable for an unweighted deck.
        for layer, holes, phase in [(0, well, 4), (1, [(8, 24, 6, 20)], 0)]:
            for lo, hi, north, hi_north in rectangles(main, holes):
                xs = [lo] + [value for value in range(lo + 1, hi) if value % 8 == phase] + [hi]
                ns = [north] + [value for value in range(north + 1, hi_north) if value % 8 == phase] + [hi_north]
                for x, xhi in zip(xs, xs[1:]):
                    for n, nhi in zip(ns, ns[1:]):
                        model.plate_rectangle(xhi - x, nhi - n, spec['trim'], x, n, top - (2 - layer) / 3)
        for north in range(6, 20, 2):
            model.part('4282', spec['trim'], 8, north, top - 1 / 3)
        if spec['kind']=='pyramid':
            lo=-max(0,16-2*(storeys(spec)-1))
            if lo:model.slab([(lo,32-lo,lo,32-lo)],top,spec['trim'],main)
        if spec['kind']=='round':
            for x in [-4,32]:
                for n in [-4,32]:
                    for layer in range(2):model.part('3031',spec['trim'],x,n,top-2/3+layer/3)
        return model
    if name.startswith('exterior-'):
        return exterior(spec, int(name.removeprefix('exterior-')) - 1)
    if name == 'columns': return columns(spec)
    if name == 'windows': return windows(spec)
    if name == 'roof-details': return roof_details(spec)
    if name == 'terraces':return terraces(spec)
    raise ValueError(f'Unimplemented district stage: {name}')


def facade_ring(model, rectangle, course, height, color, openings, door=False):
    lo, hi, south, north = rectangle
    holes=[]
    if 3 <= course < 6:
        for x in openings[0]: holes += [(x,x+4,south,south+2),(x,x+4,north-2,north)]
        for n in openings[1]: holes += [(lo,lo+2,n,n+4),(hi-2,hi,n,n+4)]
    if door and course < 6: holes.append((14,18,south,south+2))
    headers=course>=6
    x0,x1=(lo,hi) if headers or course%2==0 else (lo+2,hi-2)
    for n in [south,north-2]:model.brickwork([(x0,x1,n,n+2)],height,color,holes,phase=4 if headers else 4*(course%2))
    n0,n1=(south+2,north-2) if headers or course%2==0 else (south,north)
    for x in [lo,hi-2]:model.brickwork([(x,x+2,n0,n1)],height,color,holes,turn=True,phase=4 if headers else 4*(course%2))


def exterior(spec, level):
    model=Masonry();model.compact_short_rows=True
    base=BASE+level*PITCH;core=(0,32,0,32)
    if spec['kind']=='pyramid':
        lo=-max(0,16-2*level);hi=32-lo
        if level:
            previous=-max(0,16-2*(level-1))
            model.slab([(previous,32-previous,previous,32-previous)],base,spec['trim'],[core])
        if lo==0:return model
        for course in range(COURSES):
            color=spec['trim'] if course==8 else spec['wall']
            facade_ring(model,(lo,hi,lo,hi),course,base+course,color,([6,14,22],[6,14,22]),door=level==0)
            cuts: list[tuple[int,int,int,int]]=[core]
            if level==0 and course<6:cuts.append((14,18,lo+2,0))
            if level==0 and course==6:cuts.append((12,20,lo+2,0))
            model.bulk((lo+2,hi-2,lo+2,hi-2),base+course,color,cuts,turn=bool(course%2))
            if level==0 and course==6:
                for north in range(lo+2,0,2):model.part('3007',color,12,north,base+course)
    else:
        for side,(lo,hi) in enumerate([(-16,0),(32,48)]):
            if level>=wing_storeys(spec)[side]:continue
            area=(lo,hi,0,32)
            if level:wing_floor(model,area,base,spec['trim'])
            for course in range(COURSES):
                color=spec['trim'] if course==8 else spec['wall']
                facade_ring(model,area,course,base+course,color,([lo+6],[6,14,22]))
            if level==wing_storeys(spec)[side]-1:wing_floor(model,area,base+PITCH,spec['trim'])
    return model


def wing_floor(model, area, top, color):
    """Full-width plate beams bear on both perimeter walls of each hollow wing."""
    lo,hi,south,north=area
    assert hi-lo==16 and (north-south)%2==0
    for n in range(south,north,2):model.part('4282',color,lo,n,top-2/3)
    model.plate_rectangle(16,north-south,color,lo,south,top-1/3)


def add_windows(model, rectangle, height, trim, xs, ns, skip_door=False,skip_sides=()):
    lo,hi,south,north=rectangle
    for x in xs:
        if not (skip_door and x==14):model.part('60594',trim,x,south,height)
        model.part('60594',trim,x,north-1,height)
    for n in ns:
        if (lo,n) not in skip_sides:model.part('60594',trim,lo,n,height,turn=True)
        if (hi-1,n) not in skip_sides:model.part('60594',trim,hi-1,n,height,turn=True)


def windows(spec):
    model=Masonry()
    for level in range(storeys(spec)):
        height=BASE+level*PITCH+3
        doors=terrace_doors(spec,level)
        skip=[(0 if lo==0 else 31,14) for lo,_,_,_ in doors]
        add_windows(model,(0,32,0,32),height,spec['trim'],[6,14,22],[6,14,22],skip_door=level==0,skip_sides=skip)
        if spec['kind']=='pyramid' and level<8:
            lo=-16+2*level
            add_windows(model,(lo,32-lo,lo,32-lo),height,spec['trim'],[6,14,22],[6,14,22],skip_door=level==0)
        elif spec['kind']=='slab':
            for side,(lo,hi) in enumerate([(-16,0),(32,48)]):
                if level<wing_storeys(spec)[side]:add_windows(model,(lo,hi,0,32),height,spec['trim'],[lo+6],[6,14,22])
    return model


def columns(spec):
    model=Masonry()
    if spec['kind']=='round':
        for level in range(storeys(spec)):
            base=BASE+level*PITCH
            for x in [-4,32]:
                for n in [-4,32]:
                    for course in range(9):model.part('87081',spec['trim'],x,n,base+course)
                    if level<storeys(spec)-1:
                        for plate in range(2):model.part('3031',spec['trim'],x,n,base+9+plate/3)
        return model
    north=-16 if spec['kind']=='court' else -4
    for x in [0,10,18,28]:
        for course in range(9):model.part('87081',spec['trim'],x,north,BASE+course)
    for n in [north,north+2]:
        for x,part in [(0,'3022'),(2,'3832'),(12,'3034'),(20,'3832'),(30,'3022')]:model.part(part,spec['trim'],x,n,BASE+9)
    model.plate_rectangle(32,4,spec['trim'],0,north,BASE+9+1/3)
    return model


def roof_details(spec):
    model=Masonry();top=BASE+storeys(spec)*PITCH
    for x in range(0,32,4):
        model.part('3037',spec['wall'],x,0,top)
        model.part('3037',spec['wall'],x,30,top,rotation=yaw(math.pi))
    for x in [0,31]:
        for n in [4,20]:
            model.part('3008',spec['trim'],x,n,top,turn=True)
            for shift in [0,4]:model.part('3633',spec['trim'],x,n+shift,top+1,turn=True)
    for course in range(4):model.part('3941',spec['trim'],0,14,top+course)
    model.part('87580',spec['trim'],0,14,top+4)
    count=8 if spec['key']=='02-copper-needle' else 4 if spec['kind']=='tower' else 1
    for course in range(count):model.part('3062b',spec['trim'],0.5,14.5,top+4+1/3+course)
    model.part('4740',spec['trim'],0,14,top+4+1/3+count)
    return model

def terrace_doors(spec,level):
    if not spec.get('wing_storeys'):return []
    return [(lo,hi,13,19) for side,(lo,hi) in enumerate([(0,2),(30,32)]) if level==wing_storeys(spec)[side]]

def terraces(spec):
    """Three-sided, upright catalogue guardrails on supported 16x32 wing roofs."""
    model=Masonry()
    for side,height in enumerate(wing_storeys(spec)):
        lo,hi=(-16,0) if side==0 else (32,48);top=BASE+height*PITCH
        for north in [0,31]:
            model.brickwork([(lo,hi,north,north+1)],top,spec['trim'])
            for x in range(lo,hi,4):model.part('3633',spec['trim'],x,north,top+1,rotation=yaw(math.pi if north else 0))
        east=lo if side==0 else hi-1
        model.brickwork([(east,east+1,1,31)],top,spec['trim'],turn=True)
        for north in range(2,30,4):model.part('3633',spec['trim'],east,north,top+1,rotation=yaw(-math.pi/2 if side==0 else math.pi/2))
        for north in [1,30]:model.part('3005',spec['trim'],east,north,top+1)
    return model
