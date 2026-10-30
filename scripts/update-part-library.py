#!/usr/bin/env python3
"""Reproducibly lock the canonical catalogue to official LDraw + BrickLink metadata.

Geometry is verified from one complete official archive, not a rolling mix of
mirrors. The manifest has no wall-clock fields; unchanged inputs yield identical
bytes. Changes to existing accepted constructions require a separate retrofit.
"""
import argparse
import concurrent.futures
import hashlib
import html.parser
import json
import os
import pathlib
import re
import subprocess
import tempfile
import urllib.request
import zipfile

ROOT=pathlib.Path(__file__).resolve().parents[1]
OFFICIAL='https://library.ldraw.org/library/official'
ARCHIVE='https://library.ldraw.org/library/updates/complete.zip'
DEFAULT=ROOT/'crates/world/library/sources.lock.json'
PACK=ROOT/'crates/assets/library/parts.pack'
BAKER='ergo-ldraw-mesh-1'

def part_revision(entry):return digest(encoded({'source_revision':entry['source_revision'],'vertex_hash':entry.get('vertex_hash'),
    'mesh_hash':entry.get('asset',{}).get('mesh_hash'),'baker':BAKER}))

def encoded(value):return (json.dumps(value,sort_keys=True,ensure_ascii=False,indent=2)+'\n').encode()
def digest(value):return hashlib.sha256(value).hexdigest()
def source_text(raw):return raw.decode('utf-8-sig').replace('\r\n','\n').replace('\r','\n')
def name(value):
    value=value.replace('\\','/').strip().lower()
    if not value or value.startswith('/') or any(part in ['','..','.'] for part in value.split('/')):raise ValueError('Invalid source reference')
    return value

def download(destination):
    """One fixed authority; stream a bounded complete archive and verify its ZIP."""
    destination.parent.mkdir(parents=True,exist_ok=True)
    request=urllib.request.Request(ARCHIVE,headers={'User-Agent':'Ergo part-library updater/1'})
    with urllib.request.urlopen(request,timeout=120) as response:
        if not response.url.startswith('https://library.ldraw.org/'):raise ValueError('Unexpected source authority redirect')
        size=0
        with tempfile.NamedTemporaryFile(dir=destination.parent,delete=False) as temporary:
            try:
                while block:=response.read(1024*1024):
                    size+=len(block)
                    if size>300*1024*1024:raise ValueError('Official archive exceeds 300 MiB')
                    temporary.write(block)
                temporary.flush()
                archive=pathlib.Path(temporary.name)
                with zipfile.ZipFile(archive) as zipped:
                    if zipped.testzip() is not None:raise ValueError('Corrupt official archive')
                archive.replace(destination)
            except BaseException:
                pathlib.Path(temporary.name).unlink(missing_ok=True);raise
    return digest(destination.read_bytes())

def extract(archive,destination):
    destination.mkdir(parents=True,exist_ok=True);destination=destination.resolve()
    with zipfile.ZipFile(archive) as zipped:
        total=0
        for item in zipped.infolist():
            if item.is_dir():continue
            relative=pathlib.PurePosixPath(item.filename.replace('\\','/'))
            if relative.is_absolute() or '..' in relative.parts:raise ValueError('Invalid archive member')
            if (item.external_attr>>16)&0o170000==0o120000:raise ValueError('Archive links are not permitted')
            total+=item.file_size
            if item.file_size>8*1024*1024 or total>2*1024*1024*1024:raise ValueError('Archive expansion exceeds bounds')
            path=destination.joinpath(*relative.parts)
            if not path.resolve().is_relative_to(destination):raise ValueError('Archive escapes destination')
            path.parent.mkdir(parents=True,exist_ok=True);path.write_bytes(zipped.read(item))
    root=destination/'ldraw'
    if not (root/'parts').is_dir():raise ValueError('Expected official ldraw/parts archive layout')
    return root

class PageText(html.parser.HTMLParser):
    def __init__(self):super().__init__();self.text=[]
    def handle_data(self,data):self.text.append(data)

def metadata_masses(parts):
    urls={url for part in parts for url in re.findall(r'https://www\.bricklink\.com/v2/catalog/catalogitem\.page\?P=[A-Za-z0-9]+',part['mass_source'])}
    def fetch(url):
        request=urllib.request.Request(url,headers={'User-Agent':'Ergo authoritative catalogue updater/1'})
        with urllib.request.urlopen(request,timeout=30) as response:
            if not response.url.startswith('https://www.bricklink.com/'):raise ValueError('Unexpected metadata authority')
            raw=response.read(4*1024*1024+1)
            if len(raw)>4*1024*1024:raise ValueError('Metadata page exceeds bounds')
        parser=PageText();parser.feed(raw.decode('utf-8',errors='replace'))
        value=re.search(r'Weight:\s*([0-9.]+)\s*g',' '.join(parser.text))
        if not value or not 0<float(value[1])<10000:raise ValueError('Missing or invalid authoritative mass: '+url)
        return url,float(value[1])
    with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:values=dict(pool.map(fetch,sorted(urls)))
    for part in parts:
        urls=re.findall(r'https://www\.bricklink\.com/v2/catalog/catalogitem\.page\?P=[A-Za-z0-9]+',part['mass_source'])
        if not urls:raise ValueError('No BrickLink mass source for '+part['id'])
        grams=0
        for url in urls:
            id=url.rsplit('=',1)[-1];quantity=re.search(r'(\d+)\s*x\s*'+re.escape(id)+r'\b',part['mass_source'])
            grams+=values[url]*(int(quantity[1]) if quantity else 1)
        part['mass_kg']=grams/1000

