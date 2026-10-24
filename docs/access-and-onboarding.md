# Sign-in, agent access, and shared-world edit pacing

## Landing page and workbench

The static public guide at **https://www.ergosandbox.com** explains the
human-to-agent handoff and supplies a machine-readable `/llms.txt`. Its privacy
and terms pages describe the hosted service. Sign-in, tokens, and the API use
the separate canonical sandbox origin **https://ergosandbox.com**; the public
guide never receives credentials. See [website deployment](static-site.md).

On the authenticated apex, anonymous `/` visitors see Google sign-in, while
signed-in users see the viewer. `/app` is also an explicit viewer route.
`/login` always shows sign-in, MCP setup, token issuance, and direct API examples,
including the `/login#connect` agent handoff. Signed-in users can inspect their current edit
allowance, and explicitly generate an access token. The token can be copied or
downloaded as `ergo-mcp-token`; it is held in page memory, not local/session
storage. Downloaded token files are operator-managed credentials.

The production OpenTofu deployment requires Google credentials. With Google
unset, the explicitly labeled development account can use the local workbench
and exercise token issuance without a real Google login.
The local development root retains the setup page; `/app` opens its viewer.

Shared-world URLs use `/app?shared_world=WORLD_UUID&x=X&y=Y&z=Z`, where X/Y/Z
are public storage-block coordinates. Google sign-in preserves these links via
a signed, same-origin `return_to` restricted to `/`, `/login`, and `/app`.
No token is embedded in these URLs. Private snapshots and examples remain in
the construction browser; permission-filtered shared-world regions are listed
separately with paged discovery.

The shared browser view loads a bounded 3×3×3 block region, at most 5,000 pieces
(or the smaller configured working-set limit), and a 16 MiB CBOR read budget.
It moves with the camera; the client checks the small head every five seconds
and reloads geometry only when the region or accepted revision changes. Walk
navigation treats unqueried cells as unloaded until their region arrives.
This is not an entire-world export or a complete mechanical assembly: simulation
and editing of the clipped view are rejected. Shared edits use the authenticated
MCP/REST workflow; a complete shared-world CAD editor remains future work.

