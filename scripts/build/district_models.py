"""Original minifigure-scale district, adapting the accepted tower recipe.

Bond/floor source: https://brickbuildershandbook.com/basic-lego-techniques/
"""
from scripts.build.monument_models import BASE, COURSES, PITCH, Masonry, rectangles, stair, tower_walls

STAGES = ['foundation', *[f'floor-{level:02}' for level in range(1, 11)], 'roof']

CENTRE_TOWER = '88c288ddd45cda14e5b51cbbd116f5243610b1b08f883923ce0dabda050deb2f'
ORIGIN = (19968, 64, 0)
SPECS = [
    {'key': '01-azure-terrace', 'name': 'Azure Terrace Pyramid', 'cell': (-2, -1), 'kind': 'pyramid', 'wall': 321, 'trim': 15},
    {'key': '02-copper-needle', 'name': 'Copper Needle Tower', 'cell': (-1, -1), 'kind': 'tower', 'wall': 320, 'trim': 14},
    {'key': '03-white-slab', 'name': 'White Slab House', 'cell': (0, -1), 'kind': 'slab', 'wall': 15, 'trim': 72},
    {'key': '04-emerald-court', 'name': 'Emerald Courtyard Tower', 'cell': (1, -1), 'kind': 'court', 'wall': 288, 'trim': 19},
    {'key': '05-ochre-cascade', 'name': 'Ochre Cascade Pyramid', 'cell': (-2, 0), 'kind': 'pyramid', 'wall': 19, 'trim': 28},
    {'key': '06-civic-bell', 'name': 'Civic Bell Tower', 'cell': (-1, 0), 'kind': 'tower', 'wall': 71, 'trim': 15},
    {'key': '07-crimson-gallery', 'name': 'Crimson Gallery House', 'cell': (1, 0), 'kind': 'slab', 'wall': 320, 'trim': 19},
    {'key': '08-sandstone-ziggurat', 'name': 'Sandstone Ziggurat', 'cell': (-2, 1), 'kind': 'pyramid', 'wall': 28, 'trim': 14},
    {'key': '09-colonnade-exchange', 'name': 'Colonnade Exchange', 'cell': (-1, 1), 'kind': 'columns', 'wall': 15, 'trim': 19},
    {'key': '10-north-arch', 'name': 'North Arch Court', 'cell': (0, 1), 'kind': 'slab', 'wall': 70, 'trim': 15},
    {'key': '11-silver-observatory', 'name': 'Silver Observatory', 'cell': (1, 1), 'kind': 'round', 'wall': 72, 'trim': 71},
]
assert len(SPECS) == 11 and len({spec['cell'] for spec in SPECS}) == 11


def origin(spec):
    east, north = spec['cell']
    return [ORIGIN[0] + east * 96, ORIGIN[1] + north * 96, ORIGIN[2]]


def footprint(spec):
    if spec['kind'] in ['pyramid', 'court']:
        return (-16, 48, -16, 48)
    if spec['kind'] == 'slab':
        return (-16, 48, 0, 32)
    return (0, 32, 0, 32)


def foundation(spec):
    x, hi_x, north, hi_north = footprint(spec)
    model = Masonry()
    model.slab([(x - 4, hi_x + 4, north - 4, hi_north + 4)], BASE, 71)
    return model


def stage(spec, name):
    if name == 'foundation':
        return foundation(spec)
    model = Masonry()
    model.compact_short_rows = True
    main = [(0, 32, 0, 32)]
    well = [(10, 14, 6, 16), (18, 22, 6, 16), (10, 22, 16, 20)]
    if name.startswith('floor-'):
        level = int(name.removeprefix('floor-')) - 1
        assert 0 <= level < 10
        base = BASE + level * PITCH
        if level:
            model.slab(main, base, spec['trim'], well)
        for course in range(COURSES):
            color = spec['trim'] if course == 8 else spec['wall']
            tower_walls(model, False, course, base + course, color, level == 0)
        stair(model, base, level < 9)
        return model
    if name == 'roof':
        top = BASE + 10 * PITCH
        # Close only the actual stairwell, on its three masonry stringers.
        # The monument's wider 4..28 closure depends on its heavy crown to
        # counterbalance rear plates; it is unsuitable for an unweighted deck.
        for layer, holes, phase in [(0, well, 4), (1, [(8, 24, 6, 20)], 0)]:
            for lo, hi, north, hi_north in rectangles(main, holes):
                xs = [lo] + [value for value in range(lo + 1, hi) if value % 8 == phase] + [hi]
                ns = [north] + [value for value in range(north + 1, hi_north) if value % 8 == phase] + [hi_north]
                for x, xhi in zip(xs, xs[1:]):
                    for n, nhi in zip(ns, ns[1:]):
                        model.plate_rectangle(xhi - x, nhi - n, spec['trim'], x, n, top - (2 - layer) / 3)
        for north in range(6, 20, 2):
            model.part('4282', spec['trim'], 8, north, top - 1 / 3)
        return model
    raise ValueError(f'Unimplemented district stage: {name}')
