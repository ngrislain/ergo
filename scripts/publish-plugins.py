#!/usr/bin/env python3
"""Publish the version-locked plugin packages; reconcile immutable npm versions."""
import argparse
import base64
import hashlib
import json
import pathlib
import subprocess
import tarfile
import tempfile
import time

REGISTRY='https://registry.npmjs.org/'
BRIDGE='@ngrislain/ergo-mcp'
ROOT=pathlib.Path(__file__).resolve().parents[1]
PLUGINS={
    '@ngrislain/opencode-ergo': ROOT/'plugins/opencode-ergo',
    '@ngrislain/ergo-pi': ROOT/'plugins/ergo-pi',
}
CONTENTS={
    '@ngrislain/opencode-ergo': {'package.json','index.ts','skills/build/SKILL.md'},
    '@ngrislain/ergo-pi': {'package.json','extensions/ergo.ts','skills/build/SKILL.md'},
}


def manifest(package,version):
    data=json.loads((PLUGINS[package]/'package.json').read_text())
    if data.get('name')!=package:raise ValueError('Plugin package name mismatch: '+package)
    if data.get('version')!=version:raise ValueError('Plugin version differs from the release: '+package)
    if data.get('private'):raise ValueError('Publishable plugin packages must not be private: '+package)
    if data.get('publishConfig',{}).get('access')!='public':raise ValueError('Scoped plugin packages must declare public access: '+package)
    if data.get('dependencies',{}).get(BRIDGE)!=version:raise ValueError('Plugin must pin the bridge to its own version: '+package)
    return data


def check_bridge(version,run):
    result=run(['npm','view',BRIDGE+'@'+version,'dist.integrity','--json','--prefer-online','--registry='+REGISTRY],capture_output=True,text=True)
    if result.returncode!=0:
        raise RuntimeError('The pinned bridge version is not verified on npm; publish it before the plugins')


def missing(result):
    try:return result.returncode!=0 and json.loads(result.stdout).get('error',{}).get('code')=='E404'
    except (json.JSONDecodeError,AttributeError):return False


def pack(package,destination,run):
    # npm treats directory/file.tgz as GitHub shorthand without a leading ./.
    # An absolute local path is unambiguous for every caller and working dir.
    run(['npm','pack',str(PLUGINS[package].resolve()),'--pack-destination',str(destination)],check=True)
    tarballs=list(destination.glob('*.tgz'))
    if len(tarballs)!=1:raise RuntimeError('Expected exactly one packed plugin tarball: '+package)
    tarball=tarballs[0].resolve(strict=True)
    with tarfile.open(tarball,'r:gz') as archive:
        names={member.name[len('package/'):] for member in archive.getmembers()
               if member.name.startswith('package/') and member.isfile()}
    if names!=CONTENTS[package]:
        raise ValueError(f'Packed {package} contents differ from the declared plugin: {sorted(names ^ CONTENTS[package])}')
    return tarball


def publish(package,version,run=subprocess.run,wait=time.sleep):
    manifest(package,version)
    check_bridge(version,run)
    with tempfile.TemporaryDirectory() as temporary:
        tarball=pack(package,pathlib.Path(temporary),run)
        value='sha512-'+base64.b64encode(hashlib.sha512(tarball.read_bytes()).digest()).decode()
        query=['npm','view',package+'@'+version,'dist.integrity','--json','--prefer-online','--registry='+REGISTRY]
        existing=run(query,capture_output=True,text=True)
        if existing.returncode==0:
            if json.loads(existing.stdout)!=value:raise RuntimeError('This npm version already contains a different artifact; choose a new version')
            print(json.dumps({'package':package,'version':version,'state':'already_published_identical'}));return
        if not missing(existing):raise RuntimeError('Could not verify npm version availability; publication was not attempted')
        run(['npm','publish',str(tarball),'--access','public','--provenance','--ignore-scripts','--registry='+REGISTRY],check=True)
        # An accepted npm publication may remain in registry processing for minutes.
        # Poll reads only; never repeat the publication to resolve that delay.
        for attempt in range(61):
            verified=run(query,capture_output=True,text=True)
            if verified.returncode==0:
                if json.loads(verified.stdout)!=value:raise RuntimeError('Published npm integrity differs from the packed plugin')
                print(json.dumps({'package':package,'version':version,'state':'published_verified'}));return
            if not missing(verified):raise RuntimeError('npm accepted publication but integrity read-back failed; reconcile before any retry')
            if attempt<60:wait(10)
        raise RuntimeError('npm accepted publication but it is still processing after 10 minutes; check registry integrity before any retry')


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--version',required=True)
    args=parser.parse_args()
    for package in PLUGINS:publish(package,args.version)
if __name__=='__main__':main()
