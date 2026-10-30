"""Catalogue-based minifigure-scale street details. Datums come from MCP.

Geometry credits are retained in resolved native assets. Bounds are the declared
nominal proxies documented in docs/part-catalog.md, not exact material collision.
"""
from scripts.native_model import Model, SPECIAL_BOUNDS, CONSTRUCTS, instance

PARTS=['60594','3037','3039','3633','3062b','3941','87081','2039','4073','4740',
       '970c00','76382p00','3626ap01','4624c01','3901','4211','3788','3823','4083','3829c01']


def rooted(model,part,color,position,rotation=None):
    model.definitions['main'].append(instance('part',part+'.dat',position,color,rotation))


def person(torso=321,legs=272,hair=0):
    if legs==272 and hair==0:
        model=Model();model.construct('minifigure-v1',color=torso);return model
    model=Model();colours={'3815c01.dat':legs,'76382p00.dat':torso,'3901.dat':hair}
    for part in CONSTRUCTS['minifigure-v1']['parts']:
        model.definitions['main'].append(instance('part',part['part'],part['position'],colours.get(part['part'],part.get('color'))))
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
    model.construct('classic-car-v1',height=base,color=color)
    return model
