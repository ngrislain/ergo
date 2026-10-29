"""Third-storey, building-to-building connections; no stairs in vehicle lanes.

Technique source: Brick Builder's Handbook, overlapping slabs and supported
lintels. Genuine 3026 6x24 plates span the lane between pavement-side piers.
"""
from scripts.native_model import Model
from scripts.native_model import yaw
import math
from scripts.build import district_models as buildings
from scripts.build.monument_models import BASE,PITCH

LEVEL=2
FLOOR=BASE+LEVEL*PITCH
DECK=FLOOR+2/3
SPECS=[
    {'key':'bridge-west','name':'Copper Needle–White Slab Skybridge','left':'02-copper-needle','right':'03-white-slab'},
    {'key':'bridge-east','name':'Colonnade–North Arch Skybridge','left':'09-colonnade-exchange','right':'10-north-arch'},
]
BUILDINGS={spec['key']:spec for spec in buildings.SPECS}

def origin(spec):
    x,north,_=buildings.origin(BUILDINGS[spec['left']])
    return [x+30,north+13,0]

def portals(key):
    """Local body strips: floor opening and one bearing lintel above it."""
    if key in {spec['left'] for spec in SPECS}:return [(30,32,13,19)]
    if key in {spec['right'] for spec in SPECS}:return [(-16,-14,13,19),(-2,0,13,19),(0,2,13,19)]
    return []

def model(spec):
    result=Model()
    # Two split pavement-side piers leave a central passage at ground level.
    # These are full standard columns, not supports resting in traffic lanes.
    start=4/3
    courses=int(FLOOR-start)
    for x in [23,43]:
        for north in [0,4]:
            for course in range(courses):result.part('3003',71,x,north,start+course)
            height=start+courses
            while height<FLOOR-1e-6:
                result.part('3022',71,x,north,height);height+=1/3
    for x in [0,24]:result.part('3026',71,x,0,FLOOR)
    result.plate_rectangle(8,6,71,48,0,FLOOR)
    # Offset upper-layer seams from the 24/48-stud lower-layer joints.
    result.plate_rectangle(12,6,71,0,0,FLOOR+1/3)
    result.plate_rectangle(44,6,71,12,0,FLOOR+1/3)
    for north in [0,5]:
        for x in range(0,56,8):result.part('3008',15,x,north,DECK)
        for x in range(0,56,4):result.part('3633',15,x,north,DECK+1,rotation=yaw(math.pi if north else 0))
    return result

def walking_route(spec):
    x,north,_=origin(spec)
    waypoints=[(-26,15,FLOOR,'left corridor'),(-2,15,FLOOR,'left east corridor'),
        (-2,3,FLOOR,'left doorway'),(5,3,DECK,'left landing'),(16,3,DECK,'west span'),
        (28,3,DECK,'over traffic lane'),(42,3,DECK,'east span'),(54,3,DECK,'right landing'),
        (60,3,FLOOR,'right wing'),(70,3,FLOOR,'right core doorway'),
        (54,3,DECK,'return right landing'),(5,3,DECK,'return bridge'),(-2,3,FLOOR,'return left doorway'),
        (-2,15,FLOOR,'return left corridor'),(-26,15,FLOOR,'left stair corridor'),(-26,3,FLOOR,'left arrival')]
    return {'format':'ergo-walk-route-1','schema':2,'origin_world':[x,north,0],
        'start':[-26,3,FLOOR],'eye_height':3.5,'min_clearance':4,'speed':4,
        'waypoints':[{'name':name,'feet':[east,n,height]} for east,n,height,name in waypoints]}
