#!/usr/bin/env python3
"""Record read-only model/technique references before authoring Paris landmarks."""
import argparse
import json
import pathlib
from mcp_client import Mcp

ROOT = pathlib.Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--output', type=pathlib.Path, required=True)
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=False)
client = Mcp(str(ROOT/'target/debug/ergo-mcp'), args.output/'research.native.json', None, None)
try:
    requests = [('church-models', 'get_lego_models', dict(query='church', source='ldraw', limit=3)),
                ('notre-dame-models', 'get_lego_models', dict(query='Notre Dame', source='ldraw', limit=3)),
                ('eiffel-models', 'get_lego_models', dict(query='Eiffel', source='ldraw', limit=3)),
                ('eiffel-reference', 'get_lego_models', dict(model_id='ldraw:81')),
                ('arched-techniques', 'get_building_techniques', dict(query='arches')),
                ('tower-techniques', 'get_building_techniques', dict(query='tower')),
                ('catalogue', 'get_catalog', dict(include_extended=True))]
    for name, tool, arguments in requests:
        value = client.rpc('tools/call', dict(name=tool, arguments=arguments))
        (args.output/(name+'.json')).write_text(json.dumps(value, indent=2)+'\n')
        print(json.dumps(dict(reference=name, error=value.get('isError',False))), flush=True)
finally:
    client.close()
