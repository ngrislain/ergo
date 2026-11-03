"""Reviewed common-part families, in official LDraw local coordinates (LDU).

Geometry and connector datums come from the pinned official source dependency
closure, not similarly named parts or bounding-box guesses. Envelopes below are
nominal collision proxies. They do not describe exact hollow collision solids.
"""
import math

RECTANGULAR = '3622 3623 4477 2445 3036 3958 3456 92438 3027 6636 4162 26603'.split()
BRICKS = {'6541': 1, '3700': 2, '3701': 4, '3894': 6, '3702': 8, '3895': 12, '3703': 16, '32000': 2}
BEAMS = {'32523': 3, '32316': 5, '32524': 7, '40490': 9, '32525': 11, '41239': 13, '32278': 15}
AXLES = {'3704': 2, '32062': 2, '4519': 3, '3705': 4, '32073': 5, '3706': 6, '44294': 7, '3707': 8, '3737': 10, '3708': 12}
PINS = ['2780', '3673']
BUSHES = ['3713', '4265c']
ACCESSORIES = '30374 87994 4095 63965 3846 3837 4006 3835 3841 3847 3848 3849 3820'.split()
WHEELS = '4600 4624 3641 3482 132'.split()
FIGURES = ['973p4f', '3818', '3819', '4499']
SCENERY = ['3470', '3471']
CASTLE = ['4444', '2345', '3659', '3307', '3308a']
GARRISON = ['4524', '50231c01', '3896', '89520', '4493c01', '4493c02', '4491a']
CANDIDATES = {id + '.dat': id for id in [*RECTANGULAR, *BRICKS, *BEAMS, *AXLES, *PINS, *BUSHES, *ACCESSORIES, *WHEELS, *FIGURES, *SCENERY, *CASTLE, *GARRISON]}
# BrickLink identifies the source moulds under these catalogue spellings.
CANDIDATES['3820.dat'] = '983'
CANDIDATES['132.dat'] = '3483'
CANDIDATES.update({'973p4f.dat': '973px138', '3818.dat': '982', '3819.dat': '981'})
CANDIDATES.update({'3308a.dat': '3308', '50231c01.dat': '522',
                   '4493c01.dat': '4493c01pb02', '4493c02.dat': '4493c01pb04'})


def port(kind, start, end, radius, **extra):
    return dict(kind=kind, start=start, end=end, radius=radius, **extra)


