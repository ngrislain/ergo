#!/usr/bin/env python3
"""Source-referenced Castle mountain/moat/bridge proposal; no implicit publication.

Ideas are adapted from LEGO 31168, 6080, 7094 and 10176 at the existing castle's
scale. Parts keep their genuine dimensions. The castle and its supporting terrain
form a single replacement of the existing assembly so no unsupported intermediate
state is published.
"""
import argparse
import base64
import collections
import json
import math
from functools import cache
import pathlib
import subprocess
import sys
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from scripts.build import park
from scripts.build.district import write
from scripts.native_model import Model, PARTS, CONSTRUCTS, instance, model_leaves, yaw
from scripts.mcp_client import Mcp

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


def voxels(brick):
    bounds = brick['bounds']
    low, high = bounds['min'], bounds['max']
    assert all(abs(v-round(v)) < 1e-6 for v in low+high), brick['id']
    return {(x, y, z) for x in range(round(low[0]), round(high[0]))
            for y in range(round(low[1]), round(high[1]))
            for z in range(round(low[2]), round(high[2]))}


def prism(low, high):
    return {(x, y, z) for x in range(low[0], high[0]) for y in range(low[1], high[1]) for z in range(low[2], high[2])}


def pack_unit_cells(model, cells, colour):
    """Exact 1-stud voxel cover, preserving masonry outside each changed opening."""
    shapes = []
    for part in ['4204', '4201', '3007', '2456', '3001', '3003', '3008', '3010', '3009', '3004', '3005']:
        w, d, h = PARTS[part]
        if h != 1:continue
        shapes.extend([(int(w*d), part, int(w), int(d), False), (int(w*d), part, int(d), int(w), True)])
    shapes.sort(reverse=True)
    for z in sorted({p[2] for p in cells}):
        layer = {(x, y) for x, y, level in cells if level == z}
        while layer:
            x, y = min(layer, key=lambda p: (p[1], p[0]) if z % 2 == 0 else (p[0], p[1]))
            for _, part, w, d, turn in shapes:
                footprint = {(x+a, y+b) for a in range(w) for b in range(d)}
                if footprint <= layer:
                    layer -= footprint;model.part(part, colour, x, y, z, turn);break
            else:raise ValueError('Cannot cover masonry repair')


def architecture(scene):
    changed = set()
    fill = set()
    additions = Model()
    # Arched south entrance and a matching west doorway opening onto the bridge.
    gates = [([236, 100, 1], [244, 108, 12], 'south'), ([172, 156, 1], [180, 164, 12], 'west')]
    for low, high, facing in gates:
        changed |= prism(low, high)
        if facing == 'south':
            for x in [236, 243]:
                for y in range(100, 108):
                    for z in range(1, 10):fill.add((x, y, z))
            for y in range(100, 108):additions.definitions['main'].append(instance('part', '3308a.dat', [240, y+.5, 12], 71))
        else:
            for y in [156, 163]:
                for x in range(172, 180):
                    for z in range(1, 10):fill.add((x, y, z))
            for x in range(172, 180):additions.definitions['main'].append(instance('part', '3308a.dat', [x+.5, 160, 12], 71, yaw(math.pi/2)))
    # Use the actual 6080 wall mould and its narrow arched arrow opening. Behind
    # each front panel, the slit continues through the remaining thick masonry.
    panels = []
    for x in [186, 194, 202, 226, 246, 274, 282, 290]:panels.append((x, 100, 'south'))
    for x in [186, 194, 202, 226, 234, 242, 250, 274, 282, 290]:panels.append((x, 212, 'north'))
    for y in [114, 122, 150, 166, 190, 198]:
        panels.extend([(172, y, 'west'), (300, y, 'east')])
    for x, y, facing in panels:
        for base in [9, 18]:
            if facing in ['south', 'north']:
                changed |= prism([x, y, base], [x+5, y+8, base+6])
                changed |= prism([x-1, y, base+6], [x+7, y+8, base+7])
                additions.part('4201',71,x-1,y,base+6)
                for level in range(base, base+6):
                    for east in range(x, x+5):
                        for north in range(y+(2 if facing=='south' else 0), y+(8 if facing=='south' else 6)):
                            if east == x+2 and base+3 <= level < base+5:continue
                            fill.add((east, north, level))
                position = [x+2.5, y+.5 if facing=='south' else y+7.5, base+6]
                angle = 0 if facing=='south' else math.pi
            else:
                changed |= prism([x, y, base], [x+8, y+5, base+6])
                changed |= prism([x, y-1, base+6], [x+8, y+7, base+7])
                additions.part('4201',71,x,y-1,base+6)
                for level in range(base, base+6):
                    for east in range(x+(2 if facing=='west' else 0), x+(8 if facing=='west' else 6)):
                        for north in range(y, y+5):
                            if north == y+2 and base+3 <= level < base+5:continue
                            fill.add((east, north, level))
                position = [x+.5 if facing=='west' else x+7.5, y+2.5, base+6]
                angle = -math.pi/2 if facing=='west' else math.pi/2
            additions.definitions['main'].append(instance('part', '4444.dat', position, 71, yaw(angle)))
    selected, remaining = [], collections.defaultdict(set)
    for brick in scene['bricks']:
        id = brick['part'].removesuffix('.dat')
        if id not in PARTS or PARTS[id][2] != 1 or brick['color'] not in [71, 72]:continue
        body = voxels(brick)
        if body & changed:
            selected.append(brick['id']);remaining[brick['color']] |= body-changed
    for colour, cells in remaining.items():pack_unit_cells(additions, cells, colour)
    pack_unit_cells(additions, fill, 71)
    return additions.export(), selected, dict(wall_panels=len(panels)*2, arched_entrances=len(gates))


