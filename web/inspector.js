const $ = id => document.getElementById(id);

function localInspector(value) {
  const url = new URL(value);
  if (!['http:', 'https:'].includes(url.protocol) ||
      !['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname) ||
      url.username || url.password || url.pathname !== '/' ||
      !url.port || Number(url.port) < 1) {
    throw Error('Use the root URL of a local Inspector on localhost or a loopback address, with its port.');
  }
  return url.href;
}

$('download-config').addEventListener('click', () => {
  const config = {mcpServers: {ergo: {
    command: 'ergo-mcp', args: ['--headless', '--workspace', 'inspector.cache.json'],
    env: {ERGO_PUBLISH_URL: location.origin},
  }}};
  const url = URL.createObjectURL(new Blob([JSON.stringify(config, null, 2)], {type: 'application/json'}));
  const link = document.createElement('a'); link.href = url; link.download = 'ergo-inspector.json';
  link.click(); setTimeout(() => URL.revokeObjectURL(url), 1000);
});

$('connect').addEventListener('submit', event => {
  event.preventDefault(); $('error').hidden = true;
  try {
    const url = localInspector($('inspector-url').value);
    $('inspector-frame').src = url; $('inspector-frame').hidden = false;
    $('open-inspector').href = url; $('open-inspector').hidden = false;
    $('status').textContent = 'Inspector frame requested. If it stays blank or is blocked, use the separate-tab link. Embedded loading does not prove an MCP connection is established.';
  } catch (error) {
    $('error').textContent = error.message; $('error').hidden = false;
  }
});
