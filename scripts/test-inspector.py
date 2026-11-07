#!/usr/bin/env python3
"""Test the unlisted Inspector page, optionally embedding the actual official app."""
import argparse
import json
import os
import pathlib
import socket
import subprocess
import tempfile
import time
import urllib.request
from playwright.sync_api import sync_playwright, expect
import library_fixture

ROOT = pathlib.Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--official', action='store_true', help='Run pinned npm Inspector; opt-in network/dependency installation')
parser.add_argument('--catalogue', action='store_true', help='Also run the existing real 3D catalogue suite')
args = parser.parse_args()

def port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]

def ready(url, process, attempts=150):
    for _ in range(attempts):
        assert process.poll() is None, 'Server exited before it was ready'
        try:
            urllib.request.urlopen(url, timeout=1).close()
            return
        except OSError: time.sleep(.1)
    raise AssertionError('Server startup timed out')

with tempfile.TemporaryDirectory(prefix='ergo-inspector-') as temporary:
    directory = pathlib.Path(temporary)
    (directory/'web').symlink_to(ROOT/'web', target_is_directory=True)
    env = {key:value for key,value in os.environ.items() if not key.startswith(('ERGO_','GOOGLE_','S3_','AWS_','MCP_','INSPECTOR_'))}
    env.update(ERGO_GROUND_SOURCE='disabled', ERGO_ISOLATED_FIXTURES='1')
    library_fixture.seed(directory/'data', environment=env)
    address = f'127.0.0.1:{port()}'
    url = 'http://'+address
    with tempfile.TemporaryFile() as log:
        server = subprocess.Popen([str(ROOT/'target/debug/ergo-server')],cwd=directory,
                                  env={**env,'ERGO_BIND':address,'ERGO_PUBLIC_URL':url}, stdout=log,stderr=log)
        inspector = None
        try:
            ready(url+'/api/config',server)
            inspector_url = f'http://127.0.0.1:{port()}/'
            if args.official:
                configuration = {'mcpServers':{'ergo':{'command':str(ROOT/'target/debug/ergo-mcp'),
                    'args':['--local','--attach','--workspace',str(directory/'inspector.cache.json'),'--web-dir',str(ROOT/'web')]}}}
                config = directory/'ergo-inspector.json'
                config.write_text(json.dumps(configuration))
                inspector_env = {**env,'CLIENT_PORT':inspector_url.split(':')[-1].strip('/'),
                    'MCP_SANDBOX_PORT':str(port()),'MCP_APP_ORIGIN_PORT':str(port()),
                    'MCP_AUTO_OPEN_ENABLED':'false','MCP_STORAGE_DIR':str(directory/'inspector-state'),
                    'MCP_INSPECTOR_SECRET_STORE':'memory'}
                inspector = subprocess.Popen(['npx','--yes','@modelcontextprotocol/inspector@2.14.0','--web','--config',str(config)],
                    cwd=directory,env=inspector_env,stdout=log,stderr=log,start_new_session=True)
                ready(inspector_url+'healthz',inspector,1200)
                headers = urllib.request.urlopen(inspector_url).headers
                print('Official Inspector framing headers:', {name:headers.get(name) for name in ['X-Frame-Options','Content-Security-Policy']},flush=True)
            with sync_playwright() as p:
                browser = p.chromium.launch(channel='chrome',headless=True,args=['--enable-unsafe-swiftshader'])
                try:
                    page = browser.new_page(viewport={'width':1440,'height':1000})
                    response = page.goto(url+'/mcp-inspector')
                    assert response.status == 200
                    page.wait_for_load_state('networkidle')
                    expect(page.get_by_role('heading',name='Look inside the MCP connection.')).to_be_visible()
                    assert 'noindex' in page.locator('meta[name=robots]').get_attribute('content')
                    assert not page.locator('iframe').get_attribute('src'), 'No automatic localhost requests'
                    with page.expect_download() as download:
                        page.get_by_role('button',name='Download Ergo configuration',exact=True).click()
                    config = json.loads(pathlib.Path(download.value.path()).read_text())['mcpServers']['ergo']
                    assert config['env'] == {'ERGO_PUBLISH_URL':url}
                    assert config['args'] == ['--headless','--workspace','inspector.cache.json']
                    for unsafe in ['https://attacker.example:6274/','http://127.0.0.1:6274/api/delete']:
                        page.get_by_label('Local Inspector URL',exact=True).fill(unsafe)
                        page.get_by_role('button',name='Display Inspector',exact=True).click()
                        expect(page.get_by_role('alert')).to_be_visible()
                        assert not page.locator('iframe').get_attribute('src')
                    page.get_by_label('Local Inspector URL',exact=True).fill(inspector_url)
                    page.get_by_role('button',name='Display Inspector',exact=True).click()
                    expect(page.locator('iframe')).to_have_attribute('src',inspector_url)
                    expect(page.get_by_role('link',name='Open the official Inspector in its own tab')).to_have_attribute('href',inspector_url)
                    if args.official:
                        frame = page.frame_locator('iframe')
                        try:
                            expect(frame.locator('body')).to_contain_text('Inspector',timeout=15000)
                        except AssertionError:
                            print('Frame diagnostic:', [(f.url, f.locator('body').inner_text()[:500]) for f in page.frames if f != page.main_frame], flush=True)
                            raise
                        page.screenshot(path=str(ROOT/'target/inspector-desktop.png'))
                    page.set_viewport_size({'width':390,'height':844})
                    assert page.evaluate('document.documentElement.scrollWidth<=innerWidth+1')
                    assert page.request.get(url+'/mcp-inspector/').status == 200
                finally: browser.close()
            if args.catalogue:
                subprocess.run(['python3',str(ROOT/'scripts/test-catalogue.py'),'--url',url],check=True)
        finally:
            if inspector is not None:
                import signal
                os.killpg(inspector.pid,signal.SIGTERM)
                inspector.wait(timeout=15)
            server.terminate(); server.wait(timeout=10)
print('Inspector page passed: unlisted route, no automatic local access, safe configuration, local-only frame validation, separate-tab fallback and mobile layout.' + (' Actual official Inspector rendered.' if args.official else ''))
