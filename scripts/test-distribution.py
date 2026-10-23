#!/usr/bin/env python3
"""Offline release guard tests; no registry credentials or foreign executables."""
import importlib.util
import json
import pathlib
import struct
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('package_mcp', ROOT / 'scripts/package-mcp.py')
assert spec is not None and spec.loader is not None
packaging = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packaging)


class DistributionTests(unittest.TestCase):
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
