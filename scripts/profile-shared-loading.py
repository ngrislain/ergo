#!/usr/bin/env python3
"""Read-only transfer/timing comparison; never print construction geometry."""
import argparse,gzip,json,re,time,urllib.error,urllib.parse,urllib.request
from pathlib import Path

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--url',default='https://ergosandbox.com')
    parser.add_argument('--world')
    parser.add_argument('--cell',nargs=3,type=int,default=[19,0,0])
    parser.add_argument('--output',type=Path)
    args=parser.parse_args();base=args.url.rstrip('/');parsed=urllib.parse.urlsplit(base)
    if parsed.username or parsed.password or parsed.query or parsed.fragment:parser.error('Use an origin without credentials/query/fragment')
    if args.output and not args.output.parent.is_dir():parser.error('Output parent must exist')
    if not args.world:
        with urllib.request.urlopen(base+'/api/config',timeout=120) as response:args.world=json.load(response)['common_world']
    x,y,z=args.cell;path=f'/api/shared-worlds/{args.world}/view?x={x}&y={y}&z={z}'
    results=[];etag=None
    for name,format,encoding,conditional in [('json-identity','json','identity',False),('json-gzip','json','gzip',False),
        ('cbor-gzip','cbor','gzip',False),('cbor-gzip-repeat','cbor','gzip',False),('conditional','cbor','gzip',True)]:
        headers={'Accept-Encoding':encoding}
        if conditional:
            if not etag:continue
            headers['If-None-Match']=etag
        start=time.monotonic()
        try:response=urllib.request.urlopen(urllib.request.Request(base+path+'&format='+format,headers=headers),timeout=300)
        except urllib.error.HTTPError as error:
            if error.code!=304:raise
            response=error
        with response:
            first=time.monotonic()-start;body=response.read();elapsed=time.monotonic()-start
            decoded=gzip.decompress(body) if body and response.headers.get('Content-Encoding')=='gzip' else body
            row={'request':name,'status':response.status,'ttfb_s':round(first,3),'total_s':round(elapsed,3),
                'body_transfer_s':round(elapsed-first,3),
                'wire_bytes':len(body),'decoded_bytes':len(decoded),'content_type':response.headers.get('Content-Type'),
                'content_encoding':response.headers.get('Content-Encoding','identity'),'read_cache':response.headers.get('X-Ergo-Read-Cache'),
                'server_timing':response.headers.get('Server-Timing')}
            row['server_phase_ms']={stage:float(duration) for stage,duration in re.findall(r'(view(?:_[a-z]+)*);dur=([0-9.]+)',row['server_timing'] or '')}
            if format=='cbor':etag=response.headers.get('ETag') or etag
            results.append(row);print(json.dumps(row),flush=True)
    if args.output:args.output.write_text(json.dumps({'origin':base,'world':args.world,'cell':args.cell,'results':results},indent=2)+'\n')

if __name__=='__main__':main()
