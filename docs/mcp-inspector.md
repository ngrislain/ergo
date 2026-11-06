# Official MCP Inspector and 3D catalogue

Two dedicated pages are intentionally absent from the normal workbench navigation:

- `/catalogue` (also `/catalogue/`) browses the canonical supported pieces and
  colors. It renders genuine source meshes in WebGL2, supports rotation/zoom,
  family and connection filters, palette previews and source credits. Palette
  preview does not claim every part was manufactured in every color.
- `/mcp-inspector` (also `/mcp-inspector/`) displays an official MCP Inspector
  running on the user's own device. The page ships in the current source; it must
  be deployed before that route exists on a hosted installation.

Both have `noindex,nofollow`; unlisted is not an authorization boundary.

## Inspector setup

Download `ergo-inspector.json` from the page into a writable folder, then run:

```sh
npx --yes @modelcontextprotocol/inspector@2.14.0 --web --config ./ergo-inspector.json
```

Inspector 2.14.0 requires Node `^22.23.4 || ^24.22.1 || >=26.12.1`, separately from
Ergo's Node 20+ baseline. The downloaded configuration starts the installed
`ergo-mcp --headless --workspace inspector.cache.json` against the page's origin.
It includes no credentials. The separate cache avoids locking the normal agent's
working copy. If the host's startup deadline expires during browser sign-in, run
`ergo-mcp --login --publish-url ORIGIN` first.

Enter the loopback URL printed by Inspector and select **Display Inspector**.
Only HTTP(S) root URLs on `127.0.0.1`, `localhost` or `[::1]` with an explicit port
are accepted. The page never probes localhost automatically, stores the URL, or
sends it to Ergo. Optional Inspector session parameters stay in the browser and
local frame. The iframe uses no-referrer and a sandbox; the separate-tab fallback
also uses `noopener noreferrer`.

Browser mixed-content/local-network policies and Inspector's own frame/CSP rules
can block embedding. **Open the official Inspector in its own tab** is the honest
fallback; requesting a frame does not prove an MCP handshake succeeded. App
sandbox rendering may require its full allowed-origin chain and is better tested
in Inspector's own tab. No bypass of authentication or origin protections is
configured by this integration.

Inspector's process-spawning backend remains on loopback with its default
authentication. Ergo does **not** host a public Inspector backend or spawn
commands from page input. Tool calls through Inspector can still publish real
edits under the approved user's identity.

Verification: `python3 scripts/test-inspector.py --official --catalogue` embeds
the pinned official app and runs the real WebGL catalogue checks against a
disposable server. `--official` opts into npm dependency installation/network;
omit it for the offline wrapper/configuration checks.

Implementation: `web/inspector.html`, `web/inspector.js`, `web/inspector.css`,
`web/catalogue.html`, `web/catalogue.js` and routes in `crates/server/src/main.rs`.