def generate(source_root,previous,archive_hash=None,refresh_masses=False):
    source_root=source_root.resolve()
    metadata=[dict(value['catalogue']) for value in previous['parts'].values()]
    if refresh_masses:metadata_masses(metadata)
    # Official "Moved to" records are the authority for aliases. Similar bounds
    # alone never collapse a different mould or a printed part.
    moved={};alias_sources={}
    for path in sorted((source_root/'parts').glob('*.dat')):
        with path.open('rb') as stream:header=stream.readline(4096)
        match=re.fullmatch(rb'0\s+~Moved to\s+([A-Za-z0-9_-]+)(?:\.dat)?\s*',header.strip())
        if match:
            target=match[1].decode().removesuffix('.dat')+'.dat';moved[path.name]=target;alias_sources[path.name]=path
    def canonical(id):
        seen=set()
        while id in moved:
            if id in seen:raise ValueError('Cyclic official part alias')
            seen.add(id)
            text=source_text(alias_sources[id].read_bytes())
            instances=[line.split() for line in text.splitlines() if line.startswith('1 ')]
            if len(instances)!=1 or [float(value) for value in instances[0][2:14]]!=[0,0,0,1,0,0,0,1,0,0,0,1]:
                raise ValueError('Official alias changes the part datum; an explicit pose migration is required: '+id)
            id=moved[id]
        return id
    selected={}
    for entry in metadata:
        original=entry['id'];id=canonical(original)
        if entry['size'] is None:entry.setdefault('profile_id',original)
        entry['id']=id
        if id in selected:raise ValueError('Selected records collapse to the same canonical part: '+id)
        selected[id]=entry
    aliases={id:[] for id in selected}
    for alias in sorted(moved):
        target=alias;visited=set()
        while target in moved:
            if target in visited:raise ValueError('Cyclic official part alias')
            visited.add(target);target=moved[target]
        if target in selected:
            if canonical(alias)!=target:raise ValueError('Invalid official alias chain')
            aliases[target].append(alias)
    for entry in metadata:
        entry['aliases']=aliases[entry['id']]
    files={};parts={}
    for entry in sorted(metadata,key=lambda item:item['id']):
        pending=[(entry['id'],True)];visited=set();read_bytes=0
        while pending:
            id,top=pending.pop();id=name(id)
            if id in visited:continue
            candidates=['parts/'+id] if top or id.startswith('s/') else ['p/'+id,'parts/'+id]
            selected=next((relative for relative in candidates if (source_root/relative).is_file()),None)
            if selected is None:raise ValueError('Missing official source '+id)
            path=(source_root/selected).resolve()
            if not path.is_relative_to(source_root):raise ValueError('Source escapes the official library')
            text=source_text(path.read_bytes());raw=text.encode();read_bytes+=len(raw)
            if len(raw)>1024*1024 or read_bytes>32*1024*1024 or len(visited)>=4096:raise ValueError('Source dependency budget exceeded')
            record={'path':selected,'sha256':digest(raw)}
            if id in files and files[id]!=record:raise ValueError('Ambiguous official source '+id)
            files[id]=record;visited.add(id)
            if top:
                description=text.splitlines()[0].removeprefix('0 ').strip()
                if entry['size'] is not None:
                    dimensions=re.match(r'^(Brick|Plate|Tile)\s+(\d+)\s+x\s+(\d+)(?:\s|$)',description)
                    if not dimensions:raise ValueError('Rectangular source classification changed: '+id)
                    a,b=int(dimensions[2]),int(dimensions[3])
                    entry['size']=[max(a,b),min(a,b),3 if dimensions[1]=='Brick' else 1]
                    entry['studs']=dimensions[1]!='Tile'
                    entry['name']=f'{dimensions[1]} {a} × {b}'
                else:entry['name']=description
            for line in text.splitlines():
                fields=line.split()
                if fields and fields[0]=='1':
                    if len(fields)<15:raise ValueError('Invalid source reference in '+id)
                    pending.append((name(' '.join(fields[14:])),False))
        for alias in entry['aliases']:
            path=alias_sources[alias];raw=source_text(path.read_bytes()).encode()
            files[alias]={'path':'parts/'+alias,'sha256':digest(raw)};visited.add(alias)
        data={'catalogue':entry,'files':{id:files[id] for id in sorted(visited)}}
        source_revision=digest(encoded(data));old=previous['parts'].get(entry['id'],{})
        vertex_hash=old.get('vertex_hash') if old.get('source_revision',old.get('revision'))==source_revision else None
        locked={'source_revision':source_revision,'files':sorted(visited),'catalogue':entry}
        if vertex_hash:
            locked['vertex_hash']=vertex_hash
            if old.get('asset'):locked['asset']=old['asset']
        locked['revision']=part_revision(locked);parts[entry['id']]=locked
    return {'format':'ergo-part-library-lock-1','authorities':{'geometry':OFFICIAL,'metadata':'https://www.bricklink.com'},
        'source_archive_sha256':archive_hash,'normalization':'UTF-8 without BOM; CRLF/CR to LF; all other text preserved','parts':parts,'files':files}

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-dir',type=pathlib.Path,help='Root of an extracted official LDraw library')
    parser.add_argument('--download-archive',type=pathlib.Path,help='Fetch the complete official archive into this private operator cache')
    parser.add_argument('--extract-to',type=pathlib.Path,help='Extract a downloaded archive under this directory')
    parser.add_argument('--archive-sha256',help='Previously verified complete archive SHA-256')
    parser.add_argument('--refresh-masses',action='store_true',help='Take current masses from the recorded BrickLink sources')
    parser.add_argument('--bake',action='store_true',help='Reproducibly bake and lock the full vertex fingerprints')
    parser.add_argument('--rebake',action='store_true',help='Explicitly rebuild fingerprints after a reviewed baker algorithm update')
    parser.add_argument('--output',type=pathlib.Path,default=DEFAULT)
    parser.add_argument('--check',action='store_true',help='Fail if regeneration differs; do not modify the lock')
    args=parser.parse_args();archive_hash=args.archive_sha256
    if args.download_archive:
        archive_hash=download(args.download_archive)
        if not args.extract_to:parser.error('--download-archive requires --extract-to')
        args.source_dir=extract(args.download_archive,args.extract_to)
    if args.source_dir is None:parser.error('Provide --source-dir or download/extract arguments')
    if args.archive_sha256 and archive_hash!=args.archive_sha256:raise ValueError('Archive checksum differs from the requested snapshot')
    previous=json.loads((args.output if args.output.exists() else DEFAULT).read_text())
    archive_hash=archive_hash or previous.get('source_archive_sha256')
    lock=generate(args.source_dir,previous,archive_hash,args.refresh_masses)
    if args.rebake:
        if not args.bake:parser.error('--rebake requires --bake')
        for entry in lock['parts'].values():entry.pop('vertex_hash',None);entry.pop('asset',None);entry['revision']=part_revision(entry)
    value=encoded(lock)
    if args.bake:
        if args.output.resolve()!=DEFAULT.resolve():parser.error('--bake uses the checked-in library output')
        before=args.output.read_bytes()
        old_pack=PACK.read_bytes()
        if args.check and value!=before:raise SystemExit('Source/metadata lock differs before bake verification')
        try:
            if not args.check:args.output.write_bytes(value)
            env=dict(os.environ,ERGO_ASSET_LIBRARY_DIR=str(args.source_dir.resolve()),ERGO_ASSET_FETCH='0')
            with tempfile.TemporaryDirectory(dir=PACK.parent) as directory:
                artifact=pathlib.Path(directory)/'parts.pack'
                hashes=json.loads(subprocess.check_output(['cargo','run','--quiet','--locked','-p','ergo-assets','--example','part_fingerprints','--',str(artifact)],cwd=ROOT,env=env,text=True))
                new_pack=artifact.read_bytes()
            for id,entry in lock['parts'].items():entry.update(hashes[id]);entry['revision']=part_revision(entry)
            if args.check:
                if new_pack!=old_pack:raise ValueError('Baked canonical asset pack differs')
            else:PACK.write_bytes(new_pack)
            value=encoded(lock)
        except BaseException:
            if not args.check:args.output.write_bytes(before)
            if not args.check:PACK.write_bytes(old_pack)
            raise
    if args.check:
        if not args.output.exists() or args.output.read_bytes()!=value:raise SystemExit('Part library lock differs from deterministic regeneration')
    else:
        args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_bytes(value)
    lock=json.loads(value)
    print(json.dumps({'checked':args.check,'parts':len(lock['parts']),'source_files':len(lock['files']),'sha256':digest(value),'authorities':lock['authorities'],
        'packed_mesh_bytes':PACK.stat().st_size,
        'canonical_aliases':{alias:id for id,entry in lock['parts'].items() for alias in entry['catalogue']['aliases']}}))
if __name__=='__main__':main()
