#!/usr/bin/env python3
"""Assemble verifiable native/web artifacts and an npm MCP distribution."""
import argparse
import hashlib
import json
import pathlib
import re
import shutil
import struct

ROOT = pathlib.Path(__file__).resolve().parents[1]
PLATFORMS = {
    'darwin-arm64': ('macho', 0x0100000C),
    'darwin-x64': ('macho', 0x01000007),
    'linux-arm64': ('elf', 183),
    'linux-x64': ('elf', 62),
}


def version():
    cargo = (ROOT / 'Cargo.toml').read_text()
    match = re.search(r'\[workspace\.package\]\s*\nversion\s*=\s*"([^"]+)"', cargo)
    assert match, 'Missing Cargo workspace version'
    manifest = json.loads((ROOT / 'packages/mcp/package.json').read_text())
    assert re.fullmatch(r'\d+\.\d+\.\d+', match[1]), 'Use a stable semver release'
    assert manifest['version'] == match[1], 'Cargo and npm versions differ'
    return match[1]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def output_directory(path):
    assert not path.exists(), f'Output already exists: {path}'
    path.mkdir(parents=True)
    return path


def check_binary(path, platform):
    with path.open('rb') as binary:
        header = binary.read(32)
    kind, expected = PLATFORMS[platform]
    if kind == 'macho':
        assert header[:4] == b'\xcf\xfa\xed\xfe' and struct.unpack('<I', header[4:8])[0] == expected, f'Wrong Mach-O platform: {platform}'
    else:
        assert header[:6] == b'\x7fELF\x02\x01' and struct.unpack('<H', header[18:20])[0] == expected, f'Wrong ELF platform: {platform}'


def write_manifest(output, kind, files, platform=None):
    manifest = {'version': version(), 'kind': kind, 'files': {name: digest(output / name) for name in sorted(files)}}
    if platform:
        manifest['platform'] = platform
    (output / 'artifact.json').write_text(json.dumps(manifest, indent=2) + '\n')


def verify_artifact(path, kind, platform=None):
    manifest = json.loads((path / 'artifact.json').read_text())
    assert manifest['version'] == version() and manifest['kind'] == kind, f'Artifact version/kind mismatch: {path}'
    if platform:
        assert manifest['platform'] == platform, f'Artifact platform mismatch: {path}'
    for name, expected in manifest['files'].items():
        relative = pathlib.PurePosixPath(name)
        assert not relative.is_absolute() and '..' not in relative.parts, 'Invalid artifact path'
        assert digest(path / name) == expected, f'Artifact checksum mismatch: {name}'
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    check = commands.add_parser('version')
    check.add_argument('--tag')
    native = commands.add_parser('native')
    native.add_argument('--platform', choices=PLATFORMS, required=True)
    native.add_argument('--binary', type=pathlib.Path, required=True)
    native.add_argument('--output', type=pathlib.Path, required=True)
    web = commands.add_parser('web')
    web.add_argument('--web-dir', type=pathlib.Path, default=ROOT / 'web')
    web.add_argument('--output', type=pathlib.Path, required=True)
    npm = commands.add_parser('npm')
    npm.add_argument('--artifacts', type=pathlib.Path, required=True)
    npm.add_argument('--output', type=pathlib.Path, required=True)
    npm.add_argument('--allow-partial', action='store_true', help='Local tests only: force private=true to prevent publishing incomplete platform sets')
    args = parser.parse_args()
    release = version()
    if args.command == 'version':
        if args.tag:
            assert args.tag == 'v' + release, 'Tag does not match Cargo/npm version'
        print(release)
        return
    if args.command == 'native':
        check_binary(args.binary, args.platform)
        output = output_directory(args.output)
        shutil.copy2(args.binary, output / 'ergo-mcp')
        (output / 'ergo-mcp').chmod(0o755)
        write_manifest(output, 'native', ['ergo-mcp'], args.platform)
    elif args.command == 'web':
        output = output_directory(args.output)
        files = ['mcp.html']
        shutil.copy2(args.web_dir / 'mcp.html', output / 'mcp.html')
        for source in (args.web_dir / 'mcp-pkg').rglob('*'):
            if source.is_file() and source.suffix in ('.js', '.wasm'):
                assert not source.is_symlink(), 'Do not bundle symlinks'
                relative = source.relative_to(args.web_dir)
                (output / relative).parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(source, output / relative)
                files.append(relative.as_posix())
        assert (output / 'mcp-pkg/ergo_client_bg.wasm').read_bytes()[:4] == b'\0asm', 'Missing/invalid WASM runtime'
        assert (output / 'mcp-pkg/ergo_client.js').is_file(), 'Missing JS runtime'
        write_manifest(output, 'web', files)
    else:
        available = [key for key in PLATFORMS if (args.artifacts / ('native-' + key)).is_dir()]
        complete = len(available) == len(PLATFORMS)
        assert complete or args.allow_partial, 'A publishable package needs all four native platforms'
        assert available, 'No native artifacts'
        web_artifact = args.artifacts / 'web-runtime'
        web_manifest = verify_artifact(web_artifact, 'web')
        output = output_directory(args.output)
        package = json.loads((ROOT / 'packages/mcp/package.json').read_text())
        if complete:
            package.pop('private', None)
        else:
            package['private'] = True
        (output / 'package.json').write_text(json.dumps(package, indent=2) + '\n')
        shutil.copytree(ROOT / 'packages/mcp/bin', output / 'bin')
        (output / 'bin/ergo-mcp.cjs').chmod(0o755)
        shutil.copy2(ROOT / 'packages/mcp/README.md', output / 'README.md')
        shutil.copy2(ROOT / 'LICENSE', output / 'LICENSE')
        for key in available:
            artifact = args.artifacts / ('native-' + key)
            manifest = verify_artifact(artifact, 'native', key)
            assert set(manifest['files']) == {'ergo-mcp'}, 'Unexpected native artifact contents'
            check_binary(artifact / 'ergo-mcp', key)
            destination = output / 'native' / key
            destination.mkdir(parents=True)
            shutil.copy2(artifact / 'ergo-mcp', destination / 'ergo-mcp')
            (destination / 'ergo-mcp').chmod(0o755)
        for name in web_manifest['files']:
            destination = output / 'web' / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(web_artifact / name, destination)
        distribution = {'version': release, 'platforms': available, 'complete': complete,
                        'binaries': {key: digest(output / 'native' / key / 'ergo-mcp') for key in available},
                        'web': web_manifest['files']}
        (output / 'distribution.json').write_text(json.dumps(distribution, indent=2) + '\n')
        print(f'Assembled {package["name"]}@{release}; platforms={",".join(available)}; publishable={complete}')


if __name__ == '__main__':
    main()
