# Sign-in and agent access

The Sandbox at https://ergosandbox.com/ is public to view. Editing and private
account context require sign-in. The project guide at https://www.ergosandbox.com/
does not receive credentials or proxy the API.

## Browser setup

`/` and `/app` open the shared-world workbench. `/login#connect` contains Google
sign-in, token issuance/download and client examples. **Edit here with an agent**
carries the current view target and selected construction ID to setup; no token
is embedded in that URL.

Sign out immediately displays **Signing out…**, disables duplicate clicks and
clears the session/login cookies before returning to setup. Failures restore the
control. Sign-out does not revoke an already-issued delegated token.

## Install and delegate

Install the current prebuilt client from its GitHub Release:

```sh
npm install --global --ignore-scripts 'https://github.com/ngrislain/ergo/releases/download/v0.5.1/ngrislain-ergo-mcp-0.5.1.tgz'
ergo-mcp --login --publish-url https://ergosandbox.com
```

Sign in in the human's browser, generate/download the token, and configure only its
private file path. Do not pass Google or S3 credentials to the agent. The bundle
includes native/WASM assets; consumers need no Rust build.

Tokens are purpose-, origin- and expiry-bound. The UI requests one hour; the API
allows at most 86,400 seconds capped by the issuing session. The current
`ergo-mcp-edit-v1` purpose permits shared reads/imports/edits and private own-user
placement context, subject to authorization. It cannot mint another delegation,
change grants/retention/account plans or create another hosted world.

Writes require `X-Ergo-Request: 1` and same-origin validation. JSON writes also use
`Content-Type: application/json`. Read/setup/session responses are no-store.

## Pacing

The default persistent token bucket permits a 5,000-change burst and refills 5,000
changes per 86,400 seconds. This is neither a midnight reset nor a world-size cap.
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
