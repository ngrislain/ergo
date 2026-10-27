"""Catalogue-based minifigure-scale street details. Datums come from MCP.

Geometry credits are retained in resolved native assets. Bounds are the declared
nominal proxies documented in docs/part-catalog.md, not exact material collision.
"""
import math
from scripts.native_model import Model, SPECIAL_BOUNDS, instance, yaw

PARTS=['60594','3037','3039','3633','3062b','3941','87081','2039','4073','4740',
       '970c00','76382p00','3626ap01','4624c01']


def rooted(model,part,color,position):
    model.definitions['main'].append(instance('part',part+'.dat',position,color))


def person(torso=321,legs=272,hat=0):
    model=Model();model.part('3031',71,0,0,0)
    height=1/3
    for part,color in [('970c00',legs),('76382p00',torso),('3626ap01',14)]:
        box=SPECIAL_BOUNDS[part]
        root=height-box['min'][2]
        rooted(model,part,color,[2,1.5,root])
        height=root+box['max'][2]
    model.part('3070b',hat,1.5,1,height)
    return model


def bench():
    model=Model()
    for x in [0,5]:
        for north in [0,1]:model.part('3005',70,x,north,0)
    model.part('3795',70,0,0,1)
    model.part('3633',70,1,0,4/3)
    return model


def lamppost():
    model=Model();model.part('3022',71,0,0,0)
    height=1/3
    for part,color in [('2039',0),('3062b',14),('4740',0)]:
        box=SPECIAL_BOUNDS[part];root=height-box['min'][2]
        rooted(model,part,color,[1,1,root]);height=root+box['max'][2]
    return model


def car(color=4,pedestal=False):
    """Parked brick-built coupe with real factory wheel/tyre units.

    Wheels and chassis are gravity-supported display bodies. Rolling/steering
    mechanics and calibrated axle retention are not claimed.
    """
    model=Model();base=0
    if pedestal:
        model.part('3035',71,0,0,0);model.floor(8,4,72,0,0,1/3);base=2/3
    wheel=SPECIAL_BOUNDS['4624c01'];wheel_root=base-wheel['min'][2]
    for x in [1.5,6.5]:
        for north in [0.4,3.6]:rooted(model,'4624c01',7,[x,north,wheel_root])
    height=wheel_root+wheel['max'][2]
    model.part('3035',color,0,0,height);height+=1/3
    for x in [0,6]:model.part('3001',color,x,0,height,turn=True)
    model.part('3001',color,2,1,height)
    for x,angle in [(2,math.pi/2),(4,-math.pi/2)]:model.part('3039',0,x,1,height+1,rotation=yaw(angle))
    for x,shade in [(0,14),(7,4)]:
        for north in [0,3]:model.part('3024',shade,x,north,height+1)
    return model
