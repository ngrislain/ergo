#!/usr/bin/env python3
"""MCP checks of free-angle articulation: a guard posed about his shoulder,
a Technic chain hung between pinned A-frames that settles under gravity into
its funicular (catenary) shape and is then publishable, and the same
unbalanced chain held in place by friction pins."""
import argparse
import math
import os
import pathlib
import tempfile

import library_fixture
from mcp_client import Mcp
from native_model import instance

ROOT = pathlib.Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', type=pathlib.Path, default=ROOT / 'target/debug/ergo-mcp')
args = parser.parse_args()
BINARY = str(args.binary.resolve())
library_fixture.offline_mcp(os.environ, pathlib.Path(tempfile.mkdtemp(prefix='ergo-library-')))

HOLE = 0.7  # height of a 3703 hole above the floor (studs)


def piece(part, colour, x, y, h, rotation):
    """A part centred at (x, y, h) in isotropic studs, as a model instance in
    public units (x, y studs on cell centres, z in 1.2-stud bricks)."""
    return instance('part', part, [x, y - 0.5, h / 1.2], colour, rotation)


def cross_pin(x, y, h, part='3673.dat'):
    """A pin or axle whose axis runs along world y."""
    return piece(part, 0, x, y, h, [[0, -1, 0], [1, 0, 0], [0, 0, 1]])


def beam(part, colour, a, b, y):
    """A liftarm in the xz plane whose end holes sit on points a and b."""
    dx, dh = b[0] - a[0], b[1] - a[1]
    n = math.hypot(dx, dh)
    dx, dh = dx / n, dh / n
    # Columns: local x = d x axis, local y = d (along the holes), local z = axis (world y).
    rotation = [[-dh, dx, 0], [0, 0, 1], [dx, dh, 0]]
    return piece(part, colour, (a[0] + b[0]) / 2, y, (a[1] + b[1]) / 2, rotation)


def chain_model(middle_degrees=8.0, link_pin='3673.dat'):
    """Two pinned A-frames on mirrored 3703 bases carry a three-link chain of
    5-hole liftarms joined by `link_pin` between their apex axles."""
    leg = 4.0
    entries = [piece('3703.dat', 71, 0, y, 1.2, [[1, 0, 0], [0, 1, 0], [0, 0, 1]]) for y in (0.5, 5.5)]
    apexes = []
    for centre in (-5, 5):
        apex = (centre, HOLE + leg * math.sqrt(3) / 2)
        apexes.append(apex)
        for foot, layers, pins in (((centre - 2, HOLE), (1.5, 4.5), (1, 5)),
                                   ((centre + 2, HOLE), (-0.5, 6.5), (0, 6))):
            for y in layers:
                entries.append(beam('32316.dat', 1, foot, apex, y))
            for y in pins:
                entries.append(cross_pin(foot[0], y, foot[1]))
        entries.append(cross_pin(apex[0], 3, apex[1], '3707.dat'))
    # Three links closing the 10-stud span, deliberately asymmetric.
    span = apexes[1][0] - apexes[0][0]
    phi = math.radians(middle_degrees)
    cx, sy = span / leg - math.cos(phi), -math.sin(phi)
    mean, half = math.atan2(sy, cx), math.acos(math.hypot(cx, sy) / 2)
    angles = [mean + half, phi, mean - half]
    points = [apexes[0]]
    for a in angles:
        points.append((points[-1][0] + leg * math.cos(a), points[-1][1] - leg * math.sin(a)))
    assert math.dist(points[-1], apexes[1]) < 1e-9, points
    for (a, b), y in zip(zip(points, points[1:]), (2.5, 3.5, 2.5)):
        entries.append(beam('32316.dat', 4, a, b, y))
    for p in points[1:-1]:
        entries.append(cross_pin(p[0], 3, p[1], link_pin))
    return {'format': 'ergo-model-1', 'schema': 2, 'root': 'main', 'definitions': {'main': entries}}


with tempfile.TemporaryDirectory(prefix='ergo-articulation-') as temporary:
    directory = pathlib.Path(temporary)

    # 1. A guard holding spear and shield publishes, and his arm turns freely
    # about the shoulder while the hand keeps its spear.
    client = Mcp(BINARY, directory / 'guard.ergo.json', None, None)
    try:
        model = {'format': 'ergo-model-1', 'schema': 2, 'root': 'main', 'definitions': {'main': [
            {'target': {'kind': 'construct', 'id': 'medieval-guard-v3'}, 'color': 4,
             'transform': [[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]]}]}}
        placed = client.call('place_submodel', model=model)['structuredContent']
        assert placed['publication_ready'] is True, placed
        bricks = client.call('list_bricks')['structuredContent']['bricks']
        arm = next(b for b in bricks if b['part'] in ('3818.dat', '3819.dat'))
        pivots = client.call('inspect_brick', id=arm['id'])['structuredContent']['pivots']
        shoulder = next(p for p in pivots if 'shoulder' in p['connection'])
        assert shoulder['turns_with'] and len(shoulder['turns_with']) >= 3, shoulder  # arm, hand, spear
        turned = client.call('rotate_about_pivot', id=arm['id'], pivot_id=shoulder['pivot_id'],
                             angle_radians=-0.25)['structuredContent']
        assert turned['publication_ready'] is True, turned
        assert set(turned['rotated_piece_ids']) == set(shoulder['turns_with'])
    finally:
        client.close()

    # 2. A Technic chain on free pins settles into its catenary and publishes.
    client = Mcp(BINARY, directory / 'catenary.ergo.json', None, None)
    try:
        before = client.call('place_submodel', model=chain_model())['structuredContent']
        assert before['publication_ready'] is False, 'An asymmetric free-pin chain is a mechanism'
        settled = client.call('simulate_gravity', seconds=8)['structuredContent']
        assert settled['settled']['projection_residual_m'] < 1e-6, settled['settled']
        assert settled['publication_ready'] is True, settled
        links = sorted((b for b in client.call('list_bricks')['structuredContent']['bricks']
                        if b['part'] == '32316.dat' and b['color'] == 4), key=lambda b: b['position'][0])
        # Funicular shape of three equal links over a 10-stud span: the middle
        # link hangs level, 4 sin(acos 0.75) studs below the outer link centres
        # midway; the outer links sit 7 studs apart.
        (x0, _, z0), (x1, _, z1), (x2, _, z2) = (b['position'] for b in links)
        drop = 2 * math.sqrt(1 - 0.75 ** 2) / 1.2  # studs -> bricks
        assert abs(x2 - x0 - 7) < 0.01 and abs(x1 - (x0 + x2) / 2) < 0.01, links
        assert abs(z0 - z2) < 0.01 and abs((z0 + z2) / 2 - z1 - drop) < 0.01, links
    finally:
        client.close()

    # 3. Black friction pins resist rotation: the same unbalanced chain holds.
    client = Mcp(BINARY, directory / 'friction.ergo.json', None, None)
    try:
        held = client.call('place_submodel', model=chain_model(link_pin='2780.dat'))['structuredContent']
        assert held['publication_ready'] is True, held
        print('Articulation passed: guard posed about his shoulder; chain settled', settled['settled'],
              'and friction pins held the unbalanced chain')
    finally:
        client.close()
