#!/usr/bin/env python3
"""Authenticated landmark publication with a fresh durable MCP journal; never replay a write."""
import argparse
import hashlib
import json
import os
import pathlib
import shutil
import subprocess
from mcp_client import Mcp

ROOT=pathlib.Path(__file__).resolve().parents[1]
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--construction',type=pathlib.Path,required=True)
parser.add_argument('--token-file',type=pathlib.Path,required=True)
parser.add_argument('--url',default='https://ergosandbox.com')
parser.add_argument('--output',type=pathlib.Path,required=True)
parser.add_argument('--publish',action='store_true',help='Explicitly submit one validated new assembly')
parser.add_argument('--expected-base',help='Fence a manually reconciled recovery to its original accepted revision')
parser.add_argument('--validator',type=pathlib.Path,default=ROOT/'target/release/examples/validate_native',help='Optimized read-only static validator')
args=parser.parse_args()
args.output.mkdir(parents=True,exist_ok=False)
for name in ['ERGO_SHARED_ASSEMBLY_ID','ERGO_POSITION_WORLD','ERGO_WEB_DIR']:
    os.environ.pop(name,None)
os.environ['ERGO_MCP_HTTP_TIMEOUT_SECONDS']='1800'
checked=subprocess.run([str(args.validator),str(args.construction)],capture_output=True,text=True,check=True)
validation=json.loads(checked.stdout)
(args.output/'validation.json').write_text(json.dumps(validation,indent=2)+'\n')
if not validation.get('publication_ready'):raise SystemExit('Local static validation is not publication-ready')
preflight=Mcp(str(ROOT/'target/debug/ergo-mcp'),args.output/'preflight.native.json',args.url,args.token_file)
try:
    allowance=preflight.call('get_edit_allowance')['structuredContent']
    (args.output/'allowance.json').write_text(json.dumps(allowance,indent=2)+'\n')
    print(json.dumps(dict(authenticated_read=True,allowance=allowance)),flush=True)
finally:preflight.close()
if not args.publish:raise SystemExit(0)
workspace=args.output/'publication.native.json'
shutil.copyfile(args.construction,workspace)
attempt=dict(publication_state='not_attempted',construction_sha256=hashlib.sha256(args.construction.read_bytes()).hexdigest(),
             pieces=validation['brick_count'],origin=args.url,automatic_retry=False)
journal=args.output/'attempt.json';journal.write_text(json.dumps(attempt,indent=2)+'\n')
client=Mcp(str(ROOT/'target/debug/ergo-mcp'),workspace,args.url,args.token_file)
try:
    attempt['publication_state']='submitted_once';journal.write_text(json.dumps(attempt,indent=2)+'\n')
    print(json.dumps(dict(submitted_once=True,pieces=validation['brick_count'])),flush=True)
    arguments=dict(base_revision=args.expected_base) if args.expected_base else {}
    result=client.call('publish_construction',**arguments)['structuredContent']
    (args.output/'receipt.json').write_text(json.dumps(result,indent=2)+'\n')
    if not result.get('published') or not result.get('accepted_revision') or not result.get('assembly_id'):
        raise RuntimeError('No confirmed accepted-assembly receipt; inspect the journal before any further write')
    attempt['publication_state']='confirmed';attempt['receipt']=result
    journal.write_text(json.dumps(attempt,indent=2)+'\n')
    print(json.dumps({key:result.get(key) for key in ['published','publication_state','world_id','assembly_id','accepted_revision','view_url']}),flush=True)
except Exception:
    attempt['publication_state']='inspect_mcp_sidecar_and_accepted_history_before_any_further_write'
    journal.write_text(json.dumps(attempt,indent=2)+'\n')
    raise
finally:client.close()
