"""Original minifigure-scale district, adapting the accepted tower recipe.

Bond/floor source: https://brickbuildershandbook.com/basic-lego-techniques/
"""
from scripts.build.monument_models import BASE, COURSES, PITCH, Masonry, rectangles, stair, tower_route, tower_walls
from scripts.native_model import yaw
import math

CORE_STAGES = ['foundation', *[f'floor-{level:02}' for level in range(1, 11)], 'roof']
STAGES = [*CORE_STAGES, *[f'exterior-{level:02}' for level in range(1, 11)], 'columns', 'windows', 'roof-details']

CENTRE_TOWER = '88c288ddd45cda14e5b51cbbd116f5243610b1b08f883923ce0dabda050deb2f'
ORIGIN = (19968, 64, 0)
SPECS = [
    {'key': '01-azure-terrace', 'name': 'Azure Terrace Pyramid', 'cell': (-2, -1), 'kind': 'pyramid', 'wall': 321, 'trim': 15},
    {'key': '02-copper-needle', 'name': 'Copper Needle Tower', 'cell': (-1, -1), 'kind': 'tower', 'wall': 320, 'trim': 14},
    {'key': '03-white-slab', 'name': 'White Slab House', 'cell': (0, -1), 'kind': 'slab', 'wall': 15, 'trim': 72},
    {'key': '04-emerald-court', 'name': 'Emerald Courtyard Tower', 'cell': (1, -1), 'kind': 'court', 'wall': 288, 'trim': 19},
    {'key': '05-ochre-cascade', 'name': 'Ochre Cascade Pyramid', 'cell': (-2, 0), 'kind': 'pyramid', 'wall': 19, 'trim': 28},
    {'key': '06-civic-bell', 'name': 'Civic Bell Tower', 'cell': (-1, 0), 'kind': 'tower', 'wall': 71, 'trim': 15},
    {'key': '07-crimson-gallery', 'name': 'Crimson Gallery House', 'cell': (1, 0), 'kind': 'slab', 'wall': 320, 'trim': 19},
    {'key': '08-sandstone-ziggurat', 'name': 'Sandstone Ziggurat', 'cell': (-2, 1), 'kind': 'pyramid', 'wall': 28, 'trim': 14},
    {'key': '09-colonnade-exchange', 'name': 'Colonnade Exchange', 'cell': (-1, 1), 'kind': 'columns', 'wall': 15, 'trim': 19},
    {'key': '10-north-arch', 'name': 'North Arch Court', 'cell': (0, 1), 'kind': 'slab', 'wall': 70, 'trim': 15},
    {'key': '11-silver-observatory', 'name': 'Silver Observatory', 'cell': (1, 1), 'kind': 'round', 'wall': 72, 'trim': 71},
]
assert len(SPECS) == 11 and len({spec['cell'] for spec in SPECS}) == 11


def stages(spec):
    extra = [f'exterior-{level:02}' for level in range(1, 10 if spec['kind'] == 'pyramid' else 11)] if spec['kind'] in ['pyramid', 'slab'] else ['columns'] if spec['kind'] in ['court','columns','round'] else []
    return [*CORE_STAGES, *extra, 'windows', 'roof-details']


def required_parts(name):
    if name == 'windows': return ['60594']
    if name == 'columns': return ['87081']
    if name == 'roof-details': return ['3037','3039','3633','3062b','3941','87580','4740']
    return []


def origin(spec):
    east, north = spec['cell']
    return [ORIGIN[0] + east * 96, ORIGIN[1] + north * 96, ORIGIN[2]]


def walking_route(spec):
    """Visit all ten storeys and return through the actual district foundation."""
    route = tower_route(origin(spec))
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
        assert 0 <= level < 10
        base = BASE + level * PITCH
        if level:
            model.slab(main, base, spec['trim'], well)
        for course in range(COURSES):
            color = spec['trim'] if course == 8 else spec['wall']
            tower_walls(model, False, course, base + course, color, level == 0)
        stair(model, base, level < 9)
        return model
    if name == 'roof':
        top = BASE + 10 * PITCH
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
        return model
    if name.startswith('exterior-'):
        return exterior(spec, int(name.removeprefix('exterior-')) - 1)
    if name == 'columns': return columns(spec)
    if name == 'windows': return windows(spec)
    if name == 'roof-details': return roof_details(spec)
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
        for lo,hi in [(-16,0),(32,48)]:
            area=(lo,hi,0,32)
            if level:wing_floor(model,area,base,spec['trim'])
            for course in range(COURSES):
                color=spec['trim'] if course==8 else spec['wall']
                facade_ring(model,area,course,base+course,color,([lo+6],[6,14,22]))
            if level==9:wing_floor(model,area,base+PITCH,spec['trim'])
    return model


def wing_floor(model, area, top, color):
    """Full-width plate beams bear on both perimeter walls of each hollow wing."""
    lo,hi,south,north=area
    assert hi-lo==16 and (north-south)%2==0
    for n in range(south,north,2):model.part('4282',color,lo,n,top-2/3)
    model.plate_rectangle(16,north-south,color,lo,south,top-1/3)


def add_windows(model, rectangle, height, trim, xs, ns, skip_door=False):
    lo,hi,south,north=rectangle
    for x in xs:
        if not (skip_door and x==14):model.part('60594',trim,x,south,height)
        model.part('60594',trim,x,north-1,height)
    for n in ns:
        model.part('60594',trim,lo,n,height,turn=True)
        model.part('60594',trim,hi-1,n,height,turn=True)


def windows(spec):
    model=Masonry()
    for level in range(10):
        height=BASE+level*PITCH+3
        add_windows(model,(0,32,0,32),height,spec['trim'],[6,14,22],[6,14,22],skip_door=level==0)
        if spec['kind']=='pyramid' and level<8:
            lo=-16+2*level
            add_windows(model,(lo,32-lo,lo,32-lo),height,spec['trim'],[6,14,22],[6,14,22],skip_door=level==0)
        elif spec['kind']=='slab':
            for lo,hi in [(-16,0),(32,48)]:add_windows(model,(lo,hi,0,32),height,spec['trim'],[lo+6],[6,14,22])
    return model


def columns(spec):
    model=Masonry()
    if spec['kind']=='round':
        for level in range(10):
            base=BASE+level*PITCH
            for x in [-4,32]:
                for n in [-4,32]:
                    for course in range(9):model.part('87081',spec['trim'],x,n,base+course)
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
    model=Masonry();top=BASE+10*PITCH
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
