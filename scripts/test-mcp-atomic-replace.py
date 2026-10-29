#!/usr/bin/env python3
"""Exercise real WASM atomic replacement, identity preservation and rollback."""
import os,pathlib,tempfile
from mcp_client import Mcp

ROOT=pathlib.Path(__file__).resolve().parents[1]
os.environ['ERGO_ASSET_FETCH']='0'
with tempfile.TemporaryDirectory(prefix='ergo-atomic-replace-') as temporary:
    client=Mcp(str(ROOT/'target/debug/ergo-mcp'),pathlib.Path(temporary)/'construction.ergo.json',None,None)
    try:
        client.call('place_brick',part='3003.dat',color=4,min_corner=[0,0,0])
        client.call('place_brick',part='3003.dat',color=1,min_corner=[4,0,0])
        before=client.call('list_bricks')['structuredContent']['bricks']
        model={'format':'ergo-model-1','schema':2,'root':'main','definitions':{'main':[{'target':{'kind':'part','id':'3003.dat'},'color':14,'transform':[[1,0,0,1],[0,1,0,1],[0,0,1,1],[0,0,0,1]]}]}}
        result=client.call('place_submodel',replace_ids=[1],model=model)['structuredContent']
        assert result['replacement']['atomic'] and result['replacement']['removed_ids']==[1]
        after=client.call('list_bricks')['structuredContent']['bricks']
        assert {brick['id'] for brick in after}=={2,3}
        assert next(brick for brick in after if brick['id']==2)==before[1]
        model['definitions']['main'][0]['transform'][0][3]=5
        rejected=client.rpc('tools/call',{'name':'place_submodel','arguments':{'replace_ids':[3],'model':model}})
        assert rejected['isError']
        assert client.call('list_bricks')['structuredContent']['bricks']==after
    finally:client.close()
print('Atomic replacement MCP checks passed: final-state overlaps, unchanged identities, fresh IDs and complete rollback.')
