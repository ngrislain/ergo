'use strict';
const assert = require('node:assert/strict');
const { test } = require('node:test');
const { copyFileSync, mkdtempSync, mkdirSync, realpathSync, writeFileSync, rmSync } = require('node:fs');
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

test('the launched bridge defaults hosted login/publishing and preserves local editing and overrides', () => {
  const root = realpathSync(mkdtempSync(path.join(tmpdir(), 'ergo-npm-hosted-')));
  try {
    const launcher = path.join(root, 'bin/ergo-mcp.cjs');
    mkdirSync(path.dirname(launcher), { recursive: true });
    copyFileSync(path.join(__dirname, '../bin/ergo-mcp.cjs'), launcher);
    const binary = path.join(root, `native/${process.platform}-${process.arch}/ergo-mcp`);
    mkdirSync(path.dirname(binary), { recursive: true });
    writeFileSync(binary, `#!/usr/bin/env node
process.stdout.write(JSON.stringify({
  url: process.env.ERGO_PUBLISH_URL || null,
  args: process.argv.slice(2),
  web: process.env.ERGO_WEB_DIR,
  cwd: process.cwd(),
}) + '\\n');
`, { mode: 0o755 });
    for (const file of ['web/mcp.html', 'web/mcp-pkg/ergo_client.js', 'web/mcp-pkg/ergo_client_bg.wasm']) {
      const destination = path.join(root, file);
      mkdirSync(path.dirname(destination), { recursive: true });
      writeFileSync(destination, 'fixture');
    }
    const cleanEnvironment = Object.fromEntries(Object.entries(process.env)
      .filter(([key]) => !key.startsWith('ERGO_')));
    const cases = [
      { args: ['--attach'], env: {}, url: null },
      { args: ['--login'], env: {}, url: 'https://ergosandbox.com' },
      { args: ['--attach'], env: { ERGO_PUBLISH_TOKEN: 'test.delegation' }, url: 'https://ergosandbox.com' },
      { args: ['--attach'], env: { ERGO_PUBLISH_TOKEN_FILE: '/tmp/token with spaces' }, url: 'https://ergosandbox.com' },
      { args: ['--publish-token-file', '/tmp/token with spaces'], env: {}, url: 'https://ergosandbox.com' },
      { args: ['--login'], env: { ERGO_PUBLISH_URL: 'https://custom.example' }, url: 'https://custom.example' },
      { args: ['--login', '--publish-url', 'http://127.0.0.1:3000'], env: {}, url: 'https://ergosandbox.com' },
    ];
    for (const entry of cases) {
      const result = spawnSync(process.execPath, [launcher, ...entry.args], {
        cwd: root, env: { ...cleanEnvironment, ...entry.env }, encoding: 'utf8', timeout: 10000,
      });
      assert.equal(result.status, 0, result.stderr);
      assert.equal(result.stderr, '');
      assert.deepEqual(JSON.parse(result.stdout), {
        url: entry.url, args: entry.args, web: path.join(root, 'web'), cwd: root,
      });
    }
  } finally { rmSync(root, { recursive: true, force: true }); }
});
