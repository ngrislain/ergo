#!/usr/bin/env python3
"""Exercise plugin publication boundaries without contacting or writing to npm."""
import importlib.util,io,json,pathlib,shutil,subprocess,tarfile,tempfile,unittest
ROOT=pathlib.Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('packaging',ROOT/'scripts/package-mcp.py')
assert spec and spec.loader
packaging=importlib.util.module_from_spec(spec);spec.loader.exec_module(packaging)
spec=importlib.util.spec_from_file_location('plugin_publisher',ROOT/'scripts/publish-plugins.py')
assert spec and spec.loader
publisher=importlib.util.module_from_spec(spec);spec.loader.exec_module(publisher)

def integrity(path):
    import base64,hashlib
    return 'sha512-'+base64.b64encode(hashlib.sha512(pathlib.Path(path).read_bytes()).digest()).decode()

def write_tarball(path,entries):
    with tarfile.open(path,'w:gz') as archive:
        for name in entries:
            raw=b'plugin content\n';member=tarfile.TarInfo('package/'+name);member.size=len(raw);archive.addfile(member,io.BytesIO(raw))
    return pathlib.Path(path)

class PluginPublication(unittest.TestCase):
    def setUp(self):
        self.version=packaging.version()
        self.package=next(iter(publisher.PLUGINS))
        self.directory=tempfile.TemporaryDirectory();self.addCleanup(self.directory.cleanup)
        self.tarball=write_tarball(pathlib.Path(self.directory.name)/'packed.tgz',publisher.CONTENTS[self.package])
    def is_bridge(self,command):
        return command[1]=='view' and command[2]==publisher.BRIDGE+'@'+self.version
    def pack(self,command,**kwargs):
        assert command[1]=='pack'
        shutil.copy2(self.tarball,command[-1])
        return subprocess.CompletedProcess(command,0,str(self.tarball),'')
    def test_released_packages_pin_the_matching_bridge(self):
        for package in publisher.PLUGINS:
            data=json.loads((publisher.PLUGINS[package]/'package.json').read_text())
            self.assertEqual(data['version'],self.version)
            self.assertEqual(data['dependencies'][publisher.BRIDGE],self.version)
            publisher.manifest(package,self.version)
    def test_unpublished_bridge_stops_before_packing(self):
        def offline(command,**kwargs):
            if command[1]!='view':self.fail('Only registry reads may run: '+str(command))
            return subprocess.CompletedProcess(command,1,json.dumps({'error':{'code':'E404'}}),'')
        with self.assertRaisesRegex(RuntimeError,'bridge'):publisher.publish(self.package,self.version,offline)
    def test_existing_version_is_only_accepted_if_the_exact_artifact_matches(self):
        calls=[]
        def existing(command,**kwargs):
            calls.append(command)
            if command[1]=='pack':return self.pack(command,**kwargs)
            value=integrity(self.tarball) if not self.is_bridge(command) else 'sha512-bridge'
            return subprocess.CompletedProcess(command,0,json.dumps(value),'')
        publisher.publish(self.package,self.version,existing)
        self.assertEqual([command[1] for command in calls],['view','pack','view'])
        calls.clear()
        def different(command,**kwargs):
            calls.append(command)
            if command[1]=='pack':return self.pack(command,**kwargs)
            value='sha512-other' if not self.is_bridge(command) else 'sha512-bridge'
            return subprocess.CompletedProcess(command,0,json.dumps(value),'')
        with self.assertRaises(RuntimeError):publisher.publish(self.package,self.version,different)
        self.assertEqual([command[1] for command in calls],['view','pack','view'])
    def test_missing_version_is_packed_published_once_and_read_back(self):
        calls=[]
        def registry(command,**kwargs):
            calls.append(command)
            if command[1]=='pack':return self.pack(command,**kwargs)
            if command[1]=='publish':
                self.assertTrue(pathlib.Path(command[2]).is_file())
                return subprocess.CompletedProcess(command,0,'','')
            if self.is_bridge(command):return subprocess.CompletedProcess(command,0,json.dumps('sha512-bridge'),'')
            if len(calls)<5:return subprocess.CompletedProcess(command,1,json.dumps({'error':{'code':'E404'}}),'')
            return subprocess.CompletedProcess(command,0,json.dumps(integrity(self.tarball)),'')
        publisher.publish(self.package,self.version,registry)
        self.assertEqual([command[1] for command in calls],['view','pack','view','publish','view'])
        self.assertIn('--provenance',calls[3]);self.assertIn('public',calls[3]);self.assertIn('--ignore-scripts',calls[3])
    def test_packed_contents_are_validated_before_publication(self):
        write_tarball(self.tarball,sorted(publisher.CONTENTS[self.package])[:-1])
        calls=[]
        def registry(command,**kwargs):
            calls.append(command)
            if command[1]=='pack':return self.pack(command,**kwargs)
            return subprocess.CompletedProcess(command,0,json.dumps('sha512-bridge'),'')
        with self.assertRaisesRegex(ValueError,'contents'):publisher.publish(self.package,self.version,registry)
        self.assertEqual([command[1] for command in calls],['view','pack'])
    def test_registry_processing_polls_reads_without_republishing(self):
        calls=[];waits=[]
        def registry(command,**kwargs):
            calls.append(command)
            if command[1]=='pack':return self.pack(command,**kwargs)
            if command[1]=='publish':return subprocess.CompletedProcess(command,0,'','')
            if self.is_bridge(command):return subprocess.CompletedProcess(command,0,json.dumps('sha512-bridge'),'')
            return subprocess.CompletedProcess(command,1,json.dumps({'error':{'code':'E404'}}),'')
        with self.assertRaisesRegex(RuntimeError,'still processing'):
            publisher.publish(self.package,self.version,registry,waits.append)
        self.assertEqual(sum(command[1]=='publish' for command in calls),1)
        self.assertEqual(len(waits),60)

if __name__=='__main__':
    import argparse,sys
    parser=argparse.ArgumentParser();parser.add_argument('--pack',action='store_true',help='Also pack the real plugin directories and validate their contents')
    arguments=parser.parse_args()
    if arguments.pack:
        for package in publisher.PLUGINS:
            with tempfile.TemporaryDirectory() as temporary:
                packed=publisher.pack(package,pathlib.Path(temporary),subprocess.run)
                print(f'{package}: packed {packed.name} with the declared contents')
    unittest.main(argv=[sys.argv[0]])
