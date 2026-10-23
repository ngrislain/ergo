'use strict';
const assert = require('node:assert/strict');
const { test } = require('node:test');
const { mkdtempSync, mkdirSync, writeFileSync, rmSync } = require('node:fs');
const { tmpdir } = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { resolveRuntime, PLATFORMS } = require('../bin/ergo-mcp.cjs');

test('runtime lookup reports unsupported and incomplete installations clearly', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'ergo-npm-launcher-'));
  try {
    assert.throws(() => resolveRuntime(root, 'win32', 'x64'), /no prebuilt runtime/);
    assert.throws(() => resolveRuntime(root, 'darwin', 'arm64'), /incomplete/);
    for (const file of ['native/darwin-arm64/ergo-mcp', 'web/mcp.html',
      'web/mcp-pkg/ergo_client.js', 'web/mcp-pkg/ergo_client_bg.wasm']) {
      const destination = path.join(root, file);
      mkdirSync(path.dirname(destination), { recursive: true });
      writeFileSync(destination, 'fixture');
    }
    assert.deepEqual(resolveRuntime(root, 'darwin', 'arm64'), {
      binary: path.join(root, 'native', 'darwin-arm64', 'ergo-mcp'), web: path.join(root, 'web'),
    });
    assert.equal(PLATFORMS.size, 4);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('version is available without native startup or MCP stdout noise', () => {
  const result = spawnSync(process.execPath, [path.join(__dirname, '../bin/ergo-mcp.cjs'), '--version'], { encoding: 'utf8' });
  assert.equal(result.status, 0);
  assert.equal(result.stdout.trim(), require('../package.json').version);
  assert.equal(result.stderr, '');
});
