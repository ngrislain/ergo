"""Walkable boulevards, building skybridges and genuine street details."""
import json
from scripts.build import district_models as buildings, district_details as details
from scripts.build.monument_models import Masonry, rectangles
from scripts.native_model import Model

CENTRE_TOWER=buildings.CENTRE_TOWER
CENTRE=[buildings.ORIGIN[0]+16,buildings.ORIGIN[1]+16,0]
SPECS=[{'key':'streets','name':'Boulevards and Street Life'},
       {'key':'bridge-west','name':'Copper Needle–White Slab Skybridge'},
       {'key':'bridge-east','name':'Colonnade–North Arch Skybridge'}]
VERTICAL=[-240,-144,-48,48,144]
HORIZONTAL=[-144,-48,48,144]
BRIDGES={'bridge-west':[-152,-52],'bridge-east':[40,44]}
PARKING=[(east,north) for east in [-196,-100,-4,92] for north in [-46,42]]


def origin(spec):
    if spec['key']!='streets':
        from scripts.build import district_skybridges as sky
        return sky.origin(next(item for item in sky.SPECS if item['key']==spec['key']))
    dx,dn=BRIDGES.get(spec['key'],[0,0])
    return [CENTRE[0]+dx,CENTRE[1]+dn,0]


def stages(spec):return ['street-anchor',*ROAD_STAGES,'furniture'] if spec['key']=='streets' else ['bridge']
def required_parts(stage):return details.PARTS if stage=='furniture' else ['3633','3026'] if stage=='bridge' else []


def intersects(a,b):return min(a[1],b[1])>max(a[0],b[0]) and min(a[3],b[3])>max(a[2],b[2])


def plot_bounds():
    result=[(-40,40,-24,24)]
    for spec in buildings.SPECS:
        x,hi,n,hn=buildings.footprint(spec);dx,dn=spec['cell'];dx=dx*96-16;dn=dn*96-16
        result.append((x-4+dx,hi+4+dx,n-4+dn,hn+4+dn))
    return result


def road_areas():
    return [(x-8,x+8,-152,152) for x in VERTICAL]+[(-248,152,n-8,n+8) for n in HORIZONTAL]


def bridge_holes():return [(x-8,x+24,n,n+8) for x,n in BRIDGES.values()]

def retired_bridge_road_fill():
    """Restore the two intersections after the stair bridges are relocated."""
    model=Masonry()
    for area in bridge_holes():model.bulk(area,0,72)
    for n in HORIZONTAL:
        for x in range(-244,148,8):
            marker=(x,x+4,n,n+1)
            if not any(h[0]<=x and x+4<=h[1] and h[2]<=n and n+1<=h[3] for h in bridge_holes()):continue
            if any(v-9<x+4 and x<v+9 for v in VERTICAL):continue
            model.part('2431',14,x,n,1)
    return model


def roads():
    model=Masonry();model.compact_short_rows=True
    plots=plot_bounds();holes=[(x-1,hi+1,n-1,hn+1) for x,hi,n,hn in plots]
    areas=road_areas()
    for area in rectangles(areas,holes):model.bulk(area,0,72)
    sidewalks=[(x-11,x-8,-152,152) for x in VERTICAL]+[(x+8,x+11,-152,152) for x in VERTICAL]
    sidewalks += [(-248,152,n-11,n-8) for n in HORIZONTAL]+[(-248,152,n+8,n+11) for n in HORIZONTAL]
    # Four-stud entry paths end at one-stud modular expansion joints.
    for column,row in [(c,r) for c in [-2,-1,0,1] for r in [-1,0,1]]:
        x,n=column*96,row*96
        plot=next((p for p in plots if p[0]<=x<=p[1] and p[2]<=n<=p[3]),None)
        if plot:sidewalks.append((x-2,x+2,n-40,plot[2]-1))
    for area in rectangles(sidewalks,[*areas,*holes]):
        model.brickwork([area],0,71)
        x,hi,n,hn=area;model.plate_rectangle(hi-x,hn-n,71,x,n,1)
    bays=[(x,x+8,n,n+4) for x,n in PARKING]
    for x,n in PARKING:model.floor(8,4,72,x,n,1)
    # Smooth tiles are deliberate lane markings/parking surfaces.
    for n in HORIZONTAL:
        for x in range(-244,148,8):
            marker=(x,x+4,n,n+1)
            if any(intersects(marker,hole) for hole in [*holes,*bays]):continue
            if any(v-9<x+4 and x<v+9 for v in VERTICAL):continue
            model.part('2431',14,x,n,1)
    for x in VERTICAL:
        for n in range(-148,148,8):
            marker=(x,x+1,n,n+4)
            if any(intersects(marker,hole) for hole in [*holes,*bays]):continue
            if any(h-9<n+4 and n<h+9 for h in HORIZONTAL):continue
            model.part('2431',14,x,n,1,turn=True)
    return model


