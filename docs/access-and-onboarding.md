# Sign-in and agent access

The Sandbox at https://ergosandbox.com/ is public to view. Editing and private
account context require sign-in. The project guide at https://www.ergosandbox.com/
does not receive credentials or proxy the API.

## Browser setup

`/` and `/app` open the shared-world workbench. `/login#connect` contains Google
sign-in, token issuance/download and client examples. The Shared-world navigation
panel's **Edit with MCP / API** link opens that setup page, not a fragment on the
world viewer. Signed-in users can generate, copy or download a token there;
guests must sign in first. **Edit here with an agent**
carries the current view target and selected construction ID to setup; no token
is embedded in that URL.

When a current-source MCP bridge needs credentials, it opens `/login` with a
one-time `mcp_callback` URL and waits up to ten minutes. The browser accepts only
an HTTP `127.0.0.1` callback with a nonzero port and a 64-hex-character capability
path. **Authorize MCP server** explicitly delegates editing to the local process;
Google credentials stay in the human's browser. The token is returned in a form
POST body, never a URL. The listener checks the exact application Origin and
loopback Host, accepts one token, and closes. The bridge verifies the token with
the configured API before saving it atomically with Unix mode 0600.

The default token path is `$XDG_CONFIG_HOME/ergo/tokens/SHA256_ORIGIN.token`, or
`~/.config/ergo/tokens/SHA256_ORIGIN.token`. An explicit
`--publish-token-file`/`ERGO_PUBLISH_TOKEN_FILE` takes precedence over an environment
token and the default cache. Missing/empty files and HTTP 401 prompt browser setup;
permission errors, HTTP 403 and server/network failures do not silently trigger
new delegation. An explicit `ERGO_PUBLISH_TOKEN` remains an override: remove or
update a rejected environment value after renewal so future runs can use the saved
file. No credential is written to construction/cache JSON or MCP stdout.
Loopback servers also prompt when authentication is enabled; unauthenticated
development servers may continue without a token. Saved loopback credentials
are reused on subsequent connections.

Browser pickup requires both the updated bridge and browser UI. It is implemented
in v0.5.26, not retroactively present in older v0.5.25 assets. Manual
copy/download remains supported. If an MCP host's startup deadline is too short
for human sign-in, run `ergo-mcp --login` before connecting it.

Sign out immediately displays **Signing out…**, disables duplicate clicks and
clears the session/login cookies before returning to setup. Failures restore the
control. Sign-out does not revoke an already-issued delegated token.

## Install and delegate

Install the current prebuilt client from npm:

```sh
npm install --global --ignore-scripts @ngrislain/ergo-mcp@0.5.27
ergo-mcp --login --publish-url https://ergosandbox.com
```

Sign in in the human's browser, generate/download the token, and configure only its
private file path. Do not pass Google or S3 credentials to the agent. The bundle
includes native/WASM assets; consumers need no Rust build.

Tokens are purpose-, origin- and expiry-bound. The UI and API default to 365 days
(31,536,000 seconds), which is also the maximum; the API accepts shorter explicit
lifetimes. A valid browser session is required at issuance, but its subsequent
expiry does not shorten the delegation. The current
`ergo-mcp-edit-v1` purpose permits shared reads/imports/edits and private own-user
placement context, subject to authorization. It cannot mint another delegation,
change grants/retention/account plans or create another hosted world.

Writes require `X-Ergo-Request: 1` and same-origin validation. JSON writes also use
`Content-Type: application/json`. Read/setup/session responses are no-store.

## Pacing

The default persistent token bucket permits a 10,000-change burst and refills 10,000
changes per 86,400 seconds. This is neither a midnight reset nor a world-size cap.
Existing ledgers retain their remaining credits when the configured capacity
changes; they then refill at the new rate, up to the new capacity.
Imports charge their pieces; edits charge their changed/affected domain. Confirmed
rejections refund reservations; uncertain outcomes retain them until resolution/
refill and are not automatically repeated.

`get_edit_allowance` reports remaining changes or `unlimited:true`. An operator
can set an account's unlimited plan with `ergo-server user-plan USER_ID paid`.
The operator-owned unlimited plan enables god mode: no pacing quota and no
authoring brick-count cap. The response includes `god_mode:true` and
`authoring_piece_limit:null`. Permissions and physical acceptance still apply,
as do encoded-byte/request budgets. Browser streaming and dynamics have separate
working-set limits; they do not cap the number of bricks an account can build.

## Position context

The signed-in viewer stores its last camera/target privately. `get_user_position`
returns the caller's context and ground-level transform; it cannot read another
user by supplying an ID. No public position catalogue is exposed.

See [MCP setup](local-construction-mcp.md), [identity](identity-and-ownership.md),
[navigation](navigation.md) and [agent workflows](agent-shared-editing.md).
