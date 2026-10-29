#!/usr/bin/env python3
"""Render a >5,000-piece region from ordinary bounded imports in an isolated store."""
import json
import os
from pathlib import Path
import re
import socket
import subprocess
import tempfile
import time
import urllib.request

from playwright.sync_api import expect, sync_playwright

ROOT = Path(__file__).resolve().parents[1]


def request(url, body=None):
    headers = {'X-Ergo-Request': '1', 'Content-Type': 'application/json'}
    data = None if body is None else json.dumps(body).encode()
    with urllib.request.urlopen(urllib.request.Request(url, data=data, headers=headers), timeout=300) as response:
        return json.load(response)


def main():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        port = sock.getsockname()[1]
    base = f'http://127.0.0.1:{port}'
    environment = {key: value for key, value in os.environ.items()
                   if not key.startswith(('ERGO_', 'GOOGLE_', 'S3_', 'AWS_'))}
    with tempfile.TemporaryDirectory(prefix='ergo-dense-web-') as temporary:
        directory = Path(temporary)
        (directory / 'web').symlink_to(ROOT / 'web', target_is_directory=True)
        environment.update(ERGO_BIND=f'127.0.0.1:{port}', ERGO_PUBLIC_URL=base,
                           ERGO_DATA_DIR=str(directory / 'store'), ERGO_ISOLATED_FIXTURES='1',
                           ERGO_ASSET_FETCH='0', ERGO_GROUND_SOURCE='disabled',
                           # Pacing is independent of the unchanged per-import
                           # 5,000-piece authoring budget tested here.
                           ERGO_EDIT_BRICKS_PER_PERIOD='10000')
        with tempfile.TemporaryFile(mode='w+t') as log:
            def start():
                process = subprocess.Popen([str(ROOT / 'target/debug/ergo-server')], cwd=directory,
                                           env=environment, stdout=log, stderr=log)
                for _ in range(100):
                    if process.poll() is not None:
                        log.seek(0)
                        raise AssertionError('Isolated dense-view server stopped:\n' + log.read())
                    try:
                        request(base + '/api/config')
                        return process
                    except OSError:
                        time.sleep(.1)
                raise AssertionError('Isolated dense-view server did not start')
            server = start()
            try:
                head = request(base + '/api/shared-worlds', {'name': 'Dense region regression',
                               'authenticated_editing': True,
                               'geo': {'latitude': 48.8566, 'longitude': 2.3522, 'arcseconds_per_block': 1}})
                mesh = request(base + '/api/parts/3068b.dat')['mesh']
                names = []
                # Each import stays below the unchanged 5,000-piece authoring
                # budget. Their combined region must render all 6,000 pieces.
                for batch in range(2):
                    bricks = []
                    for index in range(batch * 3000, (batch + 1) * 3000):
                        position = [(index % 100) * 3 + 1, (index // 100) * 3 + 1, 1 / 3]
                        translated = lambda bounds: {edge: [value + offset for value, offset in zip(bounds[edge], position)] for edge in ['min', 'max']}
                        bricks.append({'id': index % 3000 + 1, 'part': '3068b.dat', 'color': 19,
                                       'rgb': 0xE4CD9E, 'step': 0, 'position': position,
                                       'transform': [1, 0, 0, 0, 1, 0, 0, 0, 1],
                                       'bounds': translated(mesh['body_bounds']), 'render_bounds': translated(mesh['bounds'])})
                    scene = {'schema': 2, 'name': f'Dense floor {batch + 1}', 'geo': head['metadata']['geo'],
                             'bricks': bricks, 'meshes': {'3068b.dat': mesh}, 'joints': [], 'fixed_bricks': [],
                             'groups': [], 'ground': [], 'model': None}
                    accepted = request(base + f"/api/shared-worlds/{head['id']}/assemblies",
                                       {'base': head['revision'], 'scene': scene})
                    head['revision'] = accepted['accepted']['revision']
                    names.append(accepted['assembly'])
                remote_scene=json.loads(json.dumps(scene))
                remote_scene['name']='Distant loading cancellation fixture'
                remote_scene['bricks']=remote_scene['bricks'][:1]
                brick=remote_scene['bricks'][0]
                delta=4096-brick['bounds']['min'][0]
                brick['position'][0]+=delta
                for field in ['bounds','render_bounds']:
                    for edge in ['min','max']:brick[field][edge][0]+=delta
                remote=request(base+f"/api/shared-worlds/{head['id']}/assemblies",{'base':head['revision'],'scene':remote_scene})
                head['revision']=remote['accepted']['revision']
                # Reopen the same disposable accepted namespace as the one
                # common world, so the browser follows the production list path.
                server.terminate()
                server.wait(timeout=15)
                assert server.returncode == 0
                environment['ERGO_COMMON_WORLD_ID'] = head['id']
                server = start()
                with sync_playwright() as playwright:
                    browser = playwright.chromium.launch(channel='chrome', headless=True,
                                                          args=['--enable-unsafe-swiftshader'])
                    try:
                        page = browser.new_page(viewport={'width': 1440, 'height': 950})
                        errors = []
                        page.on('pageerror', lambda error: errors.append(str(error)))
                        held=[]
                        progress={'stage':'discovering','completed':12,'total':27,'found_pieces':1234}
                        hold_first=[True]
                        status_revision=[head['revision']]
                        fail_status=[False]
                        deny_read=[False]
                        def loading_route(route):
                            if 'status=true' in route.request.url:
                                if fail_status[0]:route.abort()
                                else:route.fulfill(json={'revision':status_revision[0],'phase':'working','progress':dict(progress)})
                            elif deny_read[0]:
                                route.fulfill(status=403,json={'error':'Read access revoked by test'})
                            elif hold_first[0]:
                                held.append(route)
                            else:
                                route.continue_()
                        page.route('**/api/shared-worlds/*/view?*',loading_route)
                        page.goto(base + f"/app?shared_world={head['id']}&x=0&y=0&z=0",wait_until='domcontentloaded')
                        expect(page.locator('.loading-stage')).to_have_text('Finding nearby bricks',timeout=30000)
                        expect(page.locator('.loading-detail')).to_contain_text('1,234 bricks found')
                        assert page.get_by_role('progressbar').get_attribute('value') is None
                        progress.update(stage='pieces',completed=2000,total=6000,found_pieces=None)
                        expect(page.locator('.loading-percent')).to_have_text('33%',timeout=15000)
                        expect(page.locator('.loading-detail')).to_have_text('2,000 of 6,000 bricks')
                        page.screenshot(path=str(ROOT/'target/loading-progress.png'))
                        status_revision[0]='unrelated-revision'
                        progress.update(completed=1000)
                        page.wait_for_timeout(2500)
                        expect(page.locator('.loading-percent')).to_have_text('33%')
                        status_revision[0]=head['revision']
                        progress.update(completed=5000)
                        expect(page.locator('.loading-percent')).to_have_text('83%',timeout=15000)
                        assert len(held)==1
                        assert 'revision='+head['revision'] in held[0].request.url
                        fail_status[0]=True
                        page.wait_for_timeout(2500)
                        expect(page.locator('.loading-percent')).to_have_text('83%')
                        hold_first[0]=False
                        held[0].continue_()
                        expect(page.locator('.status-bar')).to_contain_text(re.compile(r'6,?000 bricks'), timeout=60000)
                        expect(page.locator('.status-bar')).to_contain_text(re.compile(r'[1-9]\d* draw calls'), timeout=60000)
                        expect(page.locator('.status-message')).to_contain_text('Shared-world region loaded', timeout=60000)
                        expect(page.locator('.loading-panel')).to_be_hidden(timeout=15000)
                        canvas=page.locator('#world-canvas')
                        quality=page.get_by_role('combobox',name='Render detail',exact=True)
                        expect(quality).to_have_value('auto')
                        quality.select_option('high')
                        expect(canvas).to_have_attribute('data-detail-scale','1')
                        page.wait_for_function("JSON.parse(document.querySelector('#world-canvas').dataset.shadowLodCounts).slice(1).every(count=>count===0)")
                        quality.select_option('performance')
                        page.wait_for_function("JSON.parse(document.querySelector('#world-canvas').dataset.shadowLodCounts).slice(1).some(count=>count>0)")
                        quality.select_option('auto')
                        expect(page.locator('.status-bar')).to_contain_text(re.compile(r'6,?000 bricks'))
                        requests = []
                        page.on('request', lambda outgoing: requests.append(outgoing.url))
                        page.locator(f'[data-assembly-id="{names[0]}"]').click()
                        expect(page.locator('.status-message')).to_contain_text('Focused resident construction')
                        assert not any('/view?' in url or url.endswith('/location') for url in requests)
                        # Local construction/index bytes survive reload, but a
                        # fresh server permission/version check still runs.
                        responses=[]
                        page.on('response',lambda response: responses.append((response.url,response.status)))
                        page.reload(wait_until='domcontentloaded')
                        expect(page.locator('.status-message')).to_contain_text('Shared-world region loaded',timeout=60000)
                        assert any('/view?' in url and status==304 for url,status in responses),responses
                        # A slow remote read must retain this scene and remain
                        # cancellable by another construction selection.
                        remote_reads=[]
                        def hold_remote(route):
                            if 'status=true' in route.request.url:route.fulfill(json={'revision':head['revision'],'phase':'working'})
                            else:remote_reads.append(route)
                        page.route('**/view?x=4&*',hold_remote)
                        page.locator(f'[data-assembly-id="{remote["assembly"]}"]').click()
                        expect(page.locator('.loading-panel')).to_be_visible(timeout=15000)
                        page.wait_for_timeout(300)
                        expect(page.locator('.status-bar')).to_contain_text(re.compile(r'6,?000 bricks'))
                        expect(page.locator(f'[data-assembly-id="{names[0]}"]')).to_be_enabled()
                        page.locator(f'[data-assembly-id="{names[0]}"]').click()
                        expect(page.locator('.status-message')).to_contain_text('Focused resident construction')
                        expect(page.locator('.loading-panel')).to_be_hidden()
                        for route in remote_reads:
                            try:route.abort()
                            except Exception:pass
                        page.unroute('**/view?x=4&*',hold_remote)
                        page.evaluate('''() => {const original=window.setTimeout.bind(window);window.__shortReadTimeout=true;window.setTimeout=(callback,delay,...args)=>original(callback,window.__shortReadTimeout&&delay===15000?80:delay,...args);}''')
                        stalled_locations=[]
                        def hold_location(route):stalled_locations.append(route)
                        pattern=f'**/assemblies/{remote["assembly"]}/location'
                        page.route(pattern,hold_location)
                        page.locator(f'[data-assembly-id="{remote["assembly"]}"]').click()
                        expect(page.get_by_role('alert')).to_contain_text('timed out',timeout=5000)
                        expect(page.locator('.loading-panel')).to_be_hidden()
                        page.evaluate('window.__shortReadTimeout=false')
                        page.locator(f'[data-assembly-id="{names[0]}"]').click()
                        expect(page.locator('.status-message')).to_contain_text('Focused resident construction')
                        for route in stalled_locations:
                            try:route.abort()
                            except Exception:pass
                        page.unroute(pattern,hold_location)
                        # A corrupted local packet must become a cache miss.
                        assert page.evaluate('''async () => {
                          const req=request=>new Promise((resolve,reject)=>{request.onsuccess=()=>resolve(request.result);request.onerror=()=>reject(request.error);});
                          const db=await req(indexedDB.open('ergo-local-data-v2',1));
                          const read=db.transaction(['entries','blobs'],'readonly');
                          const entries=await req(read.objectStore('entries').getAll());
                          const entry=entries.find(entry=>{const url=new URL(entry.url);return url.pathname.endsWith('/view')&&url.searchParams.get('x')==='0'&&url.searchParams.get('y')==='0';});
                          if(!entry){db.close();return false;}
                          const blob=await req(read.objectStore('blobs').get(entry.hash));
                          const bytes=new Uint8Array(await blob.data.arrayBuffer());bytes[0]^=1;
                          const write=db.transaction('blobs','readwrite');
                          const done=new Promise((resolve,reject)=>{write.oncomplete=resolve;write.onerror=()=>reject(write.error);});
                          write.objectStore('blobs').put({hash:entry.hash,data:new Blob([bytes])});await done;db.close();return true;
                        }''')
                        page.reload(wait_until='domcontentloaded')
                        expect(page.locator('.status-message')).to_contain_text('Shared-world region loaded',timeout=60000)
                        deny_read[0]=True
                        page.reload(wait_until='domcontentloaded')
                        expect(page.get_by_role('alert')).to_contain_text('Read access revoked by test',timeout=30000)
                        assert '6,000 bricks' not in page.locator('.status-bar').inner_text()
                        assert not errors, errors
                    finally:
                        browser.close()
            finally:
                server.terminate()
                server.wait(timeout=15)
                assert server.returncode == 0, 'Dense-view server did not shut down cleanly'
    print('Dense-region browser check passed: honest discovery and brick percentages, 6,000 rendered pieces, loading cleanup and resident focus without geometry requests.')


if __name__ == '__main__':
    main()