def street_layer(anchor):
    """Publish one existing grounded paving instance before the larger road edit."""
    model=roads()
    instances=model.definitions['main']
    model.definitions['main']=instances[:1] if anchor else instances[1:]
    return model


def road_chunks(model):
    """Keep construction order: ground masonry precedes its supported paving."""
    counts={}
    def leaves(instance):
        target=instance['target']
        if target['kind']=='part':return 1
        name=target['id']
        if name not in counts:counts[name]=sum(leaves(item) for item in model.definitions[name])
        return counts[name]
    chunks=[];size=0
    for instance in model.definitions['main'][1:]:
        count=leaves(instance)
        if not chunks or size+count>600:chunks.append([]);size=0
        chunks[-1].append(instance);size+=count
    return chunks


def road_chunk(index):
    model=roads()
    model.definitions['main']=road_chunks(model)[index]
    return model


def mount(model,name,prototype,x,n,height):
    prefix=name+'/'
    if prefix+'main' not in model.definitions:
        for key,items in prototype.definitions.items():
            copied=json.loads(json.dumps(items))
            for item in copied:
                if item['target']['kind']=='definition':item['target']['id']=prefix+item['target']['id']
            model.definitions[prefix+key]=copied
    model.reference(prefix+'main',x,n,height)


def furniture():
    model=Model();colors=[4,1,14,288,15,320,25,321]
    for index,(x,n) in enumerate(PARKING):mount(model,'lego6505-robert-paciorek-bercik-bsd-mit-car-'+str(index),details.car(colors[index]),x,n,4/3)
    for index,(column,row) in enumerate((c,r) for c in [-2,-1,0,1] for r in [-1,0,1]):
        x,n=column*96,row*96-40
        mount(model,'person-'+str(index),details.person(colors[index%8],0,0),x-6,n,4/3)
        mount(model,'person-b-'+str(index),details.person(colors[(index+3)%8],272,70),x+2,n,4/3)
        mount(model,'bench',details.bench(),x-14,n,4/3)
        mount(model,'lamp',details.lamppost(),x+12,n,4/3)
    return model


def legacy_bridge():
    """Original stair bridge, retained only to interpret pre-revision receipts."""
    model=Model()
    for x in [0,14]:
        for course in range(4):model.part('3007',71,x,0,course,turn=True)
    model.plate_rectangle(12,8,1,2,0,0)
    for n in range(0,8,2):model.part('4282',71,0,n,4)
    model.plate_rectangle(16,8,71,0,0,4+1/3)
    for step in range(4):
        for course in range(step+1):model.part('3007',71,-8+2*step,0,course,turn=True)
        for course in range(4-step):model.part('3007',71,16+2*step,0,course,turn=True)
    for n in [0,7]:
        for x in [0,8]:model.part('3008',15,x,n,4+2/3)
        for x in [0,4,8,12]:model.part('3633',15,x,n,5+2/3)
    return model


def stage(spec,name):
    if name in ['street-anchor','roads']:return street_layer(name=='street-anchor')
    if name.startswith('roads-'):return road_chunk(int(name.removeprefix('roads-'))-1)
    if name=='furniture':return furniture()
    from scripts.build import district_skybridges as sky
    return sky.model(next(item for item in sky.SPECS if item['key']==spec['key']))


def walking_route(spec):
    from scripts.build import district_skybridges as sky
    return sky.walking_route(next(item for item in sky.SPECS if item['key']==spec['key']))

def legacy_walking_route(spec):
    """Continuous two-way bridge crossing, for check_walk against actual geometry."""
    if spec['key'] not in BRIDGES:
        raise ValueError('Choose one of the two footbridge assemblies')
    deck = 4 + 2/3
    return {'format': 'ergo-walk-route-1', 'schema': 2,
            'origin_world': [CENTRE[0]+BRIDGES[spec['key']][0],CENTRE[1]+BRIDGES[spec['key']][1],0], 'start': [-10, 3, 1],
            'eye_height': 3.5, 'min_clearance': 4, 'speed': 4,
            'waypoints': [{'name': name, 'feet': [east, 3, height]} for east, height, name in [
                (2, deck, 'west stair ascent'), (8, deck, 'deck centre'),
                (14, deck, 'east deck'), (26, 1, 'east stair descent'),
                (-10, 1, 'return west')]]}


ROAD_STAGES=[f'roads-{index+1:02}' for index in range(len(road_chunks(roads())))]
STAGES=['street-anchor',*ROAD_STAGES,'furniture','bridge']
