'use strict';

const fs = require('node:fs');
const path = require('node:path');
const { spawn } = require('node:child_process');

const name = '@ngrislain/ergo-mcp';
const version = require('./package.json').dependencies[name];
const local = path.join(__dirname, 'node_modules', name);
const entry = path.join(local, 'bin', 'ergo-mcp.cjs');
const args = process.argv.slice(2);
let installed = false;
try {
  installed = require(path.join(local, 'package.json')).version === version
    && fs.statSync(entry).isFile();
} catch (error) {
  if (error.code !== 'MODULE_NOT_FOUND' && error.code !== 'ENOENT') throw error;
}

// Do not depend on a global ergo-mcp, and never run a mismatched cached copy.
// npm's ordinary registry and package-policy checks still apply.
const [command, ...argv] = installed
  ? [process.execPath, entry, ...args]
  : ['npm', 'exec', '--yes', '--ignore-scripts', '--', `${name}@${version}`, ...args];
const child = spawn(command, argv, { stdio: 'inherit' });
const handlers = ['SIGINT', 'SIGTERM', 'SIGHUP'].map(signal => {
  const forward = () => child.kill(signal);
  process.on(signal, forward);
  return [signal, forward];
});
function cleanup() {
  for (const [signal, handler] of handlers) process.removeListener(signal, handler);
}
child.once('error', error => {
  cleanup();
  process.stderr.write(`Ergo plugin: could not start the pinned MCP bridge: ${error.message}\n`);
  process.exitCode = 1;
});
child.once('exit', (code, signal) => {
  cleanup();
  if (signal) process.kill(process.pid, signal);
  else process.exitCode = code ?? 1;
});
