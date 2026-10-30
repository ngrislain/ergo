"""Small native Ergo model generator; all positions/transforms use public units."""
import itertools
import math
import json
from pathlib import Path

PART_LIBRARY=json.loads((Path(__file__).resolve().parents[1]/'crates/world/library/sources.lock.json').read_text())
STANDARD_PARTS=[entry['catalogue'] for entry in PART_LIBRARY['parts'].values() if entry['catalogue']['size'] is not None]
PART_ALIASES={alias.removesuffix('.dat'):entry['catalogue']['id'].removesuffix('.dat') for entry in PART_LIBRARY['parts'].values() for alias in entry['catalogue']['aliases']}
CONSTRUCTS={value['id']:value for value in json.loads((Path(__file__).resolve().parents[1]/'crates/world/library/constructs.json').read_text())}

def canonical_part(part):
    part=part.removesuffix('.dat')
    return PART_ALIASES.get(part,part)

class PartMap(dict):
    """Alias lookup without storing a second record for the same part."""
    def __missing__(self,part):
        canonical=canonical_part(part)
        if canonical==part:raise KeyError(part)
        return self[canonical]
    def __contains__(self,part):return super().__contains__(canonical_part(part))
    def get(self,part,default=None):
        try:return self[part]
        except KeyError:return default

PARTS=PartMap({part['id'].removesuffix('.dat'):(*part['size'][:2],part['size'][2]/3) for part in STANDARD_PARTS})


SPECIAL_BOUNDS=PartMap()
SPECIAL_RENDER_BOUNDS=PartMap()


def register_catalog_part(part,description):
    """Use actual MCP-resolved nominal datums; never infer them from studs."""
    part=canonical_part(part)
    if description.get('quality') in ['proxy','parametric'] and part not in PARTS:
        raise ValueError('A genuine catalogue asset is required for '+part)
    bounds=description['body_bounds']
    if not all(math.isfinite(v) for edge in ['min','max'] for v in bounds[edge]):raise ValueError('Invalid catalogue bounds')
    if not all(a<b for a,b in zip(bounds['min'],bounds['max'])):raise ValueError('Empty catalogue body')
    SPECIAL_BOUNDS[part]=bounds
    SPECIAL_RENDER_BOUNDS[part]=description['bounds']


def snap_origin(origin):
    def nearest(value):
        if not math.isfinite(value):raise ValueError('Placement coordinates must be finite')
        return math.floor(value+.5) if value>=0 else math.ceil(value-.5)
    x,north,height=origin
    return [nearest(x),nearest(north),nearest(height*3)/3]


def multiply(a,b):
    return [[sum(a[i][k]*b[k][j] for k in range(3)) for j in range(3)] for i in range(3)]


def yaw(angle):
    c,s=math.cos(angle),math.sin(angle)
    return [[c,-s,0],[s,c,0],[0,0,1]]


def north_rotation(angle):
    c,s=math.cos(angle),math.sin(angle)
    return [[c,0,s],[0,1,0],[-s,0,c]]


def east_rotation(angle):
    c,s=math.cos(angle),math.sin(angle)
    return [[1,0,0],[0,c,-s],[0,s,c]]


def metric_matrix(rotation):
    scale=[.008,.008,.0096]
    def clean(value):
        if abs(value)<1e-12:return 0
        if abs(value-round(value))<1e-12:return round(value)
        return value
    return [[clean(rotation[i][j]*scale[j]/scale[i]) for j in range(3)] for i in range(3)]


def local_bounds(part,rotation):
    part=canonical_part(part)
    a=metric_matrix(rotation)
    if part in SPECIAL_BOUNDS:
        bounds=SPECIAL_BOUNDS[part];lo,hi=bounds['min'],bounds['max']
    else:
        w,d,h=PARTS[part];lo,hi=[-w/2,-d/2,-h],[w/2,d/2,0]
    corners=[[sum(a[i][j]*p[j] for j in range(3)) for i in range(3)]
             for p in itertools.product(*zip(lo,hi))]
    return ([min(p[i] for p in corners) for i in range(3)],
            [max(p[i] for p in corners) for i in range(3)])


def instance(kind,id,position,color=None,rotation=None):
    rotation=rotation or yaw(0);a=metric_matrix(rotation)
    value={"target":{"kind":kind,"id":id},"transform":[a[i]+[position[i]] for i in range(3)]+[[0,0,0,1]]}
    if color is not None and color!=16:value["color"]=color
    return value


