#!/usr/bin/env python3
"""Read-only actual-WASM validation/rendering of an existing current landmark draft."""
import argparse
import base64
import json
import os
import pathlib
from mcp_client import Mcp

ROOT=pathlib.Path(__file__).resolve().parents[1]
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('construction',type=pathlib.Path)
parser.add_argument('--binary',type=pathlib.Path,default=ROOT/'target/debug/ergo-mcp')
parser.add_argument('--web-dir',type=pathlib.Path)
parser.add_argument('--output',type=pathlib.Path)
parser.add_argument('--camera',help='JSON with position_world/target_world for an optional actual WebGL capture')
args=parser.parse_args()
if args.output and (args.output.exists() or args.camera and args.output.with_suffix('.png').exists()):
    raise SystemExit('Choose fresh verification outputs')
if args.web_dir:os.environ['ERGO_WEB_DIR']=str(args.web_dir.resolve())
client=Mcp(str(args.binary.resolve()),args.construction.resolve(),None,None)
try:
    report=client.call('validate_construction')['structuredContent']
    print(json.dumps(report),flush=True)
    if args.output:args.output.write_text(json.dumps(report,indent=2)+'\n')
    if args.camera:
        result=client.call('test_visual_appearance',width=1024,height=768,lighting='studio',camera=json.loads(args.camera))
        assert args.output,'A fresh output path is needed for the image'
        args.output.with_suffix('.png').write_bytes(base64.b64decode(next(item['data'] for item in result['content'] if item['type']=='image')))
finally:client.close()
