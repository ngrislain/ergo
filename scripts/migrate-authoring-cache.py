#!/usr/bin/env python3
"""Explicit forward migration of accepted MCP caches to paged authoring.

Stop the MCP client first. This never publishes, edits accepted world storage,
or replays an uncertain mutation. A retained backup is archival, not a reader.
"""
import argparse
import fcntl
import json
import os
from pathlib import Path
import tempfile
import urllib.request


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise ValueError('Migration never forwards credentials across redirects')


def atomic(path, value):
    with tempfile.NamedTemporaryFile(mode='w', dir=path.parent, delete=False) as f:
        os.chmod(f.name, 0o600)
        json.dump(value, f, indent=2)
        f.flush()
        os.fsync(f.fileno())
        name = f.name
    os.replace(name, path)


def migrate(path, token_file, apply):
    path = path.resolve()
    sidecar = path.with_suffix('.shared.json')
    with path.with_suffix('.mcp.lock').open('a+') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        binding = json.loads(sidecar.read_text())
        if binding.get('format') == 'ergo-workspace-binding-2':
            return {'path': str(path), 'state': 'already_migrated'}
        if binding.get('pending'):
            raise ValueError('Pending publication: reconcile accepted history before migration')
        assembly = binding.get('assembly_id')
        if not assembly:
            raise ValueError('Unbound proposal: keep it as a native construction; do not migrate it as a cache')
        origin = binding['origin'].rstrip('/')
        import urllib.parse
        url = urllib.parse.urlsplit(origin)
        if url.scheme != 'https' or url.username or url.password or url.path or url.query or url.fragment:
            raise ValueError('Expected a credential-free HTTPS origin')
        token = token_file.read_text().strip()
        opener = urllib.request.build_opener(NoRedirect)
        configuration = json.load(opener.open(origin + '/api/config', timeout=30))
        if configuration.get('authoring_format') != 'ergo-workspace-2' or configuration.get('edit_format') != 'ergo-edit-1':
            raise ValueError('Deploy the paged authoring service first; no cache files changed')
        request = urllib.request.Request(
            f"{origin}/api/shared-worlds/{binding['world_id']}/assemblies/{assembly}/workspace",
            headers={'Authorization': 'Bearer ' + token, 'X-Ergo-Request': '1'})
        manifest = json.load(opener.open(request, timeout=60))
        if (manifest.get('format') != 'ergo-workspace-2' or manifest['assembly_id'] != assembly
                or manifest['world_id'] != binding['world_id']):
            raise ValueError('Deploy the paged authoring service first; no cache files changed')
        new = dict(format='ergo-workspace-binding-2', origin=origin+'/',
                   world_id=binding['world_id'], assembly_id=assembly,
                   base_revision=manifest['base_revision'], pending=False,
                   external_dependencies=True, receipt=None, versions={}, next=None,
                   total_pieces=manifest['piece_count'], next_local_id=manifest['next_local_id'],
                   next_group_id=manifest['next_group_id'], blocked_transform_groups=[])
        empty = dict(format='ergo-construction-1', schema=2, name=manifest['name'], geo=manifest['geo'],
                     model=dict(format='ergo-model-1', schema=2, root='main', definitions={'main': []}),
                     pieces=[], meshes={}, joints=[], fixed_bricks=[], ground=[], groups=[])
        if apply:
            backup = path.with_suffix('.pre-paging.backup.json')
            if backup.exists():
                import hashlib
                expected = binding.get('cache', {}).get('file_hash')
                if not expected or hashlib.sha256(backup.read_bytes()).hexdigest() != expected:
                    raise ValueError('Existing archival backup cannot be verified; inspect it before continuing')
            elif path.exists():
                with backup.open('xb') as out:
                    os.chmod(backup, 0o600)
                    with path.open('rb') as source:
                        import shutil
                        shutil.copyfileobj(source, out)
            # If interrupted between these atomic files, rerunning the migration
            # remains safe: the sidecar is committed last and no edit is sent.
            atomic(path, dict(format='ergo-authoring-cache-1', schema=2, partial=True, construction=empty))
            atomic(sidecar, new)
        return {'path': str(path), 'state': 'migrated' if apply else 'ready',
                'assembly_id': assembly, 'base_revision': manifest['base_revision'],
                'published': False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('workspaces', type=Path, nargs='+')
    parser.add_argument('--token-file', type=Path, required=True)
    parser.add_argument('--apply', action='store_true', help='Default is a read-only dry run')
    args = parser.parse_args()
    for path in args.workspaces:
        print(json.dumps(migrate(path, args.token_file, args.apply)))


if __name__ == '__main__':
    main()