Use [full-screen navigation](navigation.md#full-screen-exploration) to hide the
surrounding workbench. **Nearest construction** searches accessible private
construction footprints and currently discovered shared regions using camera
GPS coordinates. Load additional shared discovery pages with **More shared
regions**. Discovery hints can be coarse or stale; the actual spatial view is
read from an authorized accepted world revision. This is not a global exact
nearest-neighbor index over every piece in an unbounded world.

## MCP setup and browser-assisted sign-in

Build the native bridge and WASM assets using the root README instructions.
To open this server's sign-in/setup page from the terminal:

```sh
target/debug/ergo-mcp --login --publish-url https://ergosandbox.com
```

This one-time command opens `/login#connect` and exits. Sign in in the browser,
generate/download the token, then configure its path when starting the MCP
bridge normally. It does not silently exchange browser cookies or automatically
install credentials in an agent's configuration.

```sh
ERGO_PUBLISH_URL=https://ergosandbox.com \
ERGO_PUBLISH_TOKEN_FILE=/absolute/path/ergo-mcp-token \
target/debug/ergo-mcp --workspace /absolute/path/construction.ergo.json
```

The landing page provides the equivalent MCP-client JSON configuration. See
[MCP tools](local-construction-mcp.md#remote-publication-setup) for publication,
read-back, conflict, and unknown-outcome semantics.

## Tokens and direct API access

`POST /api/auth/mcp-token` is session-authenticated and requires
`X-Ergo-Request: 1`. New tokens delegate shared-world reads, creation, assembly
imports, and geometry commits. They cannot change sharing/grants, retention,
account entitlements, or private snapshots. Defaults are one hour, at most
86400 seconds, capped at the issuing session's expiry. They are purpose- and
origin-bound and are invalidated by signing-key rotation, not individual logout.
Older publication-only tokens still work for reads/create/import, but cannot
perform geometry commits; obtain a new token for editing.

For example:

```sh
export ERGO_URL=https://ergosandbox.com
# Set ERGO_TOKEN to the token copied/downloaded after sign-in.
curl --fail "$ERGO_URL/api/shared-worlds/allowance" \
  -H "Authorization: Bearer $ERGO_TOKEN"
curl --fail "$ERGO_URL/api/shared-worlds?quad=&limit=32" \
  -H "Authorization: Bearer $ERGO_TOKEN"
```

Writes also send `X-Ergo-Request: 1` and, for JSON, `Content-Type: application/json`.
Create a collaborative world with
`POST /api/shared-worlds` and `{name, geo, authenticated_editing: true}`.
Import a bounded `WorldExport` with `POST /api/shared-worlds/{id}/assemblies`
and `{base, scene}`; edit existing geometry with
`POST /api/shared-worlds/{id}/commits`. Keep the returned accepted revision and
stable piece mapping. See [the REST workflow](concurrent-global-editing.md#rest-workflow)
for the full paging/commit format.

`authenticated_editing: true` permits any signed-in identity to read/edit that
world, subject to pacing and physical validation. MCP-created worlds default
to this setting; use `authenticated_editing: false` to create an owner/grant-only
world. Raw REST creation defaults to owner/grant-only for compatibility. Only
the owner can toggle the setting through the grants endpoint; current policy is
validated atomically with publication. Paid status never bypasses permissions.

## Free-user pacing and paid entitlement

Free accounts receive a persistent token bucket with capacity **5,000 changed
bricks**, refilling by **5,000 changes per 86400 seconds**. This permits an
initial burst and then continuous refill, approximately one change every
17.28 seconds when exhausted; it is not a midnight reset or a lifetime/world
brick-count cap. It applies across worlds for the same authenticated actor.

Configure `ERGO_EDIT_BRICKS_PER_PERIOD` and `ERGO_EDIT_PERIOD_SECONDS`, or the
OpenTofu inputs `free_bricks_per_period` and `edit_period_seconds`. Normal
per-operation memory/piece limits and server validation remain independent.
Import costs one change per imported piece. Geometry commits cost the number
of explicitly changed/deleted pieces; rules/joint-only commits charge the
affected piece domain. World creation and permission/retention metadata changes
do not spend brick-change credits. Invalid drafts rejected before publication
preparation do not spend credits.

Reservations use conditional object-store updates, so concurrent processes
cannot overspend one actor's allowance. Confirmed 4xx publication failures
refund idempotently; uncertain 5xx/cancelled publication outcomes conservatively
retain reservations and the normal refill continues. No failed request is
automatically resubmitted. Exhaustion returns HTTP **429**, with `Retry-After`
and `retry_after_seconds` when a smaller accepted request can refill. A request
larger than the burst capacity must be split or use an unlimited entitlement.

`GET /api/shared-worlds/allowance`, session `/api/auth/me`, and MCP
`get_edit_allowance` expose remaining whole changes, capacity, refill period,
or `unlimited: true` for an unlimited account. These are not permissions to
publish an invalid or unauthorized construction.

Entitlements are server-owned object-store records, separate from Google profile
updates and bearer claims. With the **same storage environment as production**,
an operator can set an account's plan using the account ID shown on the setup page:

```sh
cargo run -p ergo-server -- user-plan google-USER_ID paid
cargo run -p ergo-server -- user-plan google-USER_ID free
```

The paid entitlement bypasses pacing, including after cold starts, while keeping
all authorization, working-set, and measured-physics checks. A billing provider,
checkout, and verified payment-webhook synchronization are **not implemented**;
the entitlement is currently operator-managed. Native/end-to-end tests verify
persistence, refill, concurrency, rejection, and paid-plan bypass without using
real Google accounts or payment/S3 resources.
