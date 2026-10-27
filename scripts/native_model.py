"""Small native Ergo model generator; all positions/transforms use public units."""
import itertools
import math

PARTS={"3070b":(1,1,1/3),"3069b":(2,1,1/3),"3068b":(2,2,1/3),"2431":(4,1,1/3),
       "3020":(4,2,1/3),"3031":(4,4,1/3),"3035":(8,4,1/3),"41539":(8,8,1/3),
        "3007":(8,2,1),"4201":(8,8,1),"4204":(16,8,1),"3008":(8,1,1),"3004":(2,1,1),"2456":(6,2,1),"3002":(3,2,1),"3003":(2,2,1),
       "3001":(4,2,1),"3010":(4,1,1),"3005":(1,1,1),"3024":(1,1,1/3),"3032":(6,4,1/3),"3021":(3,2,1/3),"3022":(2,2,1/3),
         "3795":(6,2,1/3),"3710":(4,1,1/3),"3034":(8,2,1/3),"3832":(10,2,1/3),"4282":(16,2,1/3),"3023":(2,1,1/3),"3666":(6,1,1/3),"3009":(6,1,1)}


SPECIAL_BOUNDS={}
SPECIAL_RENDER_BOUNDS={}


def register_catalog_part(part,description):
    """Use actual MCP-resolved nominal datums; never infer them from studs."""
    part=part.removesuffix('.dat')
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
    if color is not None:value["color"]=color
    return value


def bounds(model):
    points=[]
    def walk(id,parent):
        for value in model['definitions'][id]:
            matrix=[[sum(parent[i][k]*value['transform'][k][j] for k in range(4)) for j in range(4)] for i in range(4)]
            target=value['target']
            if target['kind']=='definition':walk(target['id'],matrix)
            else:
                part=target['id'].removesuffix('.dat')
                if part in SPECIAL_RENDER_BOUNDS:
                    rendered=SPECIAL_RENDER_BOUNDS[part];lo,hi=rendered['min'],rendered['max']
                else:
                    w,d,h=PARTS[part];top=0 if part in ['3070b','3069b','3068b','2431'] else 1/6
                    lo,hi=[-w/2,-d/2,-h],[w/2,d/2,top]
                for p in itertools.product(*zip(lo,hi),[1]):
                    points.append([sum(matrix[i][j]*p[j] for j in range(4)) for i in range(3)])
    walk(model['root'],[[int(i==j) for j in range(4)] for i in range(4)])
    return ([min(p[i] for p in points) for i in range(3)],[max(p[i] for p in points) for i in range(3)])


class Model:
    def __init__(self):
        self.definitions={"main":[]}

    def part(self,part,color,x,north,height,turn=False,rotation=None,definition="main"):
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
