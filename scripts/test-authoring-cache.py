#!/usr/bin/env python3
"""Offline forward-migration tests. Never contact a world or publish an edit."""
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('migration', Path(__file__).with_name('migrate-authoring-cache.py'))
migration = importlib.util.module_from_spec(spec)
spec.loader.exec_module(migration)


class Opener:
    def __init__(self, manifest):
        self.manifest = manifest
        self.urls = []

    def open(self, request, timeout):
        url = request if isinstance(request, str) else request.full_url
        self.urls.append(url)
        value = {'authoring_format': 'ergo-workspace-2','edit_format':'ergo-edit-1'} if url.endswith('/api/config') else self.manifest
        return io.StringIO(json.dumps(value))


class MigrationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.path = self.root / 'agent.cache.json'
        self.old = b'{"original":"accepted cache retained verbatim"}'
        self.path.write_bytes(self.old)
        self.sidecar = self.path.with_suffix('.shared.json')
        self.binding = dict(origin='https://fixture.example/', world_id='world', assembly_id='a'*64,
                            base_revision='b'*64, pending=False,
                            cache={'file_hash': hashlib.sha256(self.old).hexdigest()})
        self.sidecar.write_text(json.dumps(self.binding))
        self.token = self.root / 'token'
        self.token.write_text('test-token')
        self.manifest = dict(format='ergo-workspace-2', world_id='world', assembly_id='a'*64,
                             base_revision='c'*64, name='Accepted model', geo={'latitude':0,'longitude':0,'arcseconds_per_block':1},
                             piece_count=75000, next_local_id=75001, next_group_id=501)
        self.opener = Opener(self.manifest)

    def migrate(self, apply=False):
        with patch.object(migration.urllib.request, 'build_opener', return_value=self.opener):
            return migration.migrate(self.path, self.token, apply)

    def test_dry_run_changes_no_cache_data(self):
        before = self.sidecar.read_bytes()
        self.assertEqual(self.migrate()['state'], 'ready')
        self.assertEqual(self.path.read_bytes(), self.old)
        self.assertEqual(self.sidecar.read_bytes(), before)
        self.assertFalse(self.path.with_suffix('.pre-paging.backup.json').exists())

    def test_apply_archives_exact_bytes_and_commits_new_binding_last(self):
        result = self.migrate(True)
        self.assertFalse(result['published'])
        self.assertEqual(self.path.with_suffix('.pre-paging.backup.json').read_bytes(), self.old)
        new = json.loads(self.sidecar.read_text())
        self.assertEqual(new['format'], 'ergo-workspace-binding-2')
        self.assertEqual(new['total_pieces'], 75000)
        self.assertEqual(new['next_local_id'], 75001)
        cache = json.loads(self.path.read_text())
        self.assertEqual(cache['format'], 'ergo-authoring-cache-1')
        self.assertTrue(cache['partial'])
        self.assertEqual(cache['construction']['pieces'], [])
        self.assertEqual(self.migrate(True)['state'], 'already_migrated')

    def test_uncertain_or_unbound_work_is_never_discarded(self):
        for changes in [dict(pending=True), dict(assembly_id=None)]:
            self.sidecar.write_text(json.dumps({**self.binding, **changes}))
            with self.assertRaises(ValueError):
                self.migrate(True)
            self.assertEqual(self.path.read_bytes(), self.old)
        self.assertEqual(self.opener.urls, [])

    def test_mismatched_manifest_does_not_write(self):
        self.manifest['assembly_id'] = 'd'*64
        with self.assertRaises(ValueError):
            self.migrate(True)
        self.assertEqual(self.path.read_bytes(), self.old)

    def test_interrupted_migration_can_resume_with_verified_archive(self):
        self.path.with_suffix('.pre-paging.backup.json').write_bytes(self.old)
        self.path.write_text('{"interrupted":"empty view written before binding"}')
        self.assertEqual(self.migrate(True)['state'], 'migrated')
        self.assertEqual(self.path.with_suffix('.pre-paging.backup.json').read_bytes(), self.old)

    def test_unverified_existing_archive_is_not_overwritten(self):
        self.path.with_suffix('.pre-paging.backup.json').write_bytes(b'other user work')
        with self.assertRaises(ValueError):
            self.migrate(True)
        self.assertEqual(self.path.read_bytes(), self.old)


if __name__ == '__main__':
    unittest.main()
