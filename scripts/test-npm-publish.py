#!/usr/bin/env python3
"""Exercise publish effect boundaries without contacting or writing to npm."""
import base64,hashlib,importlib.util,io,json,pathlib,subprocess,tarfile,tempfile,unittest
ROOT=pathlib.Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('npm_publisher',ROOT/'scripts/publish-npm.py')
assert spec and spec.loader
publisher=importlib.util.module_from_spec(spec);spec.loader.exec_module(publisher)

class Publication(unittest.TestCase):
    def setUp(self):
        self.directory=tempfile.TemporaryDirectory();self.addCleanup(self.directory.cleanup)
        self.tarball=pathlib.Path(self.directory.name)/'bundle.tgz'
        self.bundle()
    def bundle(self,private=False,platforms=None):
        values={'package.json':{'name':publisher.NAME,'version':'0.5.16','private':private},
            'distribution.json':{'version':'0.5.16','complete':True,'platforms':sorted(publisher.PLATFORMS if platforms is None else platforms)}}
        with tarfile.open(self.tarball,'w:gz') as archive:
            for name,value in values.items():
                raw=json.dumps(value).encode();member=tarfile.TarInfo('package/'+name);member.size=len(raw);archive.addfile(member,io.BytesIO(raw))
    def integrity(self):return 'sha512-'+base64.b64encode(hashlib.sha512(self.tarball.read_bytes()).digest()).decode()
    def test_private_or_incomplete_source_never_reaches_npm(self):
        def forbidden(*args,**kwargs):self.fail('Invalid source package must not invoke npm')
        self.bundle(private=True)
        with self.assertRaises(ValueError):publisher.publish(self.tarball,'0.5.16',forbidden)
        self.bundle(platforms={'darwin-arm64'})
        with self.assertRaises(ValueError):publisher.publish(self.tarball,'0.5.16',forbidden)
    def test_existing_version_is_only_accepted_if_the_exact_artifact_matches(self):
        calls=[]
        def existing(command,**kwargs):
            calls.append(command);return subprocess.CompletedProcess(command,0,json.dumps(self.integrity()),'')
        publisher.publish(self.tarball,'0.5.16',existing)
        self.assertEqual(len(calls),1);self.assertEqual(calls[0][1],'view')
        def different(command,**kwargs):return subprocess.CompletedProcess(command,0,json.dumps('sha512-other'),'')
        with self.assertRaises(RuntimeError):publisher.publish(self.tarball,'0.5.16',different)
    def test_auth_failure_does_not_attempt_publication(self):
        calls=[]
        def denied(command,**kwargs):
            calls.append(command);return subprocess.CompletedProcess(command,1,json.dumps({'error':{'code':'E401'}}),'')
        with self.assertRaises(RuntimeError):publisher.publish(self.tarball,'0.5.16',denied)
        self.assertEqual(len(calls),1)
    def test_missing_version_is_published_once_and_integrity_is_read_back(self):
        calls=[]
        def registry(command,**kwargs):
            calls.append(command)
            if len(calls)==1:return subprocess.CompletedProcess(command,1,json.dumps({'error':{'code':'E404'}}),'')
            if command[1]=='publish':return subprocess.CompletedProcess(command,0,'','')
            return subprocess.CompletedProcess(command,0,json.dumps(self.integrity()),'')
        publisher.publish(self.tarball,'0.5.16',registry)
        self.assertEqual([command[1] for command in calls],['view','publish','view'])
        self.assertIn('--provenance',calls[1]);self.assertIn('--ignore-scripts',calls[1]);self.assertIn('public',calls[1])

if __name__=='__main__':unittest.main()
