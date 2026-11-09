# Agent client plugins

`plugins/` packages the stdio MCP connection and a build/inspiration skill for
Claude Code, ChatGPT and Codex (desktop), OpenCode and Pi. It contains no
credentials, hooks or remote shell endpoint. Every package but the OpenAI
desktop one installs the version-matched `@ngrislain/ergo-mcp` bridge
automatically: the OpenCode and Pi packages declare it as an npm dependency of
the plugin, and the portable package's `package.json` plus
`npm-shrinkwrap.json` let Claude Code install it into the plugin cache. The
OpenAI desktop hosts have no dependency installer and use a globally installed
bridge on PATH. The human still approves browser delegation.

`plugins/ergo` is the portable Agent Plugins 1.0.0 package shared by Claude Code
and the OpenAI desktop clients: root `plugin.json`, root `mcp.json` and
`skills/build/SKILL.md`. `plugins/opencode-ergo` (an OpenCode server plugin) and
`plugins/ergo-pi` (a Pi package) register the same bridge configuration and
skill text; release tests in `scripts/test-distribution.py` keep the copies
identical to the portable package and pin every bridge dependency to the
workspace version. `plugins/ergo/package.json` and `npm-shrinkwrap.json` stay
in lockstep with the release through `python3 scripts/package-mcp.py
plugin-lock`; the lock pins an exact version with an https registry URL and no
integrity, which npm and Claude Code both install and record on first use.

These are **source plugin packages**, not a published marketplace listing. Install
a current rebuilt/assembled Ergo MCP bridge and its matching browser UI first;
published v0.5.25 does not gain browser token pickup from installing this wrapper.
Node 20+ and Chrome/Chromium are required for Ergo; the host clients have their own
requirements. When the OpenCode or Pi package is installed from npm or Git, the
pinned `@ngrislain/ergo-mcp` dependency supplies the bridge and the plugin launches
it from its own `node_modules`; checkout-local loads, failed dependency installs
and the portable package fall back to a globally installed `ergo-mcp` on the
host's PATH. Every package starts the bridge with `--headless` and the canonical
hosted origin; normal startup opens human sign-in if credentials are missing or
rejected. For hosts with short startup deadlines, complete sign-in first with any
bridge copy — `ergo-mcp --login` from a global install, or
`npx @ngrislain/ergo-mcp@0.5.35 --login` — then reconnect; the origin-scoped
token file is shared by every copy. Secrets live in that private token file,
never the plugin manifests or chat.

## Claude Code

From this checkout, test the plugin directly:

```sh
claude --plugin-dir ./plugins/ergo
```

The repo's `.claude-plugin/marketplace.json` also advertises the plugin for a local
marketplace install. `plugins/ergo/.claude-plugin/plugin.json` and `.mcp.json` use
Claude's supported layout. `/ergo:build` provides the workflow; the same skill
can load for LEGO/Ergo building tasks. When Claude Code copies a marketplace
plugin into its cache it also installs the dependencies pinned by
`plugins/ergo/package.json` and `npm-shrinkwrap.json`, so the matching bridge
arrives with the plugin; the install is skipped if it exceeds Claude Code's
60-second budget or the lock is out of sync, and the documented global
install remains the manual fallback.

```sh
claude plugin validate plugins/ergo
```

## Codex and ChatGPT desktop

`plugins/ergo/plugin.json` and `mcp.json` use the portable Agent Plugins 1.0.0
schemas, with OpenAI-host presentation metadata under `extensions.com.openai`.
`.agents/plugins/marketplace.json` is the repo marketplace that the ChatGPT
desktop app and Codex read; install the plugin from that local source in the
Plugins Directory. Like Claude's `.mcp.json`, these OpenAI hosts run the local
stdio bridge. This is a **local/desktop stdio integration**, not a cloud/remote
MCP service; a host without local executable support cannot run the bridge
merely by reading its manifest. Publishing to the shared public plugin directory
would require OpenAI review, and local-MCP submissions are limited to approved
partner organizations; this repository documents only the repo-local marketplace
path.

## OpenCode

`plugins/opencode-ergo` is an OpenCode server plugin that registers the `ergo`
local MCP server — with the startup/catalog/execution timeouts from the
[MCP client guide](local-construction-mcp.md#opencode) — and the `ergo-build`
skill (the same build workflow, discovered by `@ergo-build`). From this checkout,
load it directly in `opencode.json(c)`:

```jsonc
{
  "$schema": "https://opencode.ai/config.json",
  "plugins": ["./plugins/opencode-ergo"]
}
```

Once the package is published to npm, listing `"@ngrislain/opencode-ergo"` or
`opencode plugin add` works the same way; the pinned `@ngrislain/ergo-mcp`
dependency is installed with it, so no global bridge install is needed. A
checkout-local load skips dependency installation and uses the PATH bridge
instead. The manual one-shot MCP configuration
in the [MCP client guide](local-construction-mcp.md#opencode) remains available
without the plugin; both register the identical bridge command and environment.

## Pi

`plugins/ergo-pi` is a [Pi package](https://pi.dev/docs/latest/packages.md): the
build skill under `skills/build` plus an extension that registers the `ergo`
stdio server with the same command, environment and a 3,900-second per-request
timeout. Install it from this checkout:

```sh
pi install ./plugins/ergo-pi
```

Once published, `pi install npm:@ngrislain/ergo-pi` (or a git source) works the
same way and installs the pinned `@ngrislain/ergo-mcp` dependency with the
package, so no global bridge install is needed. A local-path install skips
dependency installation and uses the PATH bridge instead. Verify the connection
with `pi mcp list`, inspect it with `/mcp`, and
load the workflow with `/skill:build`. A user-level `mcp.json` entry configured
through `pi mcp add ergo --env ERGO_PUBLISH_URL=https://ergosandbox.com -- ergo-mcp --headless`
reaches the same bridge without the package and takes precedence over the
extension's registration.

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
