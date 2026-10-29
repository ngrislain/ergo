"""ID-preserving height/terrace and symmetric-fence proposals from accepted caches.

Patterns adapt the existing district's bonded masonry, staggered slabs, supported
wing beams and lintels (Brick Builder's Handbook). Terrace references were
inspected through official OMR 10243, Willy Tschager [Holly Wood], CCAL 2.0;
these are original district adaptations, not a replica/import of that set.
Files/proposals do not publish: use authenticated MCP and accepted readbacks.
"""
from collections import Counter
import math
from scripts.native_model import Model,yaw
from scripts.build.monument_models import BASE,PITCH,Masonry,rectangles,stair
from scripts.build import district_models as buildings,district_revisions as existing,district_streets as streets

def fingerprint(part,color,matrix):return (part,color,*[round(value,6) for row in matrix for value in row])

def model_leaves(model,origin=(0,0,0)):
    exported=model.export(origin);result=[]
    def visit(name,parent,color):
        for item in exported['definitions'][name]:
            matrix=existing.multiply(parent,item['transform']);shade=item.get('color',color)
            if shade==16:shade=color
            target=item['target']
            if target['kind']=='definition':visit(target['id'],matrix,shade)
            else:result.append({'target':target,'color':shade,'transform':matrix})
    visit(exported['root'],[[int(i==j) for j in range(4)] for i in range(4)],71)
    return result

def stair_delta(construction,spec):
    """Replace only changed occurrences of the last storey's former upward stair."""
    count=buildings.storeys(spec)
    if count==10:return [],[]
    base=BASE+(count-1)*PITCH;origin=buildings.origin(spec)
    old=Masonry();old.compact_short_rows=True;stair(old,base,True)
    new=Masonry();new.compact_short_rows=True;stair(new,base,False)
    before=Counter(fingerprint(item['target']['id'],item['color'],item['transform']) for item in model_leaves(old,origin))
    next_items=model_leaves(new,origin)
    after=Counter(fingerprint(item['target']['id'],item['color'],item['transform']) for item in next_items)
    removed_counts=before-after;added_counts=after-before;removed=[];added=[]
    for brick in existing.leaves(construction):
        key=fingerprint(brick['part'],brick['color'],brick['transform'])
        if removed_counts[key]>0:removed.append(brick['id']);removed_counts[key]-=1
    assert not +removed_counts,'The accepted stair differs from the known source; stop rather than replacing unrelated bricks'
    for item in next_items:
        key=fingerprint(item['target']['id'],item['color'],item['transform'])
        if added_counts[key]>0:added.append(item);added_counts[key]-=1
    assert not +added_counts
    return removed,added

def height_and_terraces(construction,spec):
    count=buildings.storeys(spec)
    assert 5<=count<=10
    if count==10 and not spec.get('wing_storeys'):return [],Model()
    x,north,_=buildings.origin(spec);cut=BASE+count*PITCH-2/3
    removed,stair_items=stair_delta(construction,spec);removed=set(removed)
    model=Masonry();model.compact_short_rows=True;model.definitions['main'].extend(stair_items)
    doors=[]
    for side,height in enumerate(spec.get('wing_storeys',[])):
        assert 2<=height<count
        lo,hi=(0,2) if side==0 else (30,32);floor=BASE+height*PITCH
        doors.extend([{'min':[x+lo,north+13,floor],'max':[x+hi,north+19,floor+6]},
            {'min':[x+lo,north+12,floor+6],'max':[x+hi,north+20,floor+7]}])
        model.part('3007',spec['wall'],x+lo,north+12,floor+6,turn=True)
    for brick in existing.leaves(construction):
        if brick['id'] in removed:continue
        upper=brick['min'][2]>=cut-1e-5
        wing=False
        for side,height in enumerate(spec.get('wing_storeys',[])):
            outside=(brick['max'][0]<=x+1e-5 and brick['min'][0]<x-1e-5) if side==0 else (brick['min'][0]>=x+32-1e-5 and brick['max'][0]>x+32+1e-5)
            # The existing next-floor beam/slab is already the supported roof
            # of the shortened wing. Keep those exact occurrences and IDs.
            wing |= outside and brick['min'][2]>=BASE+height*PITCH-1e-5
        if upper or wing:removed.add(brick['id']);continue
        cuts=[volume for volume in doors if existing.overlap(brick,volume)]
        if not cuts:continue
        removed.add(brick['id'])
        if any(all(brick['min'][i]>=volume['min'][i]-1e-5 and brick['max'][i]<=volume['max'][i]+1e-5 for i in range(3)) for volume in cuts):continue
        assert abs(brick['max'][2]-brick['min'][2]-1)<1e-5,('Cannot fragment a specialty/window part',brick['id'],brick['part'])
        footprint=tuple(round(brick[edge][axis]) for axis,edge in [(0,'min'),(0,'max'),(1,'min'),(1,'max')])
        holes=[tuple(round(volume[edge][axis]) for axis,edge in [(0,'min'),(0,'max'),(1,'min'),(1,'max')]) for volume in cuts]
        for area in rectangles([footprint],holes):
            model.brickwork([area],brick['min'][2],brick['color'],turn=area[1]-area[0]<area[3]-area[2],phase=4*(round(brick['min'][2]-BASE)%2))
    streets.mount(model,'supported-shortened-roof',buildings.stage(spec,'roof'),x,north,0)
    streets.mount(model,'relocated-crown',buildings.roof_details(spec),x,north,0)
    if spec.get('wing_storeys'):streets.mount(model,'guarded-intermediate-terraces',buildings.terraces(spec),x,north,0)
    # Do not replace a source occurrence merely because a generated roof/repair
    # happens to contain the identical brick in the identical pose and colour.
    unchanged={}
    for brick in existing.leaves(construction):
        if brick['id'] in removed:unchanged.setdefault(fingerprint(brick['part'],brick['color'],brick['transform']),[]).append(brick['id'])
    minimal=Model()
    for item in model_leaves(model):
        key=fingerprint(item['target']['id'],item['color'],item['transform'])
        if unchanged.get(key):removed.remove(unchanged[key].pop())
        else:minimal.definitions['main'].append(item)
    assert removed and len(removed)<=10000
    return sorted(removed),minimal

def symmetric_fences(construction):
    """Rotate only the opposite fence row; bases/decks/supports retain their IDs."""
    bricks=existing.leaves(construction);fences=[brick for brick in bricks if brick['part']=='3633.dat']
    assert len(fences)==28,'Require the accepted fourteen-section fence on each side'
    rows=sorted({round(brick['min'][1],6) for brick in fences});assert len(rows)==2
    selected=[brick for brick in fences if abs(brick['min'][1]-rows[1])<1e-5];assert len(selected)==14
    model=Model()
    for brick in selected:model.part('3633',brick['color'],*brick['min'],rotation=yaw(math.pi))
    return [brick['id'] for brick in selected],model