def connectors(id) -> list[dict]:
    """Only explicitly reviewed functional interfaces; no shape-name heuristic."""
    id = id.removesuffix('.dat')
    if id == '973p4f':
        result = []
        angle = math.radians(9.782)
        for sign in [-1, 1]:
            axis = [-sign*math.cos(angle), math.sin(angle), 0]
            origin = [sign*15.552, 9, 0]
            result.append(port('shoulder_socket', origin, [origin[i]+2*axis[i] for i in range(3)], 5))
        return result
    if id in ['3818', '3819']:
        side = 1 if id == '3818' else -1
        mouth = [-side*5, 18.9, -9.9]
        direction = [0, -math.sqrt(.5), math.sqrt(.5)]
        return [port('shoulder_peg', [0, 0, 0], [side*2, 0, 0], 5),
                port('wrist_socket', mouth, [mouth[i]+15*direction[i] for i in range(3)], 2.5, inset=2)]
    if id == '4499':
        # The arrow crosses the central shaft at Y=-12.35. The two 12-LDU
        # grip zones lie on either side of it, not across the arrow itself.
        return [port('bar', [0, -10, 0], [0, 2, 0], 4),
                port('bar', [0, -27, 0], [0, -15, 0], 4)]
    if id in BRICKS:
        n = BRICKS[id]
        xs = [0] if n == 1 else list(range(-10*n+20, 10*n, 20))
        if id == '32000': xs = [-10, 10]
        return [port('round_hole', [x, 10, -10], [x, 10, 10], 8, inset=2) for x in xs]
    if id in BEAMS:
        n = BEAMS[id]
        return [port('round_hole', [0, -10, z], [0, 10, z], 8, inset=2) for z in range(-10*n+10, 10*n, 20)]
    if id in AXLES:
        n = AXLES[id]
        return [port('axle', [-10*n, 0, 0], [10*n, 0, 0], 6, key=[0, 1, 0])]
    if id in PINS:
        # Separate shanks: the centre stop cannot be inserted through a bore.
        return [port('pin', [-20, 0, 0], [-1, 0, 0], 6), port('pin', [1, 0, 0], [20, 0, 0], 6)]
    if id in BUSHES:
        half = 10 if id == '3713' else 5
        return [port('axle_hole', [0, 0, -half], [0, 0, half], 6, key=[1, 0, 0])]
    handles = {
        '30374': [(0, 80)], '87994': [(0, 60)], '4095': [(-120, -4), (0, 12)],
        '63965': [(-102.5, -7.5), (0, 18)], '3837': [(-12, 40)],
        '4006': [(0, 25)], '3835': [(0, 32)], '3841': [(-8, 40)],
        '3847': [(4, 20)], '3848': [(-28, 60)], '3849': [(0, 16)],
        '4497': [(0, 94)], '3957a': [(-88, -4)],
        '3846': [(0, 18)], '3846p4h': [(0, 18)],
        '30152': [(50, 80)],
    }
    if id in handles:
        result = [port('bar', [0, a, 0], [0, b, 0], 4) for a, b in handles[id]]
        if id in ['3846', '3846p4h']:
            result.append(port('bar', [0, 0, -8], [0, 0, 14], 4))
        return result
    if id == '3820':
        # 3820.dat !HELP and the cylindrical grip surface; 14.5 degree axis.
        direction = [0, math.cos(math.radians(14.5)), math.sin(math.radians(14.5))]
        centre = [0, -.8229, -9.8948]
        return [port('grip', [c-5.5*d for c, d in zip(centre, direction)],
                     [c+5.5*d for c, d in zip(centre, direction)], 4),
                port('wrist_peg', [0, 0, 0], [0, 0, 13], 2.5)]
    if id in ['76382p00', '76382p4f']:
        grip = connectors('3820')[0]
        result = []
        for sign in [-1, 1]:
            # Official factory hand placements. Rounded source matrices are
            # normalized by the runtime; they are not independent body poses.
            matrix = [[.985, sign*.1202, -sign*.1202], [-sign*.17, .6964, -.6964], [0, .707, .707]]
            offset = [sign*23.6904, 26.774, -9.8982]
            transform = lambda p: [offset[i]+sum(matrix[i][j]*p[j] for j in range(3)) for i in range(3)]
            result.append(port('grip', transform(grip['start']), transform(grip['end']), 4))
        return result
    if id == '4600':
        return [port('wheel_pin', [s*22, 5, 0], [s*33, 5, 0], 4) for s in [-1, 1]]
    if id in ['4624', '4624c01']:
        return [port('wheel_socket', [0, 0, -2], [0, 0, 8], 4),
                port('rim', [0, 0, -8], [0, 0, 8], 10, group='4624-3641')]
    if id == '3641':
        return [port('tyre', [0, 0, -8], [0, 0, 8], 10, group='4624-3641')]
    if id == '3482':
        return [port('axle_hole', [0, 0, -10], [0, 0, 10], 6, key=[1, 0, 0]),
                port('rim', [0, 0, -10], [0, 0, 10], 21.875, group='3482-3483')]
    if id == '132':
        return [port('tyre', [0, 0, -10], [0, 0, 10], 21.875, group='3482-3483')]
    return []


