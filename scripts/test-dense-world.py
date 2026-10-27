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
                        page.goto(base + f"/app?shared_world={head['id']}&x=0&y=0&z=0")
                        expect(page.locator('.status-bar')).to_contain_text(re.compile(r'6,?000 bricks'), timeout=60000)
                        expect(page.locator('.status-bar')).to_contain_text(re.compile(r'[1-9]\d* draw calls'), timeout=60000)
                        expect(page.locator('.status-message')).to_contain_text('Shared-world region loaded', timeout=60000)
                        requests = []
                        page.on('request', lambda outgoing: requests.append(outgoing.url))
                        page.locator(f'[data-assembly-id="{names[0]}"]').click()
                        expect(page.locator('.status-message')).to_contain_text('Focused resident construction')
                        assert not any('/view?' in url or url.endswith('/location') for url in requests)
                        assert not errors, errors
                    finally:
                        browser.close()
            finally:
                server.terminate()
                server.wait(timeout=15)
                assert server.returncode == 0, 'Dense-view server did not shut down cleanly'
    print('Dense-region browser check passed: 6,000 rendered pieces, ordinary bounded imports, resident focus without geometry requests.')


if __name__ == '__main__':
    main()
