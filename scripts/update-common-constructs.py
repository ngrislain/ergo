#!/usr/bin/env python3
"""Regenerate current guard/archer source datums; validate the current-only library."""
import argparse
import copy
import json
import math
import pathlib

ROOT = pathlib.Path(__file__).resolve().parents[1]
PATH = ROOT/'crates/world/library/constructs.json'
REVIEWS = {review['id']: review for review in json.loads((ROOT/'crates/world/library/reviews.json').read_text())['parts']}


def connectors(part):
    """Reviewed source-LDU connector datums of one part."""
    return REVIEWS[part if part.endswith('.dat') else part + '.dat']['connectors']


def dot(a, b): return sum(x*y for x, y in zip(a, b))
def sub(a, b): return [x-y for x, y in zip(a, b)]
def add(a, b): return [x+y for x, y in zip(a, b)]
def mul(m, p): return [dot(row, p) for row in m]
def unit(p): return [x/math.sqrt(dot(p, p)) for x in p]
def public(p): return [p[0]/20, p[2]/20, -p[1]/24]
def metric(p): return [p[0], p[2], -p[1]]
def centre(c): return [(a+b)/2 for a, b in zip(c['start'], c['end'])]


def rotation(a, b):
    a, b = unit(a), unit(b)
    v = [a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]]
    c = dot(a, b)
    if c < -1+1e-10: raise ValueError('Antiparallel grip requires an explicit roll datum')
    k = [[0, -v[2], v[1]], [v[2], 0, -v[0]], [-v[1], v[0], 0]]
    return [[(1 if i == j else 0)+k[i][j]+sum(k[i][n]*k[n][j] for n in range(3))/(1+c) for j in range(3)] for i in range(3)]


def held(part, torso, hand, color, along=.5, handle_index=0, reverse=False, roll=0):
    grip = connectors(torso['part'])[hand]
    handle = connectors(part)[handle_index]
    direction = metric(sub(grip['end'], grip['start']))
    if reverse: direction = [-x for x in direction]
    r = rotation(metric(sub(handle['end'], handle['start'])), direction)
    if roll:
        x, y, z = unit(direction); c, s = math.cos(roll), math.sin(roll); t = 1-c
        turn = [[t*x*x+c,t*x*y-s*z,t*x*z+s*y], [t*x*y+s*z,t*y*y+c,t*y*z-s*x], [t*x*z-s*y,t*y*z+s*x,t*z*z+c]]
        r = [[sum(turn[i][k]*r[k][j] for k in range(3)) for j in range(3)] for i in range(3)]
    p = [a+(b-a)*along for a, b in zip(handle['start'], handle['end'])]
    scale = [.008, .008, .0096]
    matrix = [[r[i][j]*scale[j]/scale[i] for j in range(3)] for i in range(3)]
    position = sub(add(torso['position'], public(centre(grip))), mul(matrix, public(p)))
    return dict(part=part, color=color, position=position, rotation=r)


def multiply(a, b):
    return [[sum(a[i][k]*b[k][j] for k in range(3)) for j in range(3)] for i in range(3)]


def rx(angle):
    c, s = math.cos(angle), math.sin(angle)
    return [[1, 0, 0], [0, c, -s], [0, s, c]]


def rz(angle):
    c, s = math.cos(angle), math.sin(angle)
    return [[c, -s, 0], [s, c, 0], [0, 0, 1]]


def source_part(id, position, orientation, torso, color):
    # Source LDU X/Y/Z -> public metric east/north/up is a proper rotation.
    p = [[1, 0, 0], [0, 0, 1], [0, -1, 0]]
    pt = [list(row) for row in zip(*p)]
    return dict(part=id, position=add(torso['position'], public(position)),
                rotation=multiply(multiply(p, orientation), pt), color=color)


def articulated_limbs(torso, relaxed_right=False):
    """Official shoulder/wrist datums, with hands rolled to a vertical grip.

    Arm 3818/3819 !HELP supplies the shoulder position, elevation order and
    9.782-degree splay. Wrist mouths and their 45-degree axes come from 3818s01.
    Rotate the genuine arm and hand separately; never move an accessory away
    from a fixed factory hand just to make the weapon stand upright.
    """
    limbs, hands = [], []
    beta = math.radians(14.5)
    for sign, arm_id in [(-1, '3818.dat'), (1, '3819.dat')]:
        splay = -sign*math.radians(9.782)
        phi = math.asin(-math.sin(beta)/math.cos(splay))
        elevation = phi-math.pi/4
        roll = math.atan2(-math.sin(splay), math.cos(splay)*math.cos(phi))
        if sign == -1 and relaxed_right:
            elevation, roll = 0, 0
        arm = multiply(rz(splay), rx(elevation))
        hand = multiply(multiply(arm, rx(math.pi/4)), rz(roll))
        shoulder = [sign*15.552, 9, 0]
        wrist = add(shoulder, mul(arm, [sign*5, 18.9, -9.9]))
        limbs.append(source_part(arm_id, shoulder, arm, torso, 1))
        hands.append(source_part('3820.dat', wrist, hand, torso, 14))
    return limbs, hands