def multiply(a,b):
    return [[sum(a[i][k]*b[k][j] for k in range(4)) for j in range(4)] for i in range(4)]


def guard_placements(model):
    result=[]
    def visit(name,parent,colour,active):
        assert name not in active
        for ref in model['definitions'][name]:
            matrix=multiply(parent,ref['transform']);tint=ref.get('color',colour);target=ref['target']
            if target==dict(kind='construct',id='medieval-guard-v1'):result.append(dict(transform=matrix,color=tint))
            elif target['kind']=='definition':visit(target['id'],matrix,tint,(*active,name))
    visit(model['root'],[[int(i==j) for j in range(4)] for i in range(4)],16,())
    return result


def pose_key(part,colour,matrix):
    return part,colour,*(round(v,7) for row in matrix[:3] for v in row)


def varied_garrison(scene,additions,plate_free=False):
    by_pose=collections.defaultdict(list)
    for brick in scene['bricks']:
        ref=brick_reference(brick);by_pose[pose_key(brick['part'],brick['color'],ref['transform'])].append(brick)
    placements=guard_placements(scene['model'])
    if len(placements)!=300:raise ValueError('Expected the complete accepted 300-guard baseline; inspect or rebase this recipe before applying it again')
    guards=[];capes=0;removed=[]
    for index,placement in enumerate(placements):
        components={}
        for part in CONSTRUCTS['medieval-guard-v1']['parts']:
            local=instance('part',part['part'],part['position'],part.get('color',placement['color']),yaw(part.get('quarter_turns',0)*math.pi/2))
            matches=by_pose[pose_key(part['part'],local['color'],multiply(placement['transform'],local['transform']))]
            assert matches,part['part']
            components[part['part']]=matches.pop()
        guards.append(components)
        if plate_free:
            # The legacy shield/spear depended on individual display supports.
            # Stand the sourced factory figure directly on the architecture;
            # unsupported held weapons remain deferred, never glued by fiat.
            excluded={'3031.dat','3070b.dat','3846p4h.dat','4497.dat'}
            removed.extend(brick['id'] for part,brick in components.items() if part in excluded)
            for part,brick in components.items():
                if part not in excluded:brick['position'][2]-=1/3
        helmet=components['3844.dat'];helmet['part']=['3844.dat','3896.dat','89520.dat'][index%3]
        lift=0
        if index%3!=2:
            cape='4524.dat' if index%3==0 else '50231c01.dat';lift=2/24 if cape=='4524.dat' else .6/24
            local=instance('part',cape,[2,1.5,10/3-(1/3 if plate_free else 0)+(lift if cape=='4524.dat' else 0)],[4,1,0][index%3])
            additions.definitions['main'].append(dict(local,transform=multiply(placement['transform'],local['transform'])))
            capes+=1
        for part in [components['3626ap01.dat'],helmet]:
            part['position'][2]+=lift
            if index%11==0 or index%13==0:
                angle=math.pi/2 if index%11==0 else -math.pi/2
                old=brick_reference(part)['transform'];rotation=instance('part',part['part'],[0,0,0],part['color'],yaw(angle))['transform']
                turned=multiply(rotation,old)
                part['transform']=[turned[i][j] for j in range(3) for i in range(3)]
    # Three existing courtyard guards move coherently into the new stable
    # forecourt. Their original local IDs and relative equipment poses remain.
    lowest=min(group['3031.dat']['bounds']['min'][2] for group in guards)
    moved=0
    for group,target in zip([g for g in guards if abs(g['3031.dat']['bounds']['min'][2]-lowest)<1e-6][:3],[(228,82),(236,82),(244,82)]):
        base=group['3031.dat'];dx=target[0]-base['bounds']['min'][0];dy=target[1]-base['bounds']['min'][1]
        for brick in group.values():brick['position'][0]+=dx;brick['position'][1]+=dy
        # Added cape is associated with the moved torso; move it by matching
        # its original centre before the final scene is materialized.
        torso=group['76382p4f.dat']
        for ref in additions.definitions['main']:
            if ref['target']['id'] not in ['4524.dat','50231c01.dat']:continue
            pos=ref['transform']
            if abs(pos[0][3]-(torso['position'][0]-dx))<1e-6 and abs(pos[1][3]-(torso['position'][1]-dy))<1e-6 and abs(pos[2][3]-torso['position'][2])<.1:
                pos[0][3]+=dx;pos[1][3]+=dy
        moved+=1
    for index,x in enumerate([226,234,242,250]):
        additions.definitions['main'].append(instance('part','4493c01.dat' if index%2==0 else '4493c02.dat',[x,88.75,1+57/24],0 if index%2==0 else 15))
        additions.definitions['main'].append(instance('part','4491a.dat',[x,88.75,1+65/24],[4,14][index%2]))
    return dict(guards=len(guards),capes=capes,helmet_variants=3,horses=4,repositioned_guards=moved,
                plate_free=plate_free,armed_guards=0 if plate_free else len(guards),
                removed_display_bases=len(guards) if plate_free else 0),removed