def nominal(id, bounds):
    id = id.removesuffix('.dat')
    value = {'bounds_ldu': bounds, 'studs_ldu': []}
    if id == '973p4f':
        value['bounds_ldu'] = [[-19, 0, -10], [19, 32, 10]]
    elif id == '3820':
        # Palm core: wrist post and the thin finger-end overhangs are visual.
        # Their receiving/holding datums are represented by explicit ports.
        value['bounds_ldu'] = [[-6, -4, -14], [6, 3, -5]]
    elif id in ['3818', '3819']:
        # As with factory torso cores, omit pegs and inner hanging overhangs
        # from the bearing proxy. Genuine full arm geometry remains rendered.
        value['bounds_ldu'] = [[-9, -4, -9], [-6, 17, 5]] if id == '3818' else [[6, -4, -9], [9, 17, 5]]
    elif id in BRICKS or id == '4600':
        n, depth, height = (BRICKS[id], 1, 24) if id in BRICKS else (2, 2, 8)
        value['bounds_ldu'] = [[-10*n, 0, -10*depth], [10*n, height, 10*depth]]
        value['studs_ldu'] = [[x, 0, z] for x in range(-10*n+10, 10*n, 20) for z in range(-10*depth+10, 10*depth, 20)]
        value['cavity'] = dict(width=n, depth=depth, first=[-10*n+10, -10*depth+10], bottom=height)
    elif id in BEAMS:
        n = BEAMS[id]
        value['bounds_ldu'] = [[-9, -10, -10*n+1], [9, 10, 10*n-1]]
    elif id in SCENERY:
        # Official rounded 2x2 bases contain the central inverted stud4 tube;
        # the four receiving stud centres are +/-10 LDU, at the Y=8 underside.
        value['cavity'] = dict(width=2, depth=2, first=[-10, -10], bottom=8)
        # The base/trunk is the bearing core. Filling the full canopy envelope
        # down to the floor would invent collisions with neighbouring studs.
        value['bounds_ldu'] = [[-20, bounds[0][1], -20], [20, 8, 20]]
    elif id == '4444':
        # The central 3x1 foot/header is the load-bearing core. Filling the
        # stepped wings down to the floor invents collisions with floor studs.
        value['bounds_ldu'] = [[-30, 0, -10], [30, 144, 10]]
        # This mould has stepped stud rows, not a fictional flat 2x5 grid.
        value['studs_ldu'] = [[x, 0, 0] for x in [-20, 0, 20]] + [
            [-40, 24, 0], [40, 24, 0]] + [[x, 8, 20] for x in [-20, 0, 20]]
        value['cavity'] = dict(width=3, depth=1, first=[-20, 0], bottom=144)
    elif id == '2345':
        value['bounds_ldu'] = [[-30, 0, -30], [30, 144, 30]]
        value['studs_ldu'] = [[x, 24, 20] for x in [-20, 0, 20]] + [
            [-20, 24, 0], [-20, 24, -20]]
        value['cavity'] = dict(width=2, depth=2, first=[-20, 0], bottom=144)
    elif id in ['3659', '3307', '3308a']:
        width, height = {'3659': (4, 24), '3307': (6, 48), '3308a': (8, 48)}[id]
        value['bounds_ldu'] = [[-10*width, 0, -10], [10*width, height, 10]]
        value['studs_ldu'] = [[x, 0, 0] for x in range(-10*width+10, 10*width, 20)]
        # Only the two end feet receive studs; the arch opening is empty.
        value['cavity'] = dict(width=1, depth=1, first=[-10*width+10, 0], bottom=height)
        value['additional_cavities'] = [dict(width=1, depth=1, first=[10*width-10, 0], bottom=height)]
    elif id in ['3896', '89520']:
        # As for the existing Castle helmet: retain the actual source upper
        # cap as the bearing core, with cheek/neck protection rendered in full.
        value['bounds_ldu'] = [[-13, bounds[0][1], -13], [13, 0, 13]]
        value['cavity'] = dict(width=1, depth=1, first=[0, 0], bottom=0)
    elif id == '4524':
        # Source neck ring is 2 LDU thick. 6080 places it two LDU above the
        # torso, raising head/helmet by the same amount. Drape is visual.
        value['bounds_ldu'] = [[-10, 0, -10], [10, 2, 10]]
        value['cavity'] = dict(width=1, depth=1, first=[0, 0], bottom=2)
    elif id == '50231c01':
        # Official !HELP: cape at torso Y=0; head raised by 0.6 LDU.
        # This is a fixed formed source shape, not a cloth simulation.
        value['bounds_ldu'] = [[-10, -.6, -10], [10, 0, 10]]
        value['cavity'] = dict(width=1, depth=1, first=[0, 0], bottom=0)
    elif id in ['4493c01', '4493c02']:
        # Factory unit: body/back bears at source Y=0; head and tail remain
        # genuine rendered overhangs. Four separate hoof holes are reviewed
        # in s/4494s01.dat, not one fictitious solid receiving rectangle.
        value['bounds_ldu'] = [[-20, 0, -35], [20, 57, 47]]
        feet = [dict(width=1, depth=1, first=[x, z], bottom=57)
                for z in [-25, 35] for x in [-10, 10]]
        value['cavity'], value['additional_cavities'] = feet[0], feet[1:]
        value['collision_boxes_ldu'] = [[[-20, 0, -35], [20, 20, 47]]] + [
            [[x-10, 14.5, z-10], [x+10, 57, z+(12 if z==35 else 10)]]
            for z in [-25, 35] for x in [-10, 10]]
    elif id == '4491a':
        # The original 6080 assembly seats the saddle 8 LDU above the horse
        # datum. Its actual floor is 0..8 LDU; raised sides/clips are visual.
        value['bounds_ldu'] = [[-20, 0, -10], [20, 8, 10]]
        value['studs_ldu'] = [[-10, 0, 0], [10, 0, 0]]
        value['cavity'] = dict(width=2, depth=1, first=[-10, 0], bottom=8)
    return value


