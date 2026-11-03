#!/usr/bin/env python3
"""Actual offline WASM/WebGL ground fields: variation, orientation and seams."""
import argparse
import base64
import functools
import http.server
import io
from pathlib import Path
import threading
from PIL import Image, ImageStat
from playwright.sync_api import sync_playwright

ROOT = Path(__file__).resolve().parents[1]


def block(east, north):
    def rgb(x, y):
        return 0x1e5aa8 if east + x / 64 < 1.25 else 0x00852b
    return dict(schema=2, cell_size=128, coord=[east, north, 0], bricks=[], color=71,
                surface=dict(format='ergo-ground-surface-1', rgb=[rgb(x, y) for y in range(65) for x in range(65)]),
                sample_rgb=None, source='Offline colour fixture', attribution='Synthetic fixture',
                attribution_url=None, generated_at=0, source_tiles=[], fallback_reason=None)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--artifacts', type=Path, default=ROOT / '.playwright-mcp/ground-colours')
    args = parser.parse_args()
    args.artifacts.mkdir(parents=True, exist_ok=True)

    class Handler(http.server.SimpleHTTPRequestHandler):
        def log_message(self, format, *args):
            pass

    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), functools.partial(Handler, directory=ROOT / 'web'))
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        with sync_playwright() as p:
            browser = p.chromium.launch(channel='chrome', headless=True, args=['--enable-unsafe-swiftshader'])
            try:
                page = browser.new_page(viewport={'width': 1024, 'height': 768})
                errors = []
                page.on('pageerror', lambda e: errors.append(str(e)))
                page.on('console', lambda m: errors.append(m.text) if m.type == 'error' else None)
                page.route('**/ground-test', lambda r: r.fulfill(body='<canvas id="ground"></canvas>', content_type='text/html'))
                page.goto(f'http://127.0.0.1:{server.server_port}/ground-test')
                scene = dict(schema=2, name='Ground display regression', geo=dict(latitude=0, longitude=0, arcseconds_per_block=1),
                             bricks=[], meshes={}, ground=[block(0, 0), block(1, 0)])
                result = page.evaluate('''async scene => {
                    const uploaded=[];
                    const original=WebGL2RenderingContext.prototype.texImage2D;
                    WebGL2RenderingContext.prototype.texImage2D=function(...args) {
                        if(args[3]===65 && args[4]===65 && args[8] instanceof Uint8Array) {
                            const rgba=args[8];for(let i=0;i<rgba.length;i+=4) uploaded.push((rgba[i]<<16)|(rgba[i+1]<<8)|rgba[i+2]);
                        }
                        return original.apply(this,args);
                    };
                    const module=await import('/mcp-pkg/ergo_client.js');await module.default();
                    const runtime=new module.LocalClientRuntime(document.querySelector('canvas'),JSON.stringify(scene),' {"entries":[]}',0);
                    const args={width:800,height:600,lighting:'studio',camera:{position_world:[128,-12.5,162.5],target_world:[128,64,0],vertical_fov_degrees:55}};
                    const result=JSON.parse(runtime.execute('test_visual_appearance',JSON.stringify(args)));
                    const gl=document.querySelector('canvas').getContext('webgl2');
                    return {result,error:gl.getError(),material_colours:[...new Set(uploaded)]};
                }''', scene)
                assert not result['result'].get('isError'), result
                assert result['error'] == 0, result['error']
                assert set(result['material_colours']) == {0x1e5aa8, 0x00852b}, result['material_colours']
                image = next(c['data'] for c in result['result']['content'] if c['type'] == 'image')
                raw = base64.b64decode(image)
                (args.artifacts / 'standard-two-plates.png').write_bytes(raw)
                image = Image.open(io.BytesIO(raw)).convert('RGB')
                # Both regions are on the same row and have identical lighting.
                left = ImageStat.Stat(image.crop((180, 280, 220, 320))).mean
                right = ImageStat.Stat(image.crop((580, 280, 620, 320))).mean
                assert left[2] > left[0] + 25, ('Blue side missing', left)
                assert right[1] > right[2] + 5, ('Green side missing', right)
                assert sum(abs(a-b) for a,b in zip(left,right)) > 35, ('Plate stayed uniform', left, right)
                # Adjacent fields agree at X=1024. A 2px strip across their seam
                # must not introduce a new material discontinuity.
                a = ImageStat.Stat(image.crop((397, 280, 399, 320))).mean
                b = ImageStat.Stat(image.crop((401, 280, 403, 320))).mean
                assert max(abs(x-y) for x,y in zip(a,b)) < 12, ('Cell seam', a, b)
                assert not errors, errors
            finally:
                browser.close()
    finally:
        server.shutdown()
        server.server_close()
        thread.join()
    print('Ground colour WebGL checks passed: standard materials only, discrete colour regions and matching cell edges.')


if __name__ == '__main__':
    main()