def brick_reference(brick, raise_by=0.0):
    rotation = brick['transform']
    matrix = [[rotation[3*j+i] for j in range(3)]+[brick['position'][i]+(raise_by if i==2 else 0)] for i in range(3)]
    return dict(target=dict(kind='part', id=brick['part']), color=brick['color'], transform=matrix+[[0,0,0,1]])


def native(scene, references, pieces, meshes, name) -> dict:
    return dict(format='ergo-construction-1', schema=2, name=name, geo=scene['geo'],
                model=dict(format='ergo-model-1', schema=2, root='landscape', definitions={'landscape':references}),
                pieces=pieces, meshes=meshes, joints=[], fixed_bricks=[], groups=[], ground=[])


def prepare_native(source, added, removed, landscape, url):
    scene=source['scene']
    assert not scene['joints'] and not scene['fixed_bricks'] and not scene['groups'], 'Preserve/remap authored constraints explicitly'
    meshes=dict(scene['meshes'])
    colours={}
    with urllib.request.urlopen(url.rstrip('/')+'/api/catalog',timeout=180) as response:catalogue=json.load(response)
    colours={colour['code']:colour['rgb'] for colour in catalogue['colors']}
    added_leaves=list(model_leaves(added));terrain_leaves=list(model_leaves(landscape))
    for part in sorted(({leaf['part'] for leaf in added_leaves+terrain_leaves}|{brick['part'] for brick in scene['bricks']})-meshes.keys()):
        with urllib.request.urlopen(url.rstrip('/')+'/api/parts/'+part,timeout=180) as response:asset=json.load(response)
        assert asset['mesh']['quality']=='source',part
        meshes[part]=asset['mesh']
    removed_set=set(removed)
    retained=[brick for brick in scene['bricks'] if brick['id'] not in removed_set]
    references=[brick_reference(brick,RAISE) for brick in retained]
    pieces=[dict(id=brick['id'],rgb=brick['rgb'],step=brick['step']) for brick in retained]
    next_id=max(brick['id'] for brick in scene['bricks'])+1
    for leaf in added_leaves:
        matrix=[list(row) for row in leaf['transform']];matrix[2][3]+=RAISE
        references.append(dict(target=dict(kind='part',id=leaf['part']),color=leaf['color'],transform=matrix))
        pieces.append(dict(id=next_id,rgb=colours[leaf['color']],step=0));next_id+=1
    castle=native(scene,references,pieces,meshes,scene['name'])
    terrain_refs=[dict(target=dict(kind='part',id=leaf['part']),color=leaf['color'],transform=leaf['transform']) for leaf in terrain_leaves]
    terrain_pieces=[dict(id=i+1,rgb=colours[leaf['color']],step=0) for i,leaf in enumerate(terrain_leaves)]
    ground=native(scene,terrain_refs,terrain_pieces,meshes,'Monts de la Couronne — douves et pont')
    # Retained castle local IDs keep their birth identities on replacement.
    # Supporting terrain belongs to this same atomic scene, using fresh IDs.
    combined_pieces=pieces+[dict(piece,id=next_id+i) for i,piece in enumerate(terrain_pieces)]
    combined=native(scene,references+terrain_refs,combined_pieces,meshes,scene['name'])
    return castle,ground,combined


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
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input', type=pathlib.Path, required=True)
    parser.add_argument('--output', type=pathlib.Path, required=True)
    parser.add_argument('--url', default='https://ergosandbox.com')
    parser.add_argument('--validate', action='store_true')
    parser.add_argument('--varied-garrison', action='store_true')
    parser.add_argument('--plate-free-garrison', action='store_true', help='Standing figures without legacy display bases, shield stops or unsupported weapons')
    args = parser.parse_args()
    source = json.loads(args.input.read_text())
    assert source['assembly_id'] == CASTLE and not source['external_dependencies']
    if args.output.exists():raise ValueError('Preserve the existing proposal and any publication journal; use a fresh output directory')
    args.output.mkdir(parents=True)
    target = args.output/'architecture.model.json'
    if target.exists():raise ValueError('Preserve existing proposal artifacts')
    model, removed, counts = architecture(source['scene'])
    if args.varied_garrison or args.plate_free_garrison:
        # Clone accepted arrays before preparing an authorized proposal.
        source=json.loads(json.dumps(source))
        builder=Model();builder.definitions=model['definitions']
        garrison_counts,garrison_removed=varied_garrison(source['scene'],builder,args.plate_free_garrison)
        counts.update(garrison_counts);removed.extend(garrison_removed);model=builder.export()
        assert len(removed)==len(set(removed)), 'A retained occurrence cannot be removed twice'
    write(target, model);write(args.output/'replace-ids.json', removed)
    landscape=terrain();write(args.output/'terrain.model.json', landscape)
    castle,ground,combined=prepare_native(source,model,removed,landscape,args.url)
    write(args.output/'castle.native.json',castle);write(args.output/'terrain.native.json',ground)
    write(args.output/'combined.native.json',combined);write(args.output/'bridge.route.json',bridge_route())
    write(args.output/'proposal.json', dict(published=False, base=source['base_revision'], castle=CASTLE,
                                           castle_raise=RAISE, references=REFERENCES, **counts))
    print(json.dumps(dict(published=False, architecture_added=sum(1 for _ in model_leaves(model)),
                          architecture_removed=len(removed), terrain_pieces=len(ground['pieces']),combined_pieces=len(combined['pieces']), **counts)), flush=True)
    if args.validate:
        client=Mcp(str(ROOT/'target/debug/ergo-mcp'),args.output/'combined.native.json',None,None)
        try:
            feedback=client.call('validate_construction')['structuredContent'];write(args.output/'validation.json',feedback)
            print(json.dumps(feedback),flush=True)
            if not feedback['publication_ready']:raise ValueError('Castle proposal is not publication-ready')
            # Persist the same normalized nominal datums that passed validation
            # before materializing this proposal for ordinary REST admission.
            client.call('export_construction')
            walk=subprocess.run([str(ROOT/'target/debug/examples/check_walk'),str(args.output/'combined.native.json'),str(args.output/'bridge.route.json')],capture_output=True,text=True)
            write(args.output/'walking.result.json',json.loads(walk.stdout) if walk.returncode==0 else dict(passed=False,error=walk.stderr))
            if walk.returncode:raise ValueError('Bridge walking check failed: '+walk.stderr)
            image=client.call('test_visual_appearance',width=1024,height=768,lighting='studio',camera=dict(position_world=[460,-180,250],target_world=[200,158,40],vertical_fov_degrees=45))
            (args.output/'overview.png').write_bytes(base64.b64decode(next(c['data'] for c in image['content'] if c['type']=='image')))
            image=client.call('test_visual_appearance',width=1024,height=768,lighting='studio',camera=dict(position_world=[-120,-80,200],target_world=[180,155,38],vertical_fov_degrees=45))
            (args.output/'bridge-overview.png').write_bytes(base64.b64decode(next(c['data'] for c in image['content'] if c['type']=='image')))
            for name,eye,target in [('arrowslits',[203,62,54],[194,100,42]),
                                    ('garrison-forecourt',[262,61,44],[239,88,28])]:
                image=client.call('test_visual_appearance',width=1024,height=768,lighting='studio',camera=dict(position_world=eye,target_world=target,vertical_fov_degrees=36))
                (args.output/(name+'.png')).write_bytes(base64.b64decode(next(c['data'] for c in image['content'] if c['type']=='image')))
        finally:client.close()


if __name__ == '__main__':main()