def source_bounds(root, id):
    """Source triangle envelope for standalone tools, axles, rims and tyres.

    Uses the same primitive resolution order as the source baker. No rendering
    proxy is produced. Lines/conditional lines are excluded from the envelope.
    """
    cache = {}
    def points(id, active=()):
        id = id.replace('\\', '/').lower()
        if id in cache: return cache[id]
        if id in active or len(active) > 64: raise ValueError('Cyclic/deep source geometry')
        path = next((root/p/id for p in ['p', 'parts'] if (root/p/id).is_file()), None)
        if path is None: raise ValueError('Missing source geometry '+id)
        result = []
        for line in path.read_bytes().decode('utf-8-sig').splitlines():
            f = line.split()
            if not f: continue
            if f[0] in ['3', '4']:
                result.extend([list(map(float, f[i:i+3])) for i in range(2, len(f), 3)])
            elif f[0] == '1':
                p = list(map(float, f[2:5])); m = list(map(float, f[5:14]))
                for q in points(' '.join(f[14:]), (*active, id)):
                    result.append([p[i]+sum(m[i*3+j]*q[j] for j in range(3)) for i in range(3)])
            if len(result) > 2_000_000: raise ValueError('Source geometry budget exceeded')
        cache[id] = result
        return result
    values = points(id)
    if not values: raise ValueError('Empty source geometry '+id)
    return [[min(p[i] for p in values) for i in range(3)], [max(p[i] for p in values) for i in range(3)]]


def arch_boxes(root, id, width, height):
    """Conservative 4-LDU source-triangle columns, retaining the empty archway."""
    cache = {}
    def triangles(name, active=()):
        name = name.replace('\\', '/').lower()
        if name in cache:return cache[name]
        if name in active or len(active) > 64:raise ValueError('Cyclic/deep arch source')
        path = next((root/p/name for p in ['p', 'parts'] if (root/p/name).is_file()), None)
        if path is None:raise ValueError('Missing arch primitive '+name)
        faces = []
        for line in path.read_bytes().decode('utf-8-sig').splitlines():
            f = line.split()
            if not f:continue
            if f[0] in ['3', '4']:
                p = [list(map(float, f[i:i+3])) for i in range(2, len(f), 3)]
                faces.append(p[:3])
                if f[0] == '4':faces.append([p[0], p[2], p[3]])
            elif f[0] == '1':
                offset, matrix = list(map(float, f[2:5])), list(map(float, f[5:14]))
                for face in triangles(' '.join(f[14:]), (*active, name)):
                    faces.append([[offset[i]+sum(matrix[3*i+j]*p[j] for j in range(3)) for i in range(3)] for p in face])
            if len(faces) > 100_000:raise ValueError('Arch source exceeds the reviewed proxy budget')
        cache[name] = faces
        return faces
    def clip(points, boundary, lower):
        output = []
        for a, b in zip(points, points[1:]+points[:1]):
            inside_a, inside_b = (a[0] >= boundary if lower else a[0] <= boundary), (b[0] >= boundary if lower else b[0] <= boundary)
            if inside_a:output.append(a)
            if inside_a != inside_b:
                t = (boundary-a[0])/(b[0]-a[0])
                output.append([a[i]+t*(b[i]-a[i]) for i in range(3)])
        return output
    faces = triangles(id)
    boxes = []
    for left in range(-10*width, 10*width, 4):
        right = left+4
        depths = []
        for face in faces:
            if max(p[0] for p in face) < left or min(p[0] for p in face) > right:continue
            points = clip(clip(face, left, True), right, False)
            depths.extend(p[1] for p in points)
        bottom = min(height, max(depths, default=0))
        if bottom <= 0:continue
        if boxes and abs(boxes[-1][1][1]-bottom) < 1e-8:
            boxes[-1][1][0] = right
        else:boxes.append([[left, 0, -10], [right, bottom, 10]])
    return boxes


def enrich(entry, root):
    id = entry.get('profile_id', entry['id'])
    ports = connectors(id)
    if ports: entry['connectors'] = ports
    if id in CANDIDATES and entry['size'] is None:
        entry['nominal'] = nominal(id, source_bounds(root, entry['id']))
        if id in ['3659', '3307', '3308a']:
            width, height = {'3659': (4, 24), '3307': (6, 48), '3308a': (8, 48)}[id]
            entry['nominal']['collision_boxes_ldu'] = arch_boxes(root, entry['id'], width, height)
