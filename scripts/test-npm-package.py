#!/usr/bin/env python3
"""Install a real npm tarball offline and test it outside the source checkout."""
import argparse
import json
import os
import pathlib
import re
import select
import subprocess
import sys
import tempfile
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--package-dir', type=pathlib.Path, required=True)
    parser.add_argument('--browser', action='store_true', help='Exercise real native/WASM MCP edits and PNGs with host Chrome')
    args = parser.parse_args()
    package = args.package_dir.resolve()
    manifest = json.loads((package / 'package.json').read_text())
    distribution = json.loads((package / 'distribution.json').read_text())
    assert distribution['version'] == manifest['version']
    assert distribution['complete'] or manifest.get('private') is True, 'Partial test bundles must not be publishable'
    assert not manifest.get('dependencies') and not manifest.get('scripts'), 'Consumer installs must not build/download native code'
    environment = {key:value for key,value in os.environ.items()
                   if not key.startswith(('ERGO_', 'GOOGLE_', 'S3_', 'NPM_CONFIG_'))}
    environment['ERGO_ASSET_FETCH'] = '0'
    with tempfile.TemporaryDirectory(prefix='ergo-npm-install-') as temporary:
        directory = pathlib.Path(temporary)
        config = directory / 'empty.npmrc'
        config.write_text('')
        global_config = directory / 'global.npmrc'
        global_config.write_text('')
        environment.update(NPM_CONFIG_USERCONFIG=str(config), NPM_CONFIG_GLOBALCONFIG=str(global_config),
                           NPM_CONFIG_REGISTRY='https://registry.npmjs.org/')
        packed = json.loads(subprocess.check_output(
            ['npm', 'pack', str(package), '--json', '--pack-destination', str(directory)],
            cwd=directory, env=environment, text=True))[0]
        tarball = directory / packed['filename']
        assert packed['size'] < 100 * 1024 * 1024, 'Compressed npm bundle exceeds the release budget'
        install = directory / 'installation'
        subprocess.run(['npm','install','--offline','--ignore-scripts','--no-audit','--no-fund',
                        '--prefix',str(install),str(tarball)], cwd=directory, env=environment,
                       check=True, stdout=subprocess.DEVNULL)
        executable = install / 'node_modules/.bin/ergo-mcp'
        workspace_dir = directory / 'outside checkout with spaces'
        workspace_dir.mkdir()
        version = subprocess.check_output([str(executable),'--version'],cwd=workspace_dir,env=environment,text=True).strip()
        assert version == manifest['version']
        browser_bin = directory / 'browser-bin'
        browser_bin.mkdir()
        captured_login = directory / 'login-url'
        for command in ['open', 'xdg-open']:
            opener = browser_bin / command
            opener.write_text('#!/usr/bin/env python3\nimport os, pathlib, sys\n'
                              'pathlib.Path(os.environ["ERGO_TEST_LOGIN_CAPTURE"]).write_text(sys.argv[1])\n')
            opener.chmod(0o755)
        login_environment = dict(environment, PATH=str(browser_bin)+os.pathsep+environment['PATH'],
                                 ERGO_TEST_LOGIN_CAPTURE=str(captured_login))
        for arguments, override, expected in [
            (['--login'], {}, 'https://ergosandbox.com/login#connect'),
            (['--login'], {'ERGO_PUBLISH_URL':'https://custom.example'}, 'https://custom.example/login#connect'),
            (['--login','--publish-url','http://127.0.0.1:3000'],
             {'ERGO_PUBLISH_URL':'https://custom.example'}, 'http://127.0.0.1:3000/login#connect'),
        ]:
            login = subprocess.run([str(executable),*arguments], cwd=workspace_dir,
                                   env={**login_environment,**override}, check=True,
                                   capture_output=True, text=True, timeout=15)
            assert captured_login.read_text() == expected
            assert login.stdout == '', 'Browser-login diagnostics must stay off MCP stdout'
        process = subprocess.Popen([str(executable),'--workspace',str(workspace_dir/'construction.ergo.json'),'--attach'],
                                   cwd=workspace_dir,env=environment,stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
        assert process.stdin is not None and process.stdout is not None and process.stderr is not None
        try:
            assert select.select([process.stderr],[],[],30)[0], 'No packaged runtime attachment URL'
            diagnostic = process.stderr.readline()
            found = re.search(r'http://127\.0\.0\.1:\d+/mcp\.html',diagnostic)
            assert found, diagnostic
            base = found[0].removesuffix('/mcp.html')
            for path in ['/mcp.html','/mcp-pkg/ergo_client.js','/mcp-pkg/ergo_client_bg.wasm']:
                with urllib.request.urlopen(base+path,timeout=10) as response:
                    assert response.status == 200
                    if path.endswith('.wasm'):
                        assert response.read(4) == b'\0asm'
            for sequence,method in enumerate(['initialize','tools/list','ping'],1):
                process.stdin.write(json.dumps({'jsonrpc':'2.0','id':sequence,'method':method,'params':{}})+'\n')
                process.stdin.flush()
                assert select.select([process.stdout],[],[],10)[0], 'Packaged MCP did not respond'
                reply = json.loads(process.stdout.readline())
                assert reply['id'] == sequence and 'error' not in reply, reply
                if method == 'initialize':
                    assert reply['result']['serverInfo']['version'] == manifest['version'], 'Native and npm versions differ'
                elif method == 'tools/list':
                    assert 'publish_construction' in {tool['name'] for tool in reply['result']['tools']}
        finally:
            process.stdin.close()
            process.wait(timeout=15)
            assert process.returncode == 0, process.stderr.read()
        if args.browser:
            subprocess.run([sys.executable,str(ROOT/'scripts/test-mcp.py'),'--binary',str(executable)],
                           cwd=workspace_dir,env=environment,check=True)
        print('npm package passed: offline --ignore-scripts install, executable/version, hosted login/URL overrides, bundled web/WASM, '
              'MCP JSON-RPC stdio, and startup outside the checkout.')


if __name__ == '__main__':
    main()
