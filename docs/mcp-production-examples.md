# Build with MCP and publish to production

The MCP process runs beside your agent. It builds a local working copy using
the bundled browser/WASM engine and publishes through the authenticated production
API at **https://ergosandbox.com**. Only the server writes S3. There is one public
Ergo Sandbox; accepted current-client edits update it automatically.

## Install and sign in

Use Node 20+ and the [prebuilt package](distribution.md); no Rust build is needed:

```sh
export ERGO_MCP_PACKAGE='https://github.com/ngrislain/ergo/releases/download/v0.4.0/ngrislain-ergo-mcp-0.4.0.tgz'
npm install --global "$ERGO_MCP_PACKAGE"
ergo-mcp --login --publish-url https://ergosandbox.com
```

In the browser, sign in, generate a token, and download it as `ergo-mcp-token`.
The token is normally valid for one hour. Put its absolute path in the MCP
configuration (the token itself does not belong in this JSON):

```json
{
  "mcpServers": {
    "ergo": {
      "command": "ergo-mcp",
      "args": ["--workspace", "/absolute/path/rocket.ergo.json"],
      "env": {
        "ERGO_PUBLISH_URL": "https://ergosandbox.com",
        "ERGO_PUBLISH_TOKEN_FILE": "/absolute/path/ergo-mcp-token"
      }
    }
  }
}
```

Use v0.4.0+ for shared in-place editing, LDraw submodels, current-position handoff,
and canonical storage. v0.3.x uses the old manual-publication protocol.

## Ask your agent

> Use Ergo MCP to build a small pixel rocket, a compass plaza, and a garden
> courtyard. Use common parts and colors. Keep these as three separate local
> constructions; inspect real isometric and top images, check overlap and static
> stability, and run the short gravity test. Save each as a shared
> construction in the one public Ergo Sandbox using my configured token. Return the confirmed viewer URL,
> accepted revision, and authenticated author name for each.

For example, the first ordinary brick uses this MCP tool call:

```json
{"name":"place_brick","arguments":{"part":"3003","color":4,"min_corner":[8,8,0]}}
```

Positions are `[east,north,height]` in construction units, measured at the part's
minimum corner. `quarter_turns` rotates upright parts around vertical Z.
After adding the remaining pieces:

```json
{"name":"view_construction","arguments":{"views":["isometric","top"],"width":640,"height":480}}
{"name":"test_stability_by_gravity","arguments":{"steps":1}}
{"name":"validate_construction","arguments":{}}
```

Read `publication_ready` and `revision` from validation. Then explicitly publish:

```json
{"name":"publish_construction","arguments":{"name":"My rocket","expected_local_revision":1}}
```

Replace `1` with the actual local revision. Normal production edits already commit
to the same shared assembly; this call can confirm/rename it, or admit an existing
validated proposal. It cannot create a private/alternative world and never bypasses
validation or pacing. See [position-based editing](agent-shared-editing.md).

## Repeatable demonstration

The repository's [`scripts/demo-mcp.py`](../scripts/demo-mcp.py) drives genuine
MCP stdio calls against the installed executable. It uses only Python's standard
library; Chrome/Chromium is needed for the bridge's `--headless` runtime.

From the repository root:

```sh
# Build locally, validate, and produce actual PNGs. No production writes.
python3 scripts/demo-mcp.py build --output target/my-mcp-demo

# Inspect *-0.png (isometric) and *-1.png (top), then publish the same files.
python3 scripts/demo-mcp.py publish --output target/my-mcp-demo \
  --token-file /absolute/path/ergo-mcp-token
```

For development builds, add `--binary target/debug/ergo-mcp` to both commands.
The three designs use ground-supported common parts: a pixel rocket, compass
arrows, and an open garden courtyard. They exercise placement, rotations,
images, static validation, a non-mutating gravity probe and production acceptance
without assuming unmeasured clutch strength for stacked joints.

`results.json` records working-file hashes, validation, gravity reports and
confirmed publication metadata. Changed working files are rejected at publication.
A journal marks each publication attempt before it is sent. Confirmed attempts
are not repeated; an interrupted or uncertain attempt must be resolved using its
returned world/revision information before trying again.

## Accepted revision and author attribution

The demonstration was executed against production on 2026-10-24: Pixel rocket
(29 bricks), Compass plaza (29), and Garden courtyard (30). All three returned
confirmed world/assembly read-backs and the authenticated author name. The local
run's PNGs, workspace files and acceptance receipts are in
`target/prod-mcp-demo-20261024/` (generated, not versioned). The three constructions
were subsequently copied to the single common sandbox at distinct positions;
their shared receipts are in `target/common-world-demo-20261024/`. The former
private-world copies are retired by the single-world cleanup.

Success requires `published: true` after the MCP tool reads back both the
accepted world revision and assembly. The result includes `view_url`,
`accepted_revision`, `assembly_id`, and `audit`.

`audit.actor` is the authenticated stable user ID; current production builds also
record the server-resolved `audit.actor_name`. Piece births record `created_by`
and `created_by_name`. An agent cannot select another author by changing the
construction name. Those immutable CBOR records are stored in S3 by the server.
This is authenticated attribution, not a personal cryptographic signature by a
username. Account display names need not be unique. Older accepted revisions
may have only the stable ID.

Do not interpret a local export, PNG, or a merely attempted HTTP request as
confirmed remote publication. See [publication outcomes](local-construction-mcp.md)
and [identity and ownership](identity-and-ownership.md) for the full semantics.
