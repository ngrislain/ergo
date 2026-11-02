#!/usr/bin/env python3
"""Build plate-free constructs and reposed guards/archers from source datums."""
import argparse
import copy
import json
import math
import pathlib
import common_parts

ROOT = pathlib.Path(__file__).resolve().parents[1]
PATH = ROOT/'crates/world/library/constructs.json'


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
    grip = common_parts.connectors(torso['part'])[hand]
    handle = common_parts.connectors(part)[handle_index]
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
    grip = common_parts.connectors('3820')[0]
    handle = common_parts.connectors(part)[handle_index]
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


def articulated_garrison(old):
    template = old['medieval-guard-v1']
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
    old = {c['id']: c for c in entries}
    additions = []
    for name in ['minifigure', 'medieval-guard']:
        value = copy.deepcopy(old[name+'-v1'])
        value['id'] = name+'-v2'
        value['parts'] = [p for p in value['parts'] if p['part'] not in ['3031.dat', '3070b.dat']]
        for part in value['parts']: part['position'][2] -= 1/3
        if name == 'medieval-guard':
            torso = next(p for p in value['parts'] if p['part'] == '76382p4f.dat')
            value['parts'] = [p for p in value['parts'] if p['part'] not in ['3846p4h.dat', '4497.dat']]
            value['parts'].extend([held('3846p4h.dat', torso, 0, 4, handle_index=1), held('4497.dat', torso, 1, 70, .8)])
            value['description'] = 'Plate-free Lion Knights guard. Genuine shield and spear align with the factory hand grips; feet stand directly on the ground stud grid. Instance colour selects the torso. Connector capacity remains separately calibrated.'
        else:
            value['description'] = 'Plate-free standing minifigure with genuine hair. Factory feet register directly on the ground stud grid. Instance colour selects the torso; navy legs, yellow head and black hair are fixed.'
        additions.append(value)
    car = copy.deepcopy(old['classic-car-v1'])
    car['id'] = 'classic-car-v2'
    car['description'] = 'Plate-free classic open-cabin car with four correctly seated rotating wheel bearings and two genuine 4600 wheel holders. Tyres stand directly on the ground; the internal chassis plate is structural. No road tile or display pedestal is added. Instance colour selects the body.'
    for part in car['parts']:
        if part['part'] == '4624c01.dat':
            if part['position'][1] < 2: part['position'][1] = .475
            else: part['position'][1] = 3.525; part['quarter_turns'] = 2
        else:
            part['position'][2] += 1/8
    car['parts'].extend([dict(part='4600.dat', color=0, position=[x, 2, 23/24], quarter_turns=1) for x in [2, 6]])
    # Two internal axle spacers connect each low wheel-holder floor to the
    # chassis above the tyre crowns. These are vehicle structure, not a base.
    car['parts'].extend([dict(part='3022.dat', color=0, position=[x, 2, height]) for x in [2, 6] for height in [31/24, 39/24]])
    additions.append(car)
    additions.extend(articulated_garrison(old))
    return [c for c in entries if c['id'] not in {v['id'] for v in additions}]+additions


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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    result = encoded(generate(json.loads(PATH.read_text())))
    if args.check:
        if result != PATH.read_text(): raise SystemExit('Common constructs differ from source-datum regeneration')
    else: PATH.write_text(result)


if __name__ == '__main__': main()
