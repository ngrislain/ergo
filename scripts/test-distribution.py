#!/usr/bin/env python3
"""Offline release guard tests; no registry credentials or foreign executables."""
import importlib.util
import json
import os
import pathlib
import shutil
import struct
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('package_mcp', ROOT / 'scripts/package-mcp.py')
assert spec is not None and spec.loader is not None
packaging = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packaging)


class DistributionTests(unittest.TestCase):
    def test_source_plugins_match_portable_and_claude_transports_without_credentials(self):
        root=ROOT/'plugins/ergo'
        portable=json.loads((root/'plugin.json').read_text())
        claude=json.loads((root/'.claude-plugin/plugin.json').read_text())
        self.assertEqual(portable['name'],claude['name'])
        self.assertEqual(portable['version'],packaging.version())
        self.assertEqual(claude['version'],packaging.version())
        standard=json.loads((root/'mcp.json').read_text())['mcpServers']['ergo']
        compatible=json.loads((root/'.mcp.json').read_text())['mcpServers']['ergo']
        self.assertEqual(standard.pop('type'),'stdio')
        self.assertEqual(standard['env'],{'ERGO_PUBLISH_URL':'https://ergosandbox.com'})
        self.assertEqual(compatible['env'],standard['env'])
        self.assertEqual(standard['command'],'npm')
        self.assertEqual(standard['args'],['exec','--yes','--ignore-scripts','--',
                                          f'@ngrislain/ergo-mcp@{packaging.version()}','--headless'])
        self.assertEqual(compatible['command'],'node')
        self.assertEqual(compatible['args'],['${CLAUDE_PLUGIN_ROOT}/launch-mcp.cjs','--headless'])
        self.assertIn('get_build_feedback',(root/'skills/build/SKILL.md').read_text())

    def test_claude_launcher_uses_matching_local_bridge_or_exact_npm_pin(self):
        node=shutil.which('node')
        self.assertIsNotNone(node, 'Node is required to test the plugin launcher')
        source=ROOT/'plugins/ergo'
        with tempfile.TemporaryDirectory(prefix='ergo plugin ') as temporary:
            root=pathlib.Path(temporary)
            for file in ('launch-mcp.cjs','package.json'):
                shutil.copyfile(source/file,root/file)
            npm=root/'npm'
            npm.write_text('#!/bin/sh\nprintf "%s\\n" "$@"\n')
            npm.chmod(0o755)
            local=root/'node_modules/@ngrislain/ergo-mcp'
            entry=local/'bin/ergo-mcp.cjs'
            entry.parent.mkdir(parents=True)
            def launch():
                return subprocess.check_output(
                    [node,str(root/'launch-mcp.cjs'),'--headless'],cwd=ROOT,
                    env={**os.environ,'PATH':str(root)},text=True,
                ).splitlines()
            version=packaging.version()
            fallback=['exec','--yes','--ignore-scripts','--',
                      f'@ngrislain/ergo-mcp@{version}','--headless']
            self.assertEqual(launch(),fallback)
            (local/'package.json').write_text(json.dumps({'version':version}))
            self.assertEqual(launch(),fallback)  # Incomplete cache.
            entry.write_text('console.log("plugin-local", ...process.argv.slice(2));')
            self.assertEqual(launch(),['plugin-local --headless'])
            (local/'package.json').write_text('{"version":"0.0.0"}')
            self.assertEqual(launch(),fallback)

    def test_client_plugin_packages_reuse_the_portable_package(self):
        root=ROOT/'plugins'
        portable=json.loads((root/'ergo/plugin.json').read_text())
        self.assertIn('com.openai',portable.get('extensions',{}))
        skill=(root/'ergo/skills/build/SKILL.md').read_text()
        opencode=json.loads((root/'opencode-ergo/package.json').read_text())
        self.assertEqual(opencode['version'],packaging.version())
        self.assertEqual(opencode['dependencies'].get('@ngrislain/ergo-mcp'),packaging.version())
        self.assertEqual((root/'opencode-ergo/skills/build/SKILL.md').read_text(),skill)
        index=(root/'opencode-ergo/index.ts').read_text()
        self.assertIn('path.join(dir, "node_modules", "@ngrislain", "ergo-mcp", "package.json")',index)
        self.assertIn('manifest.bin["ergo-mcp"]',index)
        self.assertIn('manifest.version === own.version',index)
        self.assertIn('["node", entry, "--headless"]',index)
        self.assertIn('["ergo-mcp", "--headless"]',index)
        self.assertIn('ERGO_PUBLISH_URL: "https://ergosandbox.com"',index)
        self.assertIn('3900000',index)
        self.assertNotIn('token',index.lower())
        pi=json.loads((root/'ergo-pi/package.json').read_text())
        self.assertEqual(pi['version'],packaging.version())
        self.assertEqual(pi['dependencies'].get('@ngrislain/ergo-mcp'),packaging.version())
        self.assertIn('pi-package',pi['keywords'])
        self.assertEqual((root/'ergo-pi/skills/build/SKILL.md').read_text(),skill)
        extension=(root/'ergo-pi/extensions/ergo.ts').read_text()
        self.assertIn('registerMcpServer("ergo"',extension)
        self.assertIn('path.join(dir, "node_modules", "@ngrislain", "ergo-mcp", "package.json")',extension)
        self.assertIn('manifest.bin["ergo-mcp"]',extension)
        self.assertIn('manifest.version === own.version',extension)
        self.assertIn('command: "ergo-mcp"',extension)
        self.assertIn('"--headless"',extension)
        self.assertIn('ERGO_PUBLISH_URL: "https://ergosandbox.com"',extension)
        self.assertIn('timeout: 3900',extension)
        self.assertNotIn('token',extension.lower())

    def test_portable_package_locks_the_pinned_bridge_for_claude(self):
        root=ROOT/'plugins/ergo'
        package=json.loads((root/'package.json').read_text())
        self.assertEqual(package['name'],'@ngrislain/ergo-plugin')
        self.assertTrue(package['private'])
        self.assertEqual(package['version'],packaging.version())
        self.assertEqual(package['dependencies'].get('@ngrislain/ergo-mcp'),packaging.version())
        lock=json.loads((root/'npm-shrinkwrap.json').read_text())
        self.assertEqual(lock['lockfileVersion'],3)
        self.assertEqual(lock['packages']['']['dependencies'],package['dependencies'])
        entry=lock['packages']['node_modules/@ngrislain/ergo-mcp']
        self.assertEqual(entry['version'],packaging.version())
        self.assertNotIn('integrity',entry)
        self.assertTrue(entry['resolved'].startswith('https://registry.npmjs.org/'))
        self.assertEqual((root/'npm-shrinkwrap.json').read_text(),packaging.plugin_lock_content(packaging.version()))

    def test_source_template_is_private_and_does_not_have_install_hooks(self):
        package = json.loads((ROOT / 'packages/mcp/package.json').read_text())
        self.assertTrue(package['private'])
        self.assertFalse(package.get('scripts'))
        self.assertEqual(package['version'], packaging.version())

    def test_native_artifacts_cannot_be_mislabeled(self):
        with tempfile.TemporaryDirectory() as temporary:
            binary = pathlib.Path(temporary) / 'ergo-mcp'
            header = bytearray(32)
            header[:6] = b'\x7fELF\x02\x01'
            header[18:20] = struct.pack('<H', 183)
            binary.write_bytes(header)
            packaging.check_binary(binary, 'linux-arm64')
            with self.assertRaises(AssertionError):
                packaging.check_binary(binary, 'linux-x64')
            with self.assertRaises(AssertionError):
                packaging.check_binary(binary, 'darwin-arm64')

    def test_mixed_or_changed_artifacts_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = pathlib.Path(temporary)
            (output / 'ergo-mcp').write_bytes(b'synthetic artifact')
            packaging.write_manifest(output, 'native', ['ergo-mcp'], 'linux-x64')
            packaging.verify_artifact(output, 'native', 'linux-x64')
            with self.assertRaises(AssertionError):
                packaging.verify_artifact(output, 'native', 'linux-arm64')
            (output / 'ergo-mcp').write_bytes(b'changed artifact')
            with self.assertRaises(AssertionError):
                packaging.verify_artifact(output, 'native', 'linux-x64')


if __name__ == '__main__':
    unittest.main()
