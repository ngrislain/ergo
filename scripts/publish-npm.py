#!/usr/bin/env python3
"""Publish only a complete, verified MCP bundle; reconcile immutable npm versions."""
import argparse
import base64
import hashlib
import json
import pathlib
import subprocess
import tarfile

REGISTRY='https://registry.npmjs.org/'
NAME='@ngrislain/ergo-mcp'
PLATFORMS={'darwin-arm64','darwin-x64','linux-arm64','linux-x64'}

def metadata(tarball,version):
    with tarfile.open(tarball,'r:gz') as archive:
        def read(name):
            members=[member for member in archive.getmembers() if member.name=='package/'+name]
            if len(members)!=1 or not members[0].isfile() or members[0].size>1024*1024:raise ValueError('Invalid bundle metadata: '+name)
            stream=archive.extractfile(members[0])
            if stream is None:raise ValueError('Missing bundle metadata: '+name)
            return json.load(stream)
        package=read('package.json');distribution=read('distribution.json')
    if package.get('name')!=NAME or package.get('version')!=version or package.get('private'):raise ValueError('Publish the assembled versioned MCP bundle, not the source package')
    if distribution.get('version')!=version or not distribution.get('complete') or set(distribution.get('platforms',[]))!=PLATFORMS:raise ValueError('All four verified native platforms are required')
    return package

def publish(tarball,version,run=subprocess.run):
    metadata(tarball,version)
    integrity='sha512-'+base64.b64encode(hashlib.sha512(tarball.read_bytes()).digest()).decode()
    existing=run(['npm','view',NAME+'@'+version,'dist.integrity','--json','--registry='+REGISTRY],capture_output=True,text=True)
    if existing.returncode==0:
        if json.loads(existing.stdout)!=integrity:raise RuntimeError('This npm version already contains a different artifact; choose a new version')
        print(json.dumps({'package':NAME,'version':version,'state':'already_published_identical','integrity':integrity}));return
    try:missing=json.loads(existing.stdout).get('error',{}).get('code')=='E404'
    except (json.JSONDecodeError,AttributeError):missing=False
    if not missing:raise RuntimeError('Could not verify npm version availability; publication was not attempted')
    run(['npm','publish',str(tarball),'--access','public','--provenance','--ignore-scripts','--registry='+REGISTRY],check=True)
    verified=run(['npm','view',NAME+'@'+version,'dist.integrity','--json','--registry='+REGISTRY],capture_output=True,text=True,check=True)
    if json.loads(verified.stdout)!=integrity:raise RuntimeError('Published npm integrity differs from the verified bundle')
    print(json.dumps({'package':NAME,'version':version,'state':'published_verified','integrity':integrity}))

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('tarball',type=pathlib.Path);parser.add_argument('--version',required=True)
    args=parser.parse_args();publish(args.tarball,args.version)
if __name__=='__main__':main()
