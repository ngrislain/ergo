#!/usr/bin/env python3
"""Remove isolated car parking pads in a fresh accepted-street proposal; no implicit write."""
import argparse
import copy
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from scripts.build import castle_landscape as native_helpers, district_streets as streets
from scripts.native_model import Model, PARTS, PART_LIBRARY, model_leaves


def flush_road(scene):
    """Replace only the top plate-course below cars with smooth, flush road tiles."""
    cx, north, _ = streets.CENTRE
    cut = {(cx+x+dx,north+n+dn,2) for x,n in streets.PARKING for dx in range(8) for dn in range(4)}
    removed, remaining = set(), set()
    rgb = None
    for brick in scene['bricks']:
        part = brick['part'].removesuffix('.dat')
        if part not in PARTS or brick['color'] != 72:
            continue
        lo, hi = brick['bounds']['min'], brick['bounds']['max']
        if lo[2] >= 1-1e-6 or hi[2] <= 2/3+1e-6:
            continue
        low = [lo[0],lo[1],lo[2]*3];high = [hi[0],hi[1],hi[2]*3]
        if any(abs(v-round(v))>1e-6 for v in low+high):
            raise ValueError('Road masonry is not on the stud/plate grid')
        cells = {(x,n,z) for x in range(round(low[0]),round(high[0]))
                 for n in range(round(low[1]),round(high[1])) for z in range(round(low[2]),round(high[2]))}
        if cells & cut:
            removed.add(brick['id']);remaining |= cells-cut;rgb=brick['rgb']
    if not removed or rgb is None:
        raise ValueError('No supporting road course found below the parking pads')
    model = Model()
    shapes = []
    for part, (w,d,h) in PARTS.items():
        entry = PART_LIBRARY['parts'][part+'.dat']['catalogue']
        hp = round(h*3)
        if part+'.dat' not in scene['meshes'] or not entry['studs'] or hp not in [1,3]:
            continue
        for a,b,turn in [(round(w),round(d),False),(round(d),round(w),True)]:
            shapes.append((a*b*hp,hp,part,a,b,turn))
    shapes.sort(reverse=True)
    ordered = iter(sorted(remaining,key=lambda p:(p[2],p[1],p[0])))
    while remaining:
        point = next(ordered)
        while point not in remaining:point=next(ordered)
        x,n,z=point
        for _,hp,part,w,d,turn in shapes:
            cells={(x+a,n+b,z+c) for a in range(w) for b in range(d) for c in range(hp)}
            if cells <= remaining:
                model.part(part,72,x,n,z/3,turn=turn);remaining-=cells;break
        else:raise ValueError('Cannot rebuild the road with current source parts')
    for x,n in streets.PARKING:
        for offset in range(4):
            for dx in [0,4]:model.part('2431',72,cx+x+dx,north+n+offset,2/3)
    result=[b for b in scene['bricks'] if b['id'] not in removed]
    next_id=max(b['id'] for b in scene['bricks'])+1
    for leaf in model_leaves(model.export()):
        matrix=leaf['transform'];mesh=scene['meshes'][leaf['part']]
        def bounds(box):
            import itertools
            points=[[sum(matrix[i][j]*p[j] for j in range(3))+matrix[i][3] for i in range(3)]
                    for p in itertools.product(*zip(box['min'],box['max']))]
            return dict(min=[min(p[i] for p in points) for i in range(3)],max=[max(p[i] for p in points) for i in range(3)])
        result.append(dict(id=next_id,part=leaf['part'],color=72,rgb=rgb,step=0,
            position=[matrix[i][3] for i in range(3)],transform=[matrix[i][j] for j in range(3) for i in range(3)],
            bounds=bounds(mesh['body_bounds']),render_bounds=bounds(mesh['bounds'])))
        next_id+=1
    scene['bricks']=result
    return dict(replaced_road_bricks=len(removed),added_flush_road_parts=len(model.definitions['main']),flush_road_height=1)


def remove_parking_pads(source):
    scene = copy.deepcopy(source['scene'])
    cx, north, _ = streets.CENTRE
    removed, moved, bays = set(), set(), []
    for index, (east, offset) in enumerate(streets.PARKING):
        x, n = cx + east, north + offset
        inside = [b for b in scene['bricks']
                  if b['bounds']['min'][0] >= x - 1e-6
                  and b['bounds']['max'][0] <= x + 8 + 1e-6
                  and b['bounds']['min'][1] >= n - 1e-6
                  and b['bounds']['max'][1] <= n + 4 + 1e-6]
        pads = [b for b in inside if b['part'] == '2431.dat' and b['color'] == 72
                and abs(b['bounds']['min'][2] - 1) < 1e-6
                and abs(b['bounds']['max'][2] - 4 / 3) < 1e-6]
        car = [b for b in inside if b['bounds']['min'][2] >= 4 / 3 - 1e-6
               and b['bounds']['max'][2] <= 8 + 1e-6]
        wheels = [b for b in car if b['part'] == '4624c01.dat']
        if len(pads) != 8 or len(wheels) != 4 or len(car) != 21:
            raise ValueError(f'Parking bay {index} differs from the accepted inventory; inspect before editing')
        car_ids = {b['id'] for b in car}
        if moved & car_ids or any(b['id'] in scene['fixed_bricks'] for b in car):
            raise ValueError('Car occurrences overlap another bay or have authored anchors')
        for joint in scene['joints']:
            if (joint['a'] in car_ids) != (joint['b'] in car_ids):
                raise ValueError('A car has an external authored joint; preserve its actual load path explicitly')
            if joint['a'] in car_ids:
                joint['anchor'][2] -= 1 / 3
        for brick in car:
            brick['position'][2] -= 1 / 3
            for key in ['bounds', 'render_bounds']:
                brick[key]['min'][2] -= 1 / 3
                brick[key]['max'][2] -= 1 / 3
        moved.update(car_ids)
        removed.update(b['id'] for b in pads)
        bays.append(dict(index=index,removed_pad_tiles=len(pads),lowered_car_pieces=len(car)))
    scene['bricks'] = [b for b in scene['bricks'] if b['id'] not in removed]
    road = flush_road(scene)
    scene['model'] = None
    report = dict(published=False,assembly_id=source['assembly_id'],base_revision=source['base_revision'],
                  removed_parking_tiles=len(removed),lowered_car_pieces=len(moved),car_count=len(bays),
                  lowering_bricks=1/3,retained_car_ids=True,bays=bays,**road)
    return scene, report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input',type=pathlib.Path,required=True)
    parser.add_argument('--output',type=pathlib.Path,required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True,exist_ok=False)
    source = json.loads(args.input.read_text())
    scene, report = remove_parking_pads(source)
    (args.output/'scene.json').write_text(json.dumps(scene,separators=(',',':'))+'\n')
    document = native_helpers.native(scene,[native_helpers.brick_reference(b) for b in scene['bricks']],
        [dict(id=b['id'],rgb=b['rgb'],step=b['step']) for b in scene['bricks']],scene['meshes'],scene['name'])
    document.update(joints=scene['joints'],fixed_bricks=scene['fixed_bricks'],groups=scene['groups'])
    (args.output/'construction.native.json').write_text(json.dumps(document,separators=(',',':'))+'\n')
    (args.output/'proposal.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report),flush=True)


if __name__ == '__main__':
    main()
