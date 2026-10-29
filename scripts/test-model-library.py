#!/usr/bin/env python3
"""Exercise model discovery/retrieval through real stdio MCP; network is opt-in."""
import argparse,json,os,pathlib,tempfile
from mcp_client import Mcp

ROOT=pathlib.Path(__file__).resolve().parents[1]
parser=argparse.ArgumentParser();parser.add_argument('--binary',default=str(ROOT/'target/debug/ergo-mcp'));parser.add_argument('--network',action='store_true');args=parser.parse_args()
with tempfile.TemporaryDirectory(prefix='ergo-model-library-') as temporary:
    directory=pathlib.Path(temporary)
    document='0 FILE main.ldr\n0 Author: Test Builder\n0 !LICENSE CC BY 4.0\n1 4 0 0 0 1 0 0 0 1 0 0 0 1 3001.dat\n'
    (directory/'car.mpd').write_text(document)
    (directory/'index.json').write_text(json.dumps([{'id':'car','name':'Custom car','tags':['vehicle','town'],'path':'car.mpd','author':'Test Builder','license':'CC BY 4.0'}]))
    os.environ['ERGO_MODEL_LIBRARY_DIR']=str(directory);os.environ['ERGO_ASSET_FETCH']='0'
    client=Mcp(args.binary,directory/'scratch.ergo.json',None,None)
    try:
        schema=client.rpc('tools/list',{})['tools'];assert any(tool['name']=='get_lego_models' and tool['annotations']['readOnlyHint'] for tool in schema)
        custom=client.call('get_lego_models',source='custom',query='town car')['structuredContent']
        assert custom['models'][0]['model_id']=='custom:car'
        retrieved=client.call('get_lego_models',model_id='custom:car')['structuredContent']
        assert retrieved['author']=='Test Builder' and retrieved['document']==document
        if args.network:
            results=client.call('get_lego_models',source='ldraw',query='6505',limit=3)['structuredContent']
            assert results['models'],results
            matches=[item for item in results['models'] if item['set_number']=='6505-1']
            assert matches,results
            match=matches[0]
            car=client.call('get_lego_models',model_id=match['model_id'],submodel='6505 - car.ldr')['structuredContent']
            assert car['author']=='Robert Paciorek [bercik]'
            assert any(item['part']=='4211.dat' for item in car['part_inventory'])
            assert '0 FILE 6505 - car.ldr' in car['document']
            print(json.dumps({'official_model':car['model_id'],'author':car['author'],'parts':len(car['part_inventory']),'sha256':car['sha256']}))
        assert client.call('list_bricks')['structuredContent']['bricks']==[], 'Reference retrieval must not place geometry'
    finally:client.close()
print('Model-library stdio MCP checks passed: schema, custom lookup, attribution, exact documents and no scene mutation.')
