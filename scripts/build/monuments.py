#!/usr/bin/env python3
"""Build walkable original monuments and preflight through the real local MCP."""
import argparse
import importlib.util
import json
from pathlib import Path
import shutil
import sys
import subprocess

ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT))
sys.path.insert(0,str(ROOT/'scripts'))
import monument_models as models

spec=importlib.util.spec_from_file_location('world_recipes',ROOT/'scripts/build-world-models.py')
assert spec and spec.loader
builder=importlib.util.module_from_spec(spec);spec.loader.exec_module(builder)

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('kind',choices=['pyramid','tower'])
    parser.add_argument('--output',required=True,type=Path)
    parser.add_argument('--origin',required=True,nargs=3,type=float)
    parser.add_argument('--binary',default='ergo-mcp')
    diagnostics=parser.add_mutually_exclusive_group()
    diagnostics.add_argument('--estimated-connectors',action='store_true',help='Run the native developer diagnostic (requires Rust), using explicit ballpark estimates')
    diagnostics.add_argument('--wasm-estimated-connectors',action='store_true',help='Run the bounded WASM/MCP diagnostic instead')
    args=parser.parse_args();binary=shutil.which(args.binary)
    if not binary:parser.error('An installed Ergo MCP binary is required')
    model=getattr(models,args.kind)
    name='Pyramid of the Three Chambers' if args.kind=='pyramid' else 'Ten-Level Seven Sisters Tower'
    report=builder.run_model(binary,args.output,name,model,args.origin,estimated_connectors=args.wasm_estimated_connectors,technique_query='lintel' if args.kind=='tower' else '',required_parts=models.TOWER_PARTS if args.kind=='tower' else ())
    report['design']={'kind':args.kind,'techniques_applied':models.TECHNIQUES,'minifigure_scale':True,
        'clear_room_height_native':9,'inspiration':'https://en.wikipedia.org/wiki/Great_Pyramid_of_Giza' if args.kind=='pyramid' else 'https://en.wikipedia.org/wiki/Main_building_of_Moscow_State_University',
        'original_design_not_replica':True,'levels':1 if args.kind=='pyramid' else 10}
    (args.output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    route=getattr(models,args.kind+'_route')(args.origin)
    (args.output/'walk-route.json').write_text(json.dumps(route,indent=2)+'\n')
    if args.estimated_connectors:
        output=(args.output/'native-stability.json').resolve()
        subprocess.run(['cargo','run','--release','--locked','-p','ergo-world','--features','simulation','--example','check_stability','--',str((args.output/'construction.ergo.json').resolve()),str(output),'--estimated-connectors'],cwd=ROOT,check=True)
        report['stability']=json.loads(output.read_text())
        (args.output/'report.json').write_text(json.dumps(report,indent=2)+'\n')

if __name__=='__main__':main()
