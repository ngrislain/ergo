#!/usr/bin/env python3
"""Accepted castle terrain and current native helpers for local refinements.

The completed castle/garrison conversion is retired. The accepted terrain was
inspired by LEGO 31168, 6080, 7094 and 10176 with genuine, unscaled parts.
"""
import math
from functools import cache
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from scripts.build import park
from scripts.native_model import Model, instance

CASTLE = 'efe68a4fb360b019bff8aefef258ace047b3c5afda4bf946e3dcda692ffe3e3d'
RAISE = 73/3
WIDTH, DEPTH = 384, 320
REFERENCES = [
    'https://www.lego.com/en-fr/product/medieval-horse-knight-castle-31168',
    'https://brickset.com/sets/6080-1/King-s-Castle',
    'https://brickset.com/sets/7094-1/King-s-Castle-Siege',
    'https://brickset.com/sets/10176-1/King-s-Castle',
    'https://library.ldraw.org/omr/sets/1034',
    'https://library.ldraw.org/omr/sets/1110',
]


def outside(value, low, high):
    return max(low-value, value-high, 0)


def natural_height(x, y):
    castle = max(0, 24-math.ceil(max(outside(x, 168, 312), outside(y, 96, 224))/2))
    neighbour = max(0, 24-math.ceil(max(outside(x, 36, 72), outside(y, 140, 180))/1.5))
    summit = max(0, 28-int(max(abs(x-44), abs(y-160))/2))
    return max(castle, neighbour, summit)


def height(x, y):
    result = natural_height(x, y)
    if 224 <= x < 256 and 80 <= y < 96:result=max(result,25)
    if 156 <= y < 164:
        if any(abs(x-centre) < 2 for centre in range(72, 169, 8)):result = max(result, 21)
        if 72 <= x < 168:
            start=72+8*int((x-72)//8)
            if start not in arch_spans():result=max(result,24)
    return result


@cache
def arch_spans():
    return [start for start in range(72,168,8)
            if max(natural_height(x,y) for x in range(start+1,start+8,2) for y in range(157,164,2))<=21]


def water(x, y):
    return 104 <= x < 376 and 32 <= y < 288 and not (116 <= x < 364 and 44 <= y < 276)


def terrain():
    model = Model()
    raster = [(x, y) for y in range(0, DEPTH, 2) for x in range(0, WIDTH, 2)]
    for wet, colour in [(False, 288), (True, 321)]:
        cells = [(x, y) for x, y in raster if water(x+1, y+1) == wet]
        parts = ['3068b'] if wet else ['41539', '3035', '3032', '3031', '3020', '3022', '3958', '3036']
        for part, x, y, turn in park.rectangles(cells, parts):model.part(part, colour, x, y, 0, turn)
    for level in range(28):
        cells = [(x, y) for x, y in raster if height(x+1, y+1) > level]
        for part, x, y, turn in park.rectangles(cells, ['4204', '4201', '3007', '2456', '3001', '3003'], reverse=bool(level % 2)):
            model.part(part, 72 if level < 20 else 71, x, y, 1/3+level, turn)
    # Piers are part of the height-field union, so they never overlap the slopes.
    spans=arch_spans()
    for start in spans:
        for north in range(156, 164):
            model.definitions['main'].append(instance('part', '3308a.dat', [start+4, north+.5, 24+1/3], 71))
    # One-stud cap rows match the true arch end feet. Wider pier studs remain
    # a full brick below the hollow opening rather than intruding into it.
    for x in sorted({start for start in spans}|{start+7 for start in spans}):
        model.part('3008',71,x,156,21+1/3,turn=True)
    for start in range(72, 168, 16):model.part('4204', 71, start, 156, 24+1/3)
    model.part('4201', 71, 64, 156, 24+1/3)
    for north in [156, 163]:
        for start in range(72, 168, 8):
            model.part('3008', 71, start, north, 25+1/3)
            model.strip(8, 71, start, north, 26+1/3)
    return model.export()


def multiply(a,b):
    return [[sum(a[i][k]*b[k][j] for k in range(4)) for j in range(4)] for i in range(4)]


def pose_key(part,colour,matrix):
    return part,colour,*(round(v,7) for row in matrix[:3] for v in row)


def brick_reference(brick, raise_by=0.0):
    rotation = brick['transform']
    matrix = [[rotation[3*j+i] for j in range(3)]+[brick['position'][i]+(raise_by if i==2 else 0)] for i in range(3)]
    return dict(target=dict(kind='part', id=brick['part']), color=brick['color'], transform=matrix+[[0,0,0,1]])


def native(scene, references, pieces, meshes, name) -> dict:
    return dict(format='ergo-construction-1', schema=2, name=name, geo=scene['geo'],
                model=dict(format='ergo-model-1', schema=2, root='landscape', definitions={'landscape':references}),
                pieces=pieces, meshes=meshes, joints=[], fixed_bricks=[], groups=[], ground=[])


def bridge_route():
    level=RAISE+1
    waypoints=[dict(name='castle-courtyard',feet=[184,160,level]),dict(name='arched-west-gate',feet=[176,160,level])]
    for x in range(168,63,-8):waypoints.append(dict(name=f'bridge-west-{x}',feet=[x,160,level]))
    for x in range(63,44,-2):waypoints.append(dict(name=f'neighbour-summit-{x}',feet=[x,161,1/3+height(x,161)]))
    for x in range(47,64,2):waypoints.append(dict(name=f'neighbour-return-{x}',feet=[x,161,1/3+height(x,161)]))
    for x in range(64,169,8):waypoints.append(dict(name=f'bridge-east-{x}',feet=[x,160,level]))
    waypoints.append(dict(name='castle-return',feet=[184,160,level]))
    return dict(format='ergo-walk-route-1',schema=2,origin_world=[0,0,0],start=[184,160,level],eye_height=3.4,min_clearance=4,speed=8,waypoints=waypoints)


def main():
    raise SystemExit('The completed pre-migration castle conversion is retired. Open the current accepted assembly for editing; this module retains shared terrain/native helpers only.')


if __name__ == '__main__':main()
