"""Catalogue-based minifigure-scale street details. Datums come from MCP.

Geometry credits are retained in resolved native assets. Bounds are the declared
nominal proxies documented in docs/part-catalog.md, not exact material collision.
"""
import math
from scripts.native_model import Model, SPECIAL_BOUNDS, instance, yaw

PARTS=['60594','3037','3039','3633','3062b','3941','87081','2039','4073','4740',
       '970c00','76382p00','3626ap01','4624c01','3901','4211','3788','3823','4083','3829c01']


def rooted(model,part,color,position,rotation=None):
    model.definitions['main'].append(instance('part',part+'.dat',position,color,rotation))


def person(torso=321,legs=272,hair=0):
    model=Model();model.part('3031',71,0,0,0)
    height=1/3
    for part,color in [('970c00',legs),('76382p00',torso),('3626ap01',14)]:
        box=SPECIAL_BOUNDS[part]
        root=height-box['min'][2]
        rooted(model,part,color,[2,1.5,root])
        height=root+box['max'][2]
    rooted(model,'3901',hair,[2,1.5,height])
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
    """Classic Town car adapted from the official LEGO 6505 OMR model.

    Robert Paciorek [bercik], https://library.ldraw.org/omr/sets/610 ; model
    https://library.ldraw.org/media/omr_models/671/6505-1.mpd , dual CCAL 2.0 / BSD-MIT.
    Preserve this attribution. Genuine 4211/3788 shells, 3823 windscreen, 4083
    roll bar and 3829c01 steering follow its part choices and orthogonal poses.
    Adaptations: a full-width gravity-supported chassis, existing factory tyre
    units and upright front lights/support plates. Retain the official open
    cabin/roll-bar silhouette rather than adding an oversized flat plate roof.
    Rolling/steering mechanics and calibrated axle retention are not claimed.
    """
    model=Model();base=0
    if pedestal:
        model.part('3035',71,0,0,0);model.floor(8,4,72,0,0,1/3);base=2/3
    wheel=SPECIAL_BOUNDS['4624c01'];wheel_root=base-wheel['min'][2]
    for x in [2,6]:
        for north in [0.4,3.6]:rooted(model,'4624c01',7,[x,north,wheel_root])
    height=wheel_root+wheel['max'][2]
    model.part('3035',0,0,0,height);height+=1/3
    for part,x,north,z,angle,shade in [
        ('4211',6,2,2/3,-math.pi/2,color),('3788',2,2,2/3,-math.pi/2,color),
        ('3829c01',3.5,2,2/3,math.pi/2,0),
        ('3023',4,.5,2/3,0,color),('3023',4,3.5,2/3,0,color),
        ('3024',7.5,.5,2/3,0,36),('3024',7.5,3.5,2/3,0,36),
        ('3710',1.5,2,1,-math.pi/2,color),
        ('3666',5,.5,1,math.pi,color),('3666',5,3.5,1,math.pi,color),
        ('3823',3.5,2,3,math.pi/2,43),('4083',7.5,2,3,math.pi/2,color),
    ]:rooted(model,part,shade,[x,north,height+z],yaw(angle))
    for north in [0,2]:model.part('3023',color,1,north,height+1/3,turn=True)
    for north in [0,3]:model.part('3024',14,1,north,height+1)
    return model