def model_leaves(model,default_color=71):
    """Expand references for diagnostics, retaining compact definitions on disk."""
    count=0
    def visit(values,parent,color,stack):
        nonlocal count
        if len(stack)>=64:raise ValueError('Model nesting exceeds 64 levels')
        for value in values:
            matrix=[[sum(parent[i][k]*value['transform'][k][j] for k in range(4)) for j in range(4)] for i in range(4)]
            shade=value.get('color',color);shade=color if shade==16 else shade
            target=value['target'];kind=target['kind'];id=target['id']
            if kind=='definition':
                if id in stack:raise ValueError('Cyclic model definition')
                yield from visit(model['definitions'][id],matrix,shade,[*stack,id])
            elif kind=='construct':
                parts=CONSTRUCTS[id]['parts']
                values=[instance('part',part['part'],part['position'],part.get('color'),yaw(part.get('quarter_turns',0)*math.pi/2)) for part in parts]
                yield from visit(values,matrix,shade,[*stack,'construct:'+id])
            elif kind=='part':
                count+=1
                if count>200000:raise ValueError('Model exceeds 200,000 parts')
                yield {'part':id,'color':shade,'transform':matrix}
            else:raise ValueError('Unknown instance target kind '+kind)
    yield from visit(model['definitions'][model['root']],[[int(i==j) for j in range(4)] for i in range(4)],default_color,[model['root']])


def bounds(model):
    points=[]
    for leaf in model_leaves(model):
        matrix=leaf['transform'];part=canonical_part(leaf['part'])
        if part in SPECIAL_RENDER_BOUNDS:
            rendered=SPECIAL_RENDER_BOUNDS[part];lo,hi=rendered['min'],rendered['max']
        else:
            w,d,h=PARTS[part];top=0 if part in ['3070b','3069b','3068b','2431'] else 1/6
            lo,hi=[-w/2,-d/2,-h],[w/2,d/2,top]
        for p in itertools.product(*zip(lo,hi),[1]):
            points.append([sum(matrix[i][j]*p[j] for j in range(4)) for i in range(3)])
    return ([min(p[i] for p in points) for i in range(3)],[max(p[i] for p in points) for i in range(3)])


class Model:
    def __init__(self):
        self.definitions={"main":[]}

    def part(self,part,color,x,north,height,turn=False,rotation=None,definition="main"):
        part=canonical_part(part)
        rotation=rotation or yaw(math.pi/2 if turn else 0)
        lo,_=local_bounds(part,rotation)
        position=[p-q for p,q in zip([x,north,height],lo)]
        self.definitions.setdefault(definition,[]).append(instance("part",part+".dat",position,color,rotation))

    def plate_rectangle(self,width,depth,color,x,north,height):
        if not width or not depth:return
        parts=['41539','3035','3032','3031','4282','3832','3034','3795','3020','3021','3022','3666','3710','3023','3024']
        choices=[(w*d,part,w,d,turn) for part in parts for w,d,turn in [(*PARTS[part][:2],False),(PARTS[part][1],PARTS[part][0],True)] if w<=width and d<=depth]
        _,part,w,d,turn=max(choices,key=lambda v:v[0])
        self.part(part,color,x,north,height,turn=turn)
        self.plate_rectangle(width-w,d,color,x+w,north,height)
        self.plate_rectangle(width,depth-d,color,x,north+d,height)

    def reference(self,id,x,north,height,color=None,rotation=None,definition="main"):
        self.definitions.setdefault(definition,[]).append(instance("definition",id,[x,north,height],color,rotation))

    def construct(self,id,x:float=0,north:float=0,height:float=0,color=None,rotation=None,definition="main"):
        if id not in CONSTRUCTS:raise ValueError('Unknown versioned construct '+id)
        self.definitions.setdefault(definition,[]).append(instance('construct',id,[x,north,height],color,rotation))

    def strip(self,length,color,x,north,height,turn=False,definition="main"):
        offset=0
        for part,size in [("2431",4),("3069b",2),("3070b",1)]:
            while length>=size:
                self.part(part,color,x+(0 if turn else offset),north+(offset if turn else 0),height,turn,definition=definition)
                offset+=size;length-=size
        assert length==0

    def floor(self,width,depth,color,x,north,height,definition="main"):
        for row in range(depth):self.strip(width,color,x,north+row,height,definition=definition)

    def column(self,part,color,x,north,layers,height:float=0.,definition="main"):
        for layer in range(layers):self.part(part,color,x,north,height+layer/3,definition=definition)

    def export(self,origin=(0,0,0)):
        origin=tuple(snap_origin(origin))
        if origin!=(0,0,0):
            root="placed";definitions={**self.definitions,root:[instance("definition","main",list(origin))]}
        else:root="main";definitions=self.definitions
        return {"format":"ergo-model-1","schema":2,"root":root,"definitions":definitions}
