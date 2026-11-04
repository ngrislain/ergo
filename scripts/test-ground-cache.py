#!/usr/bin/env python3
"""Current floor fields persist locally and identical patterns share GPU textures."""
import argparse
import json
import re
import time
from playwright.sync_api import expect, sync_playwright

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--url', required=True)
parser.add_argument('--artifacts')
args = parser.parse_args()
with sync_playwright() as playwright:
    browser = playwright.chromium.launch(channel='chrome', headless=True)
    try:
        page = browser.new_page(viewport={'width': 1200, 'height': 900}, device_scale_factor=2)
        errors = []
        page.on('pageerror', lambda error: errors.append(str(error)))
        config = page.request.get(args.url + '/api/config').json()
        config.update(ground_provider='Floor cache fixture', ground_revision='a' * 64)
        palette = {color['code']: color['rgb'] for color in page.request.get(args.url + '/api/catalog').json()['colors']}
        colour = [2]
        page.route('**/api/config', lambda route: route.fulfill(json=config))
        requests = []
        def ground(route):
            requests.append(route.request.url)
            match = re.search(r'/api/ground/(-?\d+)/(-?\d+)', route.request.url)
            x, north = map(int, match.groups())
            block = dict(schema=2, cell_size=128, coord=[x, north, 0], bricks=[], color=colour[0],
                         surface={'format': 'ergo-ground-surface-1', 'rgb': [palette[colour[0]]] * (65 * 65)},
                         sample_rgb=palette[colour[0]], source='Floor cache fixture', attribution='Synthetic test fixture',
                         attribution_url=None, generated_at=int(time.time()), source_tiles=[], fallback_reason=None)
            route.fulfill(json=block, headers={'ETag': '"floor-' + str(colour[0]) + '"'})
        page.route('**/api/ground/**', ground)
        page.goto(args.url + '/app', wait_until='domcontentloaded')
        expect(page.locator('.status-message')).to_contain_text('Construction loaded', timeout=30000)
        fog = page.get_by_label('Viewing distance (fog)')
        fog.evaluate('(input)=>{input.value=400;input.dispatchEvent(new Event("input",{bubbles:true}));}')
        canvas = page.locator('#world-canvas')
        expect(canvas).to_have_attribute('data-ground-patterns', '1', timeout=30000)
        page.wait_for_timeout(2000)
        assert len(requests) > 4, requests
        uploads = canvas.get_attribute('data-ground-pattern-uploads')
        assert uploads == '1', f'Identical fields were uploaded more than once: {uploads}'
        # Performance reduces raster work on a high-DPI screen, preserving the
        # CSS aspect ratio/projection and independently sized shadow buffer.
        before = canvas.evaluate('(canvas)=>({width:canvas.width,height:canvas.height,shadow:canvas.dataset.shadowMap})')
        page.get_by_label('Render detail', exact=True).select_option('performance')
        page.wait_for_function('width=>document.querySelector("#world-canvas").width<width', arg=before['width'])
        after = canvas.evaluate('(canvas)=>({width:canvas.width,height:canvas.height,shadow:canvas.dataset.shadowMap})')
        assert before['width'] == after['width'] * 2 and before['height'] == after['height'] * 2
        assert before['shadow'] == after['shadow']
        bindings = page.request.get(args.url + '/pkg/ergo_viewer.js').text()
        found = re.search(r"from ['\"](\./snippets/[^'\"]+browser_cache\.js)['\"]", bindings)
        assert found
        module = args.url + '/pkg/' + found.group(1).removeprefix('./')
        cached = page.evaluate('''async module=>{
          const cache=await import(module);const request=r=>new Promise((resolve,reject)=>{r.onsuccess=()=>resolve(r.result);r.onerror=()=>reject(r.error);});
          const db=await request(indexedDB.open('ergo-local-data-cell128-v1',1));const entries=await request(db.transaction('entries').objectStore('entries').getAll());db.close();
          const urls=[];for(const entry of entries)if(entry.url.includes('/api/ground/')){const value=await cache.cacheGet(entry.url);if(value)urls.push(entry.url);}
          return urls;
        }''', module)
        assert cached, 'No persistent floor fields'
        requests.clear()
        page.reload(wait_until='domcontentloaded')
        expect(page.locator('.status-message')).to_contain_text('Construction loaded', timeout=30000)
        page.wait_for_timeout(2000)
        assert not set(cached).intersection(requests), 'Fresh public floor patterns were downloaded again'
        # A new source/palette/algorithm identity must not use old floor data.
        config['ground_revision'] = 'b' * 64
        colour[0] = 1
        requests.clear()
        page.reload(wait_until='domcontentloaded')
        expect(page.locator('.status-message')).to_contain_text('Construction loaded', timeout=30000)
        expect(canvas).to_have_attribute('data-ground-patterns', '1', timeout=30000)
        assert requests and all('cache_revision=' + 'b' * 64 in url for url in requests)
        assert not errors, errors
        print('Floor cache passed: local reuse after reload, source-version invalidation, one shared GPU pattern, DPR-aware performance without shadow-resolution changes.')
    finally:
        browser.close()
