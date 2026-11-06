# Codex and Claude Code plugin

`plugins/ergo` packages the stdio MCP connection and a build/inspiration skill for
both clients. It contains no credentials, hooks, auto-installed executables or
remote shell endpoint. The human still approves browser delegation.

These are **source plugin packages**, not a published marketplace listing. Install
a current rebuilt/assembled Ergo MCP bridge and its matching browser UI first;
published v0.5.25 does not gain browser token pickup from installing this wrapper.
Node 20+ and Chrome/Chromium are required for Ergo; the host clients have their own
requirements. `ergo-mcp` must be on the host's PATH. The plugin starts it with
`--headless` and the canonical hosted origin; normal startup opens human sign-in
if credentials are missing or rejected. For clients with short startup deadlines,
complete `ergo-mcp --login` first, then reconnect. Secrets live in the bridge's
private origin-specific token file, never the plugin manifests or chat.

## Claude Code

From this checkout, test the plugin directly:

```sh
claude --plugin-dir ./plugins/ergo
```

The repo's `.claude-plugin/marketplace.json` also advertises the plugin for a local
marketplace install. `plugins/ergo/.claude-plugin/plugin.json` and `.mcp.json` use
Claude's supported layout. `/ergo:build` provides the workflow; the same skill
can load for LEGO/Ergo building tasks.

```sh
claude plugin validate plugins/ergo
```

## Codex

`plugins/ergo/plugin.json` and `mcp.json` use the portable Agent Plugins 1.0.0
schemas. `.agents/plugins/marketplace.json` advertises the plugin from this repo;
install it from the local marketplace in a supported Codex host. This is a
**local/desktop stdio integration**, not a cloud/ChatGPT remote MCP service or a
public plugin-directory submission. A host without local executable support
cannot run the bridge merely by reading its manifest.

## State and permissions

The default workspace is `construction.ergo.json` in the client's working
directory. Use separate writable folders/cache paths for concurrent MCP instances;
the bridge deliberately locks each working copy. Plugin installation does not
authorize edits. The remote server still checks identity, permissions, pacing and
physical acceptance. Inspect confirmed `shared_commit` receipts, not local JSON,
to establish publication.

The bundled skill searches existing sets by feature, retrieves and inspects
references with attribution, then builds or opens an existing shared construction.
It follows an inspect–act–review–adjust harness, using `get_build_feedback`, real
rendered images and human feedback at milestones rather than treating validation
as a design-quality verdict. Suggestions are disclosed heuristics; no automatic
aesthetic pass or dynamics loop is introduced.
See [model search](model-library.md) and [sign-in](access-and-onboarding.md).
