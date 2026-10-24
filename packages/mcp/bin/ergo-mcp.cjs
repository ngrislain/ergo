#!/usr/bin/env node
'use strict';

const fs = require('node:fs');
const path = require('node:path');
const { spawn } = require('node:child_process');

const PLATFORMS = new Set(['darwin-arm64', 'darwin-x64', 'linux-arm64', 'linux-x64']);

function resolveRuntime(root, platform = process.platform, arch = process.arch) {
  const key = `${platform}-${arch}`;
  if (!PLATFORMS.has(key)) {
    throw new Error(`Ergo MCP has no prebuilt runtime for ${key}. Supported: ${[...PLATFORMS].join(', ')}.`);
  }
  const binary = path.join(root, 'native', key, 'ergo-mcp');
  const web = path.join(root, 'web');
  for (const file of [binary, path.join(web, 'mcp.html'),
    path.join(web, 'mcp-pkg', 'ergo_client.js'), path.join(web, 'mcp-pkg', 'ergo_client_bg.wasm')]) {
    if (!fs.existsSync(file)) {
      throw new Error(`The Ergo MCP package is incomplete (${path.relative(root, file)} is missing). Reinstall the published package; a source checkout must be assembled before npm pack.`);
    }
  }
  return { binary, web };
}

function main() {
  const root = path.resolve(path.dirname(fs.realpathSync(__filename)), '..');
  if (process.argv.length === 3 && process.argv[2] === '--version') {
    process.stdout.write(`${require(path.join(root, 'package.json')).version}\n`);
    return;
  }
  let runtime;
  try {
    if (process.platform === 'linux') {
      const version = process.report.getReport().header.glibcVersionRuntime;
      const [major, minor] = (version || '').split('.').map(Number);
      if (!(major > 2 || (major === 2 && minor >= 36))) {
        throw new Error('The prebuilt Linux MCP bridge requires glibc 2.36 or newer; musl/Alpine is not supported.');
      }
    }
    runtime = resolveRuntime(root);
  } catch (error) {
    process.stderr.write(`ergo-mcp: ${error.message}\n`);
    process.exitCode = 1;
    return;
  }
  const args = process.argv.slice(2);
  const env = { ...process.env, ERGO_WEB_DIR: process.env.ERGO_WEB_DIR || runtime.web };
  // A remote URL without a token prevents native startup, so leave unauthenticated
  // local editing alone while making hosted login/publishing work without a URL.
  if (!env.ERGO_PUBLISH_URL && (args.includes('--login') ||
    args.includes('--publish-token-file') || env.ERGO_PUBLISH_TOKEN || env.ERGO_PUBLISH_TOKEN_FILE)) {
    env.ERGO_PUBLISH_URL = 'https://ergosandbox.com';
  }
  const child = spawn(runtime.binary, args, {
    stdio: 'inherit',
    shell: false,
    env,
  });
  // stdout remains exclusively native MCP JSON-RPC. Diagnostics use stderr.
  const signals = new Map();
  for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
    const forward = () => child.kill(signal);
    signals.set(signal, forward);
    process.on(signal, forward);
  }
  function cleanup() {
    for (const [signal, handler] of signals) process.removeListener(signal, handler);
  }
  child.once('error', (error) => {
    cleanup();
    process.stderr.write(`ergo-mcp: could not start the native bridge: ${error.message}\n`);
    process.exitCode = 1;
  });
  child.once('exit', (code, signal) => {
    cleanup();
    if (signal) process.kill(process.pid, signal);
    else process.exitCode = code ?? 1;
  });
}

if (require.main === module) main();
module.exports = { resolveRuntime, PLATFORMS };
