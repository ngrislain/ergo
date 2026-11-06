#!/usr/bin/env python3
"""Browser-approved native token pickup against a disposable authenticated server."""
import argparse
import importlib.util
import hashlib
import json
import os
import pathlib
import socket
import select
import subprocess
import tempfile
import time
import urllib.parse
from playwright.sync_api import sync_playwright, expect

ROOT = pathlib.Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', default=str(ROOT/'target/debug/ergo-mcp'))
args = parser.parse_args()
spec = importlib.util.spec_from_file_location('publish_helpers', ROOT/'scripts/test-mcp-publish.py')
helpers = importlib.util.module_from_spec(spec)
spec.loader.exec_module(helpers)
with socket.socket() as sock:
    sock.bind(('127.0.0.1', 0))
    port = sock.getsockname()[1]
url = f'http://127.0.0.1:{port}'
with tempfile.TemporaryDirectory(prefix='ergo-login-') as temporary:
    directory = pathlib.Path(temporary)
    (directory/'web').symlink_to(ROOT/'web', target_is_directory=True)
    (directory/'store/users').mkdir(parents=True)
    (directory/'store/users/alice.json').write_text(json.dumps({'id':'alice','name':'Alice Builder','email':'alice@example.test','picture':None,'created_at':1,'last_login':1}))
    (directory/'bin').mkdir()
    # Capture automatic browser opening without disturbing the user's real browser.
    for command in ['open','xdg-open']:
        script = directory/'bin'/command
        script.write_text('#!/bin/sh\nprintf "%s" "$1" > "$ERGO_TEST_LOGIN_URL"\n')
        script.chmod(0o700)
    env = {key:value for key,value in os.environ.items() if not key.startswith(('ERGO_','GOOGLE_','S3_','AWS_'))}
    env.update(ERGO_ASSET_FETCH='0', ERGO_GROUND_SOURCE='disabled', ERGO_ISOLATED_FIXTURES='1', ERGO_WEB_DIR=str(ROOT/'web'),
               PATH=str(directory/'bin')+os.pathsep+env['PATH'], ERGO_TEST_LOGIN_URL=str(directory/'login-url'),
               XDG_CONFIG_HOME=str(directory/'config'))
    server_env = {**env,'ERGO_BIND':f'127.0.0.1:{port}','ERGO_PUBLIC_URL':url,'ERGO_DATA_DIR':str(directory/'store'),
                  'GOOGLE_CLIENT_ID':'test','GOOGLE_CLIENT_SECRET':'test','ERGO_SESSION_SECRET':helpers.SECRET.decode()}
    with tempfile.TemporaryFile() as log:
        server = subprocess.Popen([str(ROOT/'target/debug/ergo-server')], cwd=directory, env=server_env, stdout=log, stderr=log)
        try:
            for _ in range(100):
                try:
                    if helpers.http(url+'/api/config')[0] == 200: break
                except OSError: time.sleep(.1)
            else: raise AssertionError('Test server did not start')
            with sync_playwright() as p:
                browser = p.chromium.launch(channel='chrome',headless=True,args=['--enable-unsafe-swiftshader'])
                context = browser.new_context()
                context.add_cookies([{'name':'ergo_session','value':helpers.signed_session('alice'),'url':url,'httpOnly':True}])
                page = context.new_page()
                for case in ['login','missing','rejected','implicit']:
                    destination = (directory/'config/ergo/tokens'/(hashlib.sha256(url.encode()).hexdigest()+'.token')) if case == 'implicit' else directory/(case+'-token')
                    if case == 'rejected': destination.write_text('expired.token')
                    captured = directory/'login-url'
                    captured.unlink(missing_ok=True)
                    command = [args.binary,'--publish-url',url]
                    if case != 'implicit': command += ['--publish-token-file',str(destination)]
                    command += ['--login'] if case == 'login' else ['--attach','--workspace',str(directory/(case+'.json'))]
                    process = subprocess.Popen(command, cwd=directory, env=env, stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=log,text=True)
                    try:
                        for _ in range(100):
                            if captured.exists() and captured.read_text(): break
                            assert process.poll() is None, 'MCP exited before opening sign-in'
                            time.sleep(.05)
                        else: raise AssertionError('Missing automatic browser sign-in')
                        login_url = captured.read_text()
                        parsed = urllib.parse.urlparse(login_url)
                        assert parsed.path == '/login' and parsed.fragment == 'connect'
                        assert 'token' not in urllib.parse.parse_qs(parsed.query)
                        page.goto(login_url)
                        page.wait_for_load_state('networkidle')
                        page.get_by_role('button',name='Authorize MCP server',exact=True).click()
                        page.wait_for_url('http://127.0.0.1:*/callback/*')
                        expect(page.get_by_role('heading',name='Authorization received',exact=True)).to_be_visible()
                        for _ in range(100):
                            if destination.exists() and destination.read_text() != 'expired.token': break
                            time.sleep(.05)
                        else: raise AssertionError('MCP did not save the approved token')
                        token = destination.read_text()
                        assert destination.stat().st_mode & 0o777 == 0o600
                        assert helpers.http(url+'/api/shared-worlds/allowance',token=token)[0] == 200
                        if case == 'login':
                            stdout, _ = process.communicate(timeout=10)
                            assert process.returncode == 0 and stdout == ''
                        else:
                            process.stdin.write(json.dumps({'jsonrpc':'2.0','id':1,'method':'initialize','params':{}})+'\n')
                            process.stdin.flush()
                            assert json.loads(process.stdout.readline())['id'] == 1
                            process.stdin.close(); process.wait(timeout=10)
                            assert process.returncode == 0
                    finally:
                        if process.poll() is None: process.terminate(); process.wait(timeout=10)
                # Restart without a token setting: the origin-specific saved
                # credential must be reused, not prompt on every connection.
                captured.unlink(missing_ok=True)
                process = subprocess.Popen(command,cwd=directory,env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=log,text=True)
                try:
                    process.stdin.write(json.dumps({'jsonrpc':'2.0','id':1,'method':'initialize','params':{}})+'\n')
                    process.stdin.flush()
                    assert select.select([process.stdout],[],[],10)[0], 'Cached sign-in did not initialize'
                    assert json.loads(process.stdout.readline())['id']==1
                    assert not captured.exists(), 'A saved credential should not reopen sign-in'
                    process.stdin.close(); process.wait(timeout=10)
                finally:
                    if process.poll() is None: process.terminate(); process.wait(timeout=10)
                for callback in ['https://attacker.example/callback/'+'a'*64, 'http://127.0.0.1:1234/unsafe']:
                    page.goto(url+'/login?'+urllib.parse.urlencode({'mcp_callback':callback})+'#connect')
                    expect(page.get_by_role('alert')).to_contain_text('Invalid MCP callback')
                    expect(page.get_by_role('button',name='Generate access token',exact=True)).to_be_disabled()
                browser.close()
        finally:
            server.terminate(); server.wait(timeout=10)
print('MCP login passed: explicit login, missing/rejected tokens, human browser approval, loopback POST, private storage, protocol-only stdout and callback validation.')
