#!/usr/bin/env python3
"""Actual WASM/WebGL captures: lit transparency and cross-prototype depth order."""
import argparse
import base64
import io
import os
from pathlib import Path
import tempfile
from PIL import Image, ImageStat, ImageChops
from mcp_client import Mcp

ROOT=Path(__file__).resolve().parents[1]
parser=argparse.ArgumentParser()
parser.add_argument('--binary',default=str(ROOT/'target/debug/ergo-mcp'))
parser.add_argument('--artifacts',type=Path,default=ROOT/'.playwright-mcp/materials')
args=parser.parse_args();args.artifacts.mkdir(parents=True,exist_ok=True)
os.environ['ERGO_ASSET_FETCH']='0'
camera={'position_world':[2,-12,2],'target_world':[2,2,.5],'vertical_fov_degrees':35}
def capture(client,name,lighting='studio'):
    result=client.call('test_visual_appearance',camera=camera,width=640,height=480,lighting=lighting)
    content=next(item for item in result['content'] if item['type']=='image')
    raw=base64.b64decode(content['data']);(args.artifacts/(name+'.png')).write_bytes(raw)
    return Image.open(io.BytesIO(raw)).convert('RGB')
def centre(image):return ImageStat.Stat(image.crop((304,224,336,256))).mean
with tempfile.TemporaryDirectory(prefix='ergo-materials-') as temporary:
    client=Mcp(args.binary,Path(temporary)/'clear.json',None,None)
    try:
        client.call('place_brick',part='3001',color=4,min_corner=[0,3,0])
        background=capture(client,'background')
        client.call('place_brick',part='3001',color=15,min_corner=[0,0,0])
        opaque=capture(client,'opaque')
        client.call('remove_bricks',ids=[2])
        client.call('place_brick',part='3001',color=47,min_corner=[0,0,0])
        clear=capture(client,'clear')
        a,b,c=centre(background),centre(opaque),centre(clear)
        assert c[0]-c[1]>b[0]-b[1]+12,('The red background is not visible through clear plastic',a,b,c)
        assert sum(abs(x-y) for x,y in zip(c,a))>3,('Clear plastic lost its surface lighting',a,c)
        world=capture(client,'clear-world',lighting='world')
        assert ImageStat.Stat(ImageChops.difference(clear,world)).mean[0]>.1,'Transparent pieces must respond to lighting'
    finally:client.close()
    pictures=[]
    for index,reverse in enumerate([False,True]):
        client=Mcp(args.binary,Path(temporary)/f'overlap-{index}.json',None,None)
        try:
            client.call('place_brick',part='3001',color=15,min_corner=[0,6,0])
            layers=[('3001',36,[0,3,0]),('3003',43,[1,0,0])]
            for part,color,position in reversed(layers) if reverse else layers:
                client.call('place_brick',part=part,color=color,min_corner=position)
            pictures.append(capture(client,f'overlap-{index}'))
        finally:client.close()
    error=ImageStat.Stat(ImageChops.difference(*pictures)).mean
    assert max(error)<.05,('Transparency depends on insertion order rather than camera depth',error)
print('Material render checks passed: visible background, lit clear surfaces, and stable cross-prototype overlap order.')
