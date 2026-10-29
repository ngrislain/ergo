"""Prepare atomic MCP replacement models from authoritative construction caches.

No file edit here is a shared commit. Publication uses place_submodel(replace_ids)
and checks its accepted receipt. Unchanged source occurrences retain their IDs.
"""
import itertools,math
from scripts.native_model import Model,instance,yaw
from scripts.build.monument_models import Masonry,rectangles
from scripts.build import district_models as buildings,district_streets as streets,district_details as details,district_skybridges as sky

def multiply(a,b):return [[sum(a[i][k]*b[k][j] for k in range(4)) for j in range(4)] for i in range(4)]

def leaves(construction):
    model=construction['model'];properties=iter(construction['pieces']);result=[]
    def visit(name,parent,color):
        for value in model['definitions'][name]:
            transform=multiply(parent,value['transform']);shade=value.get('color',color);shade=color if shade==16 else shade;target=value['target']
            if target['kind']=='definition':visit(target['id'],transform,shade)
            else:
                metadata=next(properties);part=target['id'];box=construction['meshes'][part]['body_bounds']
                points=[[sum(transform[i][j]*point[j] for j in range(4)) for i in range(3)] for point in itertools.product(*zip(box['min'],box['max']),[1])]
                result.append({'id':metadata['id'],'part':part,'color':shade,'position':[transform[i][3] for i in range(3)],
                    'min':[min(point[i] for point in points) for i in range(3)],'max':[max(point[i] for point in points) for i in range(3)]})
    visit(model['root'],[[int(i==j) for j in range(4)] for i in range(4)],71)
    try:next(properties);raise ValueError('Unexpected unmatched piece properties')
    except StopIteration:pass
    return result

def overlap(brick,volume):return all(min(a,b)-max(c,d)>1e-5 for a,b,c,d in zip(brick['max'],volume['max'],brick['min'],volume['min']))

def portal_replacement(construction,spec):
    x,north,_=buildings.origin(spec);strips=sky.portals(spec['key']);assert strips
    volumes=[]
    for lo,hi,south,end in strips:
        volumes.append({'min':[x+lo,north+south,sky.FLOOR],'max':[x+hi,north+end,sky.FLOOR+6]})
        volumes.append({'min':[x+lo,north+12,sky.FLOOR+6],'max':[x+hi,north+20,sky.FLOOR+7]})
    removed=[];model=Masonry();model.compact_short_rows=True
    for brick in leaves(construction):
        cuts=[volume for volume in volumes if overlap(brick,volume)]
        if not cuts:continue
        removed.append(brick['id'])
        if any(all(brick['min'][i]>=volume['min'][i]-1e-5 and brick['max'][i]<=volume['max'][i]+1e-5 for i in range(3)) for volume in cuts):continue
        height=brick['max'][2]-brick['min'][2]
        if abs(height-1)>1e-5:raise ValueError('Cannot split a nonstandard portal piece '+brick['part'])
        footprint=tuple(round(brick[edge][axis]) for axis,edge in [(0,'min'),(0,'max'),(1,'min'),(1,'max')])
        holes=[tuple(round(volume[edge][axis]) for axis,edge in [(0,'min'),(0,'max'),(1,'min'),(1,'max')]) for volume in cuts]
        for area in rectangles([footprint],holes):model.brickwork([area],brick['min'][2],brick['color'],turn=area[1]-area[0]<area[3]-area[2],phase=4*(round(brick['min'][2]-sky.FLOOR)%2))
    for lo,hi,_,_ in strips:model.part('3007',spec['wall'],x+lo,north+12,sky.FLOOR+6,turn=True)
    assert removed,'No doorway pieces found; stop rather than publishing the wrong edit'
    return removed,model

def street_replacement(construction):
    old=leaves(construction);removed=[];model=Model();colors=[4,1,14,288,15,320,25,321]
    cx,cn,_=streets.CENTRE
    for index,(east,north) in enumerate(streets.PARKING):
        lo,hi,south,end=cx+east,cx+east+8,cn+north,cn+north+4
        members=[brick for brick in old if brick['min'][2]>=4/3-1e-5 and brick['min'][0]>=lo-1e-5 and brick['max'][0]<=hi+1e-5 and brick['min'][1]>=south-1e-5 and brick['max'][1]<=end+1e-5]
        assert len(members)==14,('Unexpected existing car',index,len(members))
        removed.extend(brick['id'] for brick in members)
        streets.mount(model,'lego6505-robert-paciorek-bercik-bsd-mit-car-'+str(index),details.car(colors[index]),lo,south,4/3)
    caps=[brick for brick in old if '3070b' in brick['part'] and 5<brick['min'][2]<7]
    assert len(caps)==24,('Unexpected figure cap count',len(caps))
    for brick in caps:
        removed.append(brick['id'])
        model.definitions['main'].append(instance('part','3901.dat',[*brick['position'][:2],brick['min'][2]],brick['color']))
    streets.mount(model,'retired-bridge-road-fill',streets.retired_bridge_road_fill(),cx,cn,0)
    assert len(set(removed))==len(removed)
    return removed,model
