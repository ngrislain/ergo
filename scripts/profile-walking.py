#!/usr/bin/env python3
"""Read-only walking/render profile using a saved, accepted region CBOR packet."""
import argparse
import json
import pathlib
import statistics
from playwright.sync_api import expect, sync_playwright

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--url', required=True)
parser.add_argument('--packet', type=pathlib.Path, required=True)
parser.add_argument('--revision', default='461bc066e10257df74cc9720b7491f9ebb557a972ee7764de1e5668c427491d0')
parser.add_argument('--seconds', type=float, default=5)
parser.add_argument('--dpr', type=float, default=1)
parser.add_argument('--artifacts', default='.playwright-mcp')
parser.add_argument('--output', type=pathlib.Path)
args = parser.parse_args()
packet = args.packet.read_bytes()
world = '11111111-1111-4111-8111-111111111111'
geo = {'latitude': 48.8566, 'longitude': 2.3522, 'arcseconds_per_block': 1}
results = []
with sync_playwright() as playwright:
    browser = playwright.chromium.launch(channel='chrome', headless=True, args=['--enable-unsafe-swiftshader'])
    try:
        for hour in [12, 0]:
            page = browser.new_page(viewport={'width': 1440, 'height': 950}, device_scale_factor=args.dpr)
            errors = []
            page.on('pageerror', lambda error: errors.append(str(error)))
            page.add_init_script(f'''const started=performance.now();Date.now=()=>Date.UTC(2026,5,21,{hour})+(performance.now()-started)%59000;''')
            config = page.request.get(args.url + '/api/config').json()
            config.update(common_world=world, geo=geo)
            page.route('**/api/config', lambda route: route.fulfill(json=config))
            def region(route):
                url = route.request.url
                if '/view?' in url and 'status=true' not in url:
                    route.fulfill(body=packet, content_type='application/cbor')
                elif '/assemblies?' in url:
                    route.fulfill(json={'items': [], 'next': None})
                elif 'status=true' in url:
                    route.fulfill(json={'revision': args.revision, 'phase': None})
                else:
                    route.fulfill(json={'id': world, 'revision': args.revision,
                                        'metadata': {'name': 'Accepted castle render profile', 'geo': geo}})
            page.route('**/api/shared-worlds/' + world + '**', region)
            page.goto(args.url + '/app?x=1&y=1&z=0', wait_until='domcontentloaded')
            expect(page.locator('.status-message')).to_contain_text('Shared-world region loaded', timeout=90000)
            canvas = page.locator('#world-canvas')
            for quality, fog in [('auto', 2200), ('performance', 2200), ('auto', 800)]:
                page.get_by_label('Render detail', exact=True).select_option(quality)
                page.get_by_label('Viewing distance (fog)').evaluate('(input,value)=>{input.value=value;input.dispatchEvent(new Event("input",{bubbles:true}));}', str(fog))
                page.get_by_role('button', name='Fly', exact=True).click()
                page.get_by_role('button', name='Go to a location', exact=False).click()
                dialog = page.locator('#location-dialog')
                dialog.get_by_label('Latitude', exact=True).fill(str(geo['latitude'] + 82 / (128 * 3600)))
                dialog.get_by_label('Longitude', exact=True).fill(str(geo['longitude'] + 240 / (128 * 3600)))
                dialog.get_by_role('button', name='Go to coordinates', exact=True).click()
                page.get_by_role('button', name='Walk', exact=True).click()
                expect(canvas).to_have_attribute('data-navigation', 'walk', timeout=30000)
                page.wait_for_timeout(300)
                yaw = json.loads(canvas.get_attribute('data-camera-rotation'))[0]
                canvas.dispatch_event('pointerdown', {'pointerId': 1, 'button': 0, 'clientX': 100, 'clientY': 100})
                canvas.dispatch_event('pointermove', {'pointerId': 1, 'button': 0, 'clientX': 100 + round(yaw / .007), 'clientY': 100})
                canvas.dispatch_event('pointerup', {'pointerId': 1, 'button': 0, 'clientX': 100 + round(yaw / .007), 'clientY': 100})
                canvas.focus()
                page.keyboard.down('ArrowUp')
                page.wait_for_timeout(1000)
                page.evaluate('''() => {window.__walkSamples=[];window.__walkStart={time:performance.now(),frames:Number(document.querySelector('#world-canvas').dataset.renderedFrames)};window.__walkTimer=setInterval(()=>window.__walkSamples.push({...document.querySelector('#world-canvas').dataset}),100);}''')
                page.wait_for_timeout(args.seconds * 1000)
                page.keyboard.up('ArrowUp')
                sample = page.evaluate('''() => {clearInterval(window.__walkTimer);const canvas=document.querySelector('#world-canvas');const gl=canvas.getContext('webgl2'),debug=gl.getExtension('WEBGL_debug_renderer_info');return {seconds:(performance.now()-window.__walkStart.time)/1000,frames:Number(canvas.dataset.renderedFrames)-window.__walkStart.frames,samples:window.__walkSamples,renderer:debug?gl.getParameter(debug.UNMASKED_RENDERER_WEBGL):gl.getParameter(gl.RENDERER)};}''')
                row = {'lighting': 'sunlight' if hour else 'flashlight', 'quality': quality, 'fog': fog,
                       'dpr': args.dpr, 'renderer': sample['renderer'],
                       'rendered_fps': round(sample['frames'] / sample['seconds'], 2),
                       'resident_scene': page.locator('.status-bar').inner_text(),
                       'camera': canvas.get_attribute('data-camera-position')}
                for key in ['scenePlanMs', 'groundPlanMs', 'uploadMs', 'colorTriangles', 'shadowDrawCalls', 'instanceUploadBytes', 'groundStuds', 'occludedPieces', 'detailScale']:
                    values = [float(value[key]) for value in sample['samples'] if key in value]
                    if values:
                        row[key + '_median'] = round(statistics.median(values), 3)
                        row[key + '_max'] = max(values)
                results.append(row)
                print(json.dumps(row), flush=True)
            assert not errors, errors
            page.close()
    finally:
        browser.close()
if args.output:
    args.output.write_text(json.dumps({'read_only_saved_accepted_packet': str(args.packet), 'results': results}, indent=2) + '\n')