def held_by_hand(part, hand, color, handle_index=0, along=.5, roll=0.0):
    grip = connectors('3820')[0]
    handle = connectors(part)[handle_index]
    scale = [.008, .008, .0096]
    hr = hand['rotation']
    hm = [[hr[i][j]*scale[j]/scale[i] for j in range(3)] for i in range(3)]
    target = add(hand['position'], mul(hm, public(centre(grip))))
    direction = mul(hr, metric(sub(grip['end'], grip['start'])))
    r = rotation(metric(sub(handle['end'], handle['start'])), direction)
    if roll:
        x, y, z = unit(direction); c, s = math.cos(roll), math.sin(roll); t = 1-c
        turn = [[t*x*x+c,t*x*y-s*z,t*x*z+s*y], [t*x*y+s*z,t*y*y+c,t*y*z-s*x], [t*x*z-s*y,t*y*z+s*x,t*z*z+c]]
        r = multiply(turn, r)
    matrix = [[r[i][j]*scale[j]/scale[i] for j in range(3)] for i in range(3)]
    p = [a+(b-a)*along for a, b in zip(handle['start'], handle['end'])]
    return dict(part=part, color=color, position=sub(target, mul(matrix, public(p))), rotation=r)


def articulated_garrison(current):
    template = current['medieval-guard-v3']
    torso = dict(part='973p4f.dat', position=[2, 1.5, 3])
    arms, hands = articulated_limbs(torso)
    common = [dict(part='3815c01.dat', color=0, position=[2, 1.5, 5/3]), torso,
              *arms, *hands, dict(part='3626ap01.dat', color=14, position=[2, 1.5, 4]),
              dict(part='3844.dat', color=72, position=[2, 1.5, 4])]
    guard = dict(id='medieval-guard-v3', name='Lion Knights guard with reposed wrists',
        description='Plate-free guard with independently posed genuine arms and hands. The right hand closes around an upright spear shaft; the left holds the shield handle with its face forward. Instance colour selects the printed torso. Joint strength requires separate calibration.',
        credits=template['credits'], parts=[*copy.deepcopy(common), held_by_hand('4497.dat', hands[0], 70), held_by_hand('3846p4h.dat', hands[1], 4)])
    archer_arms, archer_hands = articulated_limbs(torso, relaxed_right=True)
    archer_common = [*copy.deepcopy(common[:2]), *archer_arms, *archer_hands, *copy.deepcopy(common[6:])]
    archer = dict(id='medieval-archer-v1', name='Lion Knights archer with genuine bow and arrow',
        description='Plate-free archer carrying genuine 4499 bow with arrow in the left hand, using a grip zone beside the arrow crossing. Independently posed source arms and wrists keep the bow upright. Instance colour selects the printed torso. This is a ready/carry pose, not a drawn bow.',
        credits=template['credits'], parts=[*archer_common, held_by_hand('4499.dat', archer_hands[1], 70, roll=-math.pi/2)])
    return [guard, archer]


def generate(entries):
    current = {c['id']: c for c in entries}
    expected = {'castle-standard-v1', 'minifigure-v2', 'classic-car-v2',
                'medieval-guard-v3', 'medieval-archer-v1'}
    if set(current) != expected or len(entries) != len(expected):
        raise ValueError('Only the five current common constructs are supported')
    updated = {entry['id']: entry for entry in articulated_garrison(current)}
    result = [updated.get(entry['id'], copy.deepcopy(entry)) for entry in entries]
    for entry in result:
        if any(part['part'] not in REVIEWS for part in entry['parts']):
            raise ValueError('Common constructs must reference reviewed library parts')
    return result


def encoded(entries):
    # Keep each component pose on one line for straightforward source review.
    result = []
    for entry in entries:
        lines = ['  {']
        lines += ['    '+json.dumps(k)+':'+json.dumps(v, ensure_ascii=False, separators=(',', ':'))+',' for k, v in entry.items() if k != 'parts']
        lines += ['    "parts":[']
        lines += ['      '+json.dumps(p, ensure_ascii=False, separators=(',', ':'))+(',' if i+1 < len(entry['parts']) else '') for i, p in enumerate(entry['parts'])]
        lines += ['    ]', '  }']
        result.append('\n'.join(lines))
    return '[\n'+',\n'.join(result)+'\n]\n'


def same_poses(left, right):
    """Libm may differ in the last bits across macOS/Linux; keep stored IDs/poses.

    Only numeric differences below 1e-12 are tolerated. Names, component lists,
    colours, transforms and all non-numeric metadata must still agree.
    """
    if isinstance(left, (int, float)) and isinstance(right, (int, float)):
        return math.isclose(left, right, rel_tol=0, abs_tol=1e-12)
    if isinstance(left, list) and isinstance(right, list):
        return len(left) == len(right) and all(same_poses(a, b) for a, b in zip(left, right))
    if isinstance(left, dict) and isinstance(right, dict):
        return left.keys() == right.keys() and all(same_poses(left[k], right[k]) for k in left)
    return left == right


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    result = encoded(generate(json.loads(PATH.read_text())))
    if args.check:
        if not same_poses(json.loads(result), json.loads(PATH.read_text())):
            raise SystemExit('Common constructs differ from source-datum regeneration')
    else: PATH.write_text(result)


if __name__ == '__main__': main()
