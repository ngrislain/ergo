# Agent client plugins

`plugins/` packages the stdio MCP connection and a build/inspiration skill for
Claude Code, ChatGPT and Codex (desktop), OpenCode and Pi. It contains no
credentials, hooks or remote shell endpoint. All marketplace plugins select the
version-matched `@ngrislain/ergo-mcp` bridge without a global install. OpenCode
and Pi declare the bridge as an npm dependency. Claude Code prefers its locked
plugin-local copy and otherwise uses `npm exec` with the exact version. OpenAI
desktop hosts launch the same pinned version via `npm exec`. The human still
approves browser delegation.

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

The OpenCode and Pi plugins are published on npm at **0.5.38** and can be
installed without a repository checkout or a global bridge installation. The
Claude Code and OpenAI packages remain source/repo-marketplace distributions,
not public marketplace listings.
Node 20+ and Chrome/Chromium are required for Ergo; the host clients have their own
requirements. When the OpenCode or Pi package is installed from npm or Git, the
pinned `@ngrislain/ergo-mcp` dependency supplies the bridge and the plugin launches
it from its own `node_modules`; checkout-local OpenCode/Pi loads can fall
back to `ergo-mcp` on PATH. Every package starts the bridge with `--headless` and the canonical
hosted origin; normal startup opens human sign-in if credentials are missing or
rejected. Since v0.5.38 the handshake no longer waits for that
approval; tool calls made before it finishes return the sign-in link (see
[startup](local-construction-mcp.md#install-and-configure)). With v0.5.37 and
earlier, hosts with short startup deadlines (Claude Code allows 30 s) need sign-in
completed first with any bridge copy — `ergo-mcp --login` from a global install,
or `npx @ngrislain/ergo-mcp@0.5.37 --login` — then a reconnect; the
origin-scoped token file is shared by every copy. Secrets live in that private token file,
never the plugin manifests or chat.

The public [website setup guide](https://www.ergosandbox.com/#connect) uses npm
for OpenCode/Pi and remote marketplace registration for Claude Code/Codex;
no manual checkout is needed. The bridge requests human browser approval when
needed, with an optional login command for short host startup deadlines. Its
manual MCP tab remains a separate, verified-release installation path.
Marketplace launchers that use `npm exec` require npm on the host's PATH and
may need registry access on first launch. They disable npm lifecycle scripts,
but do not bypass registry configuration or package-firewall policy.

## Claude Code

Register the repository marketplace directly and install the plugin:

```sh
claude plugin marketplace add ngrislain/ergo
claude plugin install ergo@ergo
```

Claude Code fetches the repository internally; Git is required, but no manual
clone is needed. The repo's `.claude-plugin/marketplace.json` advertises the
plugin. `plugins/ergo/.claude-plugin/plugin.json` and `.mcp.json` use
Claude's supported layout. `/ergo:build` provides the workflow; the same skill
can load for LEGO/Ergo building tasks. When Claude Code copies a marketplace
plugin into its cache it also installs the dependencies pinned by
`plugins/ergo/package.json` and `npm-shrinkwrap.json`, so the matching bridge
arrives with the plugin when that installation succeeds. If it is skipped or
the cached bridge is missing or mismatched, Claude's launcher uses npm to run
the pinned bridge without a global install. Developers can test a checkout with
`claude --plugin-dir ./plugins/ergo` or add it as a local marketplace with
`/plugin marketplace add ./` and `/plugin install ergo@ergo`.

```sh
claude plugin validate plugins/ergo
```

## Codex and ChatGPT desktop

`plugins/ergo/plugin.json` and `mcp.json` use the portable Agent Plugins 1.0.0
schemas, with OpenAI-host presentation metadata under `extensions.com.openai`.
`.agents/plugins/marketplace.json` is the repo marketplace that the ChatGPT
desktop app and Codex read. Register it with
`codex plugin marketplace add ngrislain/ergo` (Git required; the CLI fetches the
marketplace), then select the Ergo marketplace and install the plugin in the
supported desktop Plugins Directory. These hosts do not install the plugin's
npm dependencies; `mcp.json` invokes npm to run the pinned local
stdio bridge. This is a **local/desktop stdio integration**, not a cloud/remote
MCP service; a host without local executable support cannot run the bridge
merely by reading its manifest. Publishing to the shared public plugin directory
would require OpenAI review, and local-MCP submissions are limited to approved
partner organizations; this repository documents only the repo-local marketplace
path and direct repository registration, not a public directory submission.

## OpenCode

`plugins/opencode-ergo` is an OpenCode server plugin that registers the `ergo`
local MCP server — with the startup/catalog/execution timeouts from the
[MCP client guide](local-construction-mcp.md#opencode) — and the `ergo-build`
skill (the same build workflow, discovered by `@ergo-build`). Install the
published package by adding it to `opencode.json(c)`:

```jsonc
{
  "$schema": "https://opencode.ai/config.json",
  "plugins": ["@ngrislain/opencode-ergo@0.5.38"]
}
```

OpenCode installs the package and its pinned `@ngrislain/ergo-mcp` dependency,
so no Git clone or global bridge install is needed. Approve browser sign-in
when the bridge requests it. For development, a checkout-local entry
`"./plugins/opencode-ergo"` skips dependency installation and uses the PATH
bridge instead. The manual one-shot MCP configuration
in the [MCP client guide](local-construction-mcp.md#opencode) remains available
without the plugin; both register the identical bridge command and environment.

## Pi

`plugins/ergo-pi` is a [Pi package](https://pi.dev/docs/latest/packages.md): the
build skill under `skills/build` plus an extension that registers the `ergo`
stdio server with the same command, environment and a 3,900-second per-request
timeout. Install the published package:

```sh
pi install npm:@ngrislain/ergo-pi@0.5.38
```

Pi installs the pinned `@ngrislain/ergo-mcp` dependency with the package, so no
Git clone or global bridge install is needed. Approve browser sign-in when
requested. For development, `pi install ./plugins/ergo-pi` loads the checkout;
a local-path install skips dependency installation and uses the PATH bridge
instead. Verify the connection
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
